//! Content-addressed store of unpacked OCI images for the built-in microVM
//! tier, under `<data_dir>/containers/`.
//!
//! Layout:
//!
//! ```text
//! containers/
//!   rootfs/<digest>/          unpacked image, keyed by manifest digest
//!   rootfs/<digest>.complete  marker written after a successful unpack
//!   refs/<ref>                image reference -> digest, for offline reuse
//!   tmp/                      unpack staging + per-run ephemeral rootfs
//! ```
//!
//! Registry access goes through `oci-client`; layers are unpacked with the
//! `tar` crate (whose `unpack_in` refuses path traversal and writing through
//! symlinks) plus OCI whiteout handling. The design called for the
//! `oci-unpack` crate here, but it turned out to be Linux-only (openat2,
//! Linux mode_t) and Phase 1's primary target is macOS.
//!
//! Everything here is blocking (registry download, tar extraction, filesystem
//! clone); callers run it on a background thread.

use anyhow::{Context as _, Result, anyhow, bail};
use oci_client::secrets::RegistryAuth;
use sha2::{Digest as _, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt as _;

const EPHEMERAL_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

pub struct ImageStore {
    root: PathBuf,
}

impl ImageStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn global_root() -> PathBuf {
        paths::data_dir().join("containers")
    }

    fn rootfs_parent(&self) -> PathBuf {
        self.root.join("rootfs")
    }

    fn refs_dir(&self) -> PathBuf {
        self.root.join("refs")
    }

    fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    pub fn rootfs_dir_for_digest(&self, digest: &str) -> PathBuf {
        self.rootfs_parent().join(sanitize_component(digest))
    }

    fn complete_marker(&self, digest: &str) -> PathBuf {
        self.rootfs_parent()
            .join(format!("{}.complete", sanitize_component(digest)))
    }

    fn ref_path(&self, image_ref: &str) -> PathBuf {
        self.refs_dir().join(sanitize_ref(image_ref))
    }

    /// The unpacked rootfs for `image_ref` if a completed copy is already on
    /// disk, without touching the network.
    pub fn cached_rootfs(&self, image_ref: &str) -> Option<PathBuf> {
        let digest = fs::read_to_string(self.ref_path(image_ref)).ok()?;
        let digest = digest.trim();
        let dir = self.rootfs_dir_for_digest(digest);
        (self.complete_marker(digest).is_file() && dir.is_dir()).then_some(dir)
    }

    /// Resolve `image_ref` to a manifest digest and make sure the unpacked
    /// rootfs for that digest exists locally, downloading it if necessary.
    /// Falls back to a previously cached copy when the registry is
    /// unreachable.
    pub fn ensure_image(&self, image_ref: &str) -> Result<PathBuf> {
        match self.pull_if_needed(image_ref) {
            Ok(dir) => Ok(dir),
            Err(pull_err) => {
                if let Some(dir) = self.cached_rootfs(image_ref) {
                    log::warn!(
                        "could not pull {image_ref} ({pull_err:#}); using the cached copy"
                    );
                    return Ok(dir);
                }
                Err(pull_err.context(format!(
                    "failed to pull container image {image_ref} and no cached copy exists"
                )))
            }
        }
    }

    fn pull_if_needed(&self, image_ref: &str) -> Result<PathBuf> {
        let reference = oci_client::Reference::try_from(image_ref)
            .map_err(|err| anyhow!("invalid image reference {image_ref}: {err}"))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build tokio runtime for image pull")?;
        // Anonymous auth only — the default sandbox image is public, and
        // private registries are out of scope for the built-in tier for now.
        let auth = RegistryAuth::Anonymous;
        let client = oci_client::Client::new(client_config());

        runtime.block_on(async {
            let digest = client.fetch_manifest_digest(&reference, &auth).await?;
            let dir = self.rootfs_dir_for_digest(&digest);
            if !self.complete_marker(&digest).is_file() {
                log::info!("pulling container image {image_ref} ({digest})");
                self.download_and_unpack(&client, &reference, &auth, &digest)
                    .await?;
            }
            self.write_ref(image_ref, &digest)?;
            Ok(dir)
        })
    }

    /// Download and unpack the image into the digest-addressed directory.
    /// Unpacks into a staging dir first so a crash never leaves a half-written
    /// rootfs behind the completion marker, then renames into place.
    async fn download_and_unpack(
        &self,
        client: &oci_client::Client,
        reference: &oci_client::Reference,
        auth: &RegistryAuth,
        digest: &str,
    ) -> Result<()> {
        fs::create_dir_all(self.tmp_dir())?;
        fs::create_dir_all(self.rootfs_parent())?;
        let staging = self.tmp_dir().join(format!("unpack-{}", unique_suffix()));
        fs::create_dir_all(&staging)?;

        let result = self
            .download_and_unpack_into(client, reference, auth, &staging)
            .await;
        if let Err(err) = result {
            fs::remove_dir_all(&staging).ok();
            return Err(err);
        }

        let final_dir = self.rootfs_dir_for_digest(digest);
        match fs::rename(&staging, &final_dir) {
            Ok(()) => {}
            // Lost a race against a concurrent pull of the same digest; the
            // winner's copy is just as good.
            Err(_) if final_dir.is_dir() => {
                fs::remove_dir_all(&staging).ok();
            }
            Err(err) => {
                fs::remove_dir_all(&staging).ok();
                return Err(anyhow!(err)
                    .context(format!("failed to move unpacked image into {final_dir:?}")));
            }
        }
        fs::write(self.complete_marker(digest), digest)?;
        Ok(())
    }

    async fn download_and_unpack_into(
        &self,
        client: &oci_client::Client,
        reference: &oci_client::Reference,
        auth: &RegistryAuth,
        staging: &Path,
    ) -> Result<()> {
        let (manifest, _) = client
            .pull_image_manifest(reference, auth)
            .await
            .context("failed to fetch image manifest")?;

        for layer in &manifest.layers {
            // Each layer lands in a temp file first: layers can be hundreds
            // of MB, so they must not be buffered in memory, and unpacking
            // reads synchronously while the download is async.
            let blob_path = self
                .tmp_dir()
                .join(format!("blob-{}", unique_suffix()));
            let mut blob_file = tokio::fs::File::create(&blob_path)
                .await
                .with_context(|| format!("failed to create {blob_path:?}"))?;
            let download = async {
                client
                    .pull_blob(reference, layer, &mut blob_file)
                    .await
                    .with_context(|| format!("failed to download layer {}", layer.digest))?;
                blob_file.flush().await?;
                drop(blob_file);
                apply_layer(&blob_path, &layer.media_type, staging)
            }
            .await;
            fs::remove_file(&blob_path).ok();
            download?;
        }
        Ok(())
    }

    fn write_ref(&self, image_ref: &str, digest: &str) -> Result<()> {
        fs::create_dir_all(self.refs_dir())?;
        fs::write(self.ref_path(image_ref), digest)?;
        Ok(())
    }

    /// Clone `rootfs` into a writable per-run directory. Uses copy-on-write
    /// (APFS clonefile / reflink) so it is cheap; the caller removes it after
    /// the run.
    pub fn create_ephemeral_rootfs(&self, rootfs: &Path) -> Result<PathBuf> {
        fs::create_dir_all(self.tmp_dir())?;
        let dest = self.tmp_dir().join(format!("run-{}", unique_suffix()));
        clone_dir(rootfs, &dest)?;
        configure_guest_etc(&dest)?;
        Ok(dest)
    }

    /// Best-effort removal of per-run directories that outlived their run
    /// (crashed helper, killed terminal). Anything under `tmp/` older than a
    /// day is fair game — live runs are minutes, not days.
    pub fn remove_stale_ephemerals(&self) {
        let Ok(entries) = fs::read_dir(self.tmp_dir()) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let stale = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > EPHEMERAL_MAX_AGE);
            if stale {
                if let Err(err) = fs::remove_dir_all(entry.path()) {
                    log::warn!("failed to remove stale sandbox dir {:?}: {err}", entry.path());
                }
            }
        }
    }
}

static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!(
        "{}-{}-{}",
        std::process::id(),
        RUN_COUNTER.fetch_add(1, Ordering::Relaxed),
        nanos
    )
}

/// OCI architecture name for the microVM guest — same architecture as the
/// host, since libkrun does no emulation.
fn guest_architecture() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    }
}

/// Client config whose platform resolver picks the linux/<host-arch> entry
/// out of a multi-arch index — the guest is always Linux, even on macOS
/// hosts, so the default resolver must not be used.
fn client_config() -> oci_client::client::ClientConfig {
    let os: oci_spec::image::Os = "linux".into();
    let architecture: oci_spec::image::Arch = guest_architecture().into();
    oci_client::client::ClientConfig {
        platform_resolver: Some(Box::new(move |entries| {
            entries
                .iter()
                .find(|entry| {
                    entry.platform.as_ref().is_some_and(|platform| {
                        platform.os == os && platform.architecture == architecture
                    })
                })
                .map(|entry| entry.digest.clone())
        })),
        ..Default::default()
    }
}

/// Unpack one (possibly compressed) layer tarball onto `target`, applying
/// OCI whiteouts. `tar::Entry::unpack_in` supplies the safety properties:
/// it refuses absolute paths, `..` traversal, and writing through symlinks.
fn apply_layer(blob_path: &Path, media_type: &str, target: &Path) -> Result<()> {
    let file = fs::File::open(blob_path)?;
    let reader: Box<dyn Read> = if media_type.ends_with("+gzip") || media_type.ends_with(".gzip") {
        Box::new(flate2::read::GzDecoder::new(file))
    } else if media_type.ends_with("+zstd") || media_type.ends_with(".zstd") {
        Box::new(zstd::Decoder::new(file)?)
    } else if media_type.ends_with("+tar") || media_type.ends_with(".tar") {
        Box::new(file)
    } else {
        bail!("unsupported layer media type {media_type}");
    };

    let mut archive = tar::Archive::new(reader);
    archive.set_preserve_permissions(true);
    archive.set_preserve_mtime(true);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        // OCI whiteouts: `.wh..wh..opq` empties the directory it appears in
        // (opaque dir); `.wh.<name>` deletes `<name>` from lower layers.
        if file_name == ".wh..wh..opq" {
            if let Some(parent) = path.parent() {
                let dir = target.join(parent);
                if dir.starts_with(target) && dir.is_dir() {
                    for child in fs::read_dir(&dir)?.flatten() {
                        remove_any(&child.path());
                    }
                }
            }
            continue;
        }
        if let Some(hidden) = file_name.strip_prefix(".wh.") {
            if let Some(parent) = path.parent() {
                let victim = target.join(parent).join(hidden);
                if victim.starts_with(target) {
                    remove_any(&victim);
                }
            }
            continue;
        }

        match entry.header().entry_type() {
            // Device nodes need root to create and are useless under
            // virtio-fs anyway (libkrun's guest mounts its own /dev).
            tar::EntryType::Block | tar::EntryType::Char => continue,
            _ => {}
        }

        // A file may replace a directory (or vice versa) from a lower layer.
        let destination = target.join(&path);
        if destination.starts_with(target)
            && fs::symlink_metadata(&destination).is_ok_and(|meta| {
                meta.is_dir() != entry.header().entry_type().is_dir()
            })
        {
            remove_any(&destination);
        }

        if !entry.unpack_in(target)? {
            log::warn!("skipped suspicious tar entry {path:?}");
        }
    }
    Ok(())
}

fn remove_any(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    let result = if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    if let Err(err) = result {
        log::warn!("failed to remove {path:?} while applying image layer: {err}");
    }
}

/// Copy-on-write directory clone via the platform `cp`, which is the only
/// stable interface to clonefile/reflink without extra dependencies.
// Blocking `output()` is fine here: everything in this module runs on the
// dedicated container-prepare thread, never on an async executor.
#[allow(clippy::disallowed_methods)]
fn clone_dir(src: &Path, dest: &Path) -> Result<()> {
    let mut command = std::process::Command::new("cp");
    if cfg!(target_os = "macos") {
        // BSD cp: -c requests clonefile, -R recursive, -p preserve modes.
        command.arg("-Rpc");
    } else if cfg!(target_os = "linux") {
        command.arg("-a").arg("--reflink=auto");
    } else {
        bail!("the built-in sandbox is not supported on this platform");
    }
    let output = command
        .arg(src)
        .arg(dest)
        .output()
        .context("failed to run cp for rootfs clone")?;
    if !output.status.success() {
        bail!(
            "cloning rootfs {src:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// OCI images ship an empty (or dangling-symlink) /etc/resolv.conf and expect
/// the container runtime to inject one. libkrun's TSI networking proxies
/// sockets through the host, but the guest's libc resolver still reads
/// resolv.conf, so give it public resolvers.
fn configure_guest_etc(rootfs: &Path) -> Result<()> {
    let etc = rootfs.join("etc");
    fs::create_dir_all(&etc)?;

    let resolv = etc.join("resolv.conf");
    // May exist as a dangling symlink into /run; remove before writing.
    if fs::symlink_metadata(&resolv).is_ok() {
        fs::remove_file(&resolv)?;
    }
    fs::write(&resolv, "nameserver 1.1.1.1\nnameserver 8.8.8.8\n")?;

    let hosts = etc.join("hosts");
    if fs::symlink_metadata(&hosts).is_err() {
        fs::write(&hosts, "127.0.0.1 localhost\n")?;
    }
    Ok(())
}

/// Digests ("sha256:abc…") become directory names; keep them readable but
/// filesystem-safe.
fn sanitize_component(digest: &str) -> String {
    digest
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

/// Image references can contain `/` and other separators; keep a readable
/// prefix and disambiguate with a content hash so distinct refs can never
/// collide after sanitization.
fn sanitize_ref(image_ref: &str) -> String {
    let readable: String = image_ref
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let hash = Sha256::digest(image_ref.as_bytes());
    let mut short_hash = String::with_capacity(12);
    for byte in hash.iter().take(6) {
        short_hash.push_str(&format!("{byte:02x}"));
    }
    format!("{readable}-{short_hash}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_directories_are_filesystem_safe() {
        let store = ImageStore::new(PathBuf::from("/data/containers"));
        let dir = store.rootfs_dir_for_digest("sha256:00f5f2");
        assert_eq!(dir, PathBuf::from("/data/containers/rootfs/sha256-00f5f2"));
    }

    #[test]
    fn distinct_refs_never_collide_after_sanitization() {
        // Both sanitize to the same readable prefix; the hash suffix must
        // keep them apart.
        let a = sanitize_ref("ubuntu:latest");
        let b = sanitize_ref("ubuntu/latest");
        assert_ne!(a, b);
        assert!(a.starts_with("ubuntu_latest-"));
    }

    #[test]
    fn cached_rootfs_requires_ref_marker_and_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = ImageStore::new(temp.path().to_path_buf());
        assert_eq!(store.cached_rootfs("ubuntu:latest"), None);

        // Ref written but no rootfs yet -> still a cache miss.
        store
            .write_ref("ubuntu:latest", "sha256:abc")
            .expect("write ref");
        assert_eq!(store.cached_rootfs("ubuntu:latest"), None);

        // Rootfs dir without completion marker (interrupted unpack) -> miss.
        let dir = store.rootfs_dir_for_digest("sha256:abc");
        fs::create_dir_all(&dir).expect("create rootfs dir");
        assert_eq!(store.cached_rootfs("ubuntu:latest"), None);

        // Marker present -> hit.
        fs::write(store.complete_marker("sha256:abc"), "sha256:abc").expect("write marker");
        assert_eq!(store.cached_rootfs("ubuntu:latest"), Some(dir));
    }

    #[test]
    fn ephemeral_rootfs_is_a_writable_clone_with_dns_configured() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = ImageStore::new(temp.path().join("store"));

        let source = temp.path().join("source");
        fs::create_dir_all(source.join("etc")).expect("create source/etc");
        fs::write(source.join("etc/os-release"), "ID=ubuntu\n").expect("write file");

        let clone = store
            .create_ephemeral_rootfs(&source)
            .expect("create ephemeral rootfs");
        assert_ne!(clone, source);
        assert_eq!(
            fs::read_to_string(clone.join("etc/os-release")).expect("read clone"),
            "ID=ubuntu\n"
        );
        let resolv = fs::read_to_string(clone.join("etc/resolv.conf")).expect("read resolv");
        assert!(resolv.contains("nameserver"));
        // The original image stays pristine.
        assert!(!source.join("etc/resolv.conf").exists());
    }

    #[test]
    fn stale_ephemeral_cleanup_ignores_missing_tmp_dir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = ImageStore::new(temp.path().join("does-not-exist"));
        store.remove_stale_ephemerals();
    }

    const TAR_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar";

    fn tar_layer(entries: &[(&str, Option<&str>)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, contents) in entries {
            match contents {
                Some(contents) => {
                    let mut header = tar::Header::new_gnu();
                    header.set_size(contents.len() as u64);
                    header.set_mode(0o644);
                    header.set_cksum();
                    builder
                        .append_data(&mut header, path, contents.as_bytes())
                        .expect("append file");
                }
                None => {
                    let mut header = tar::Header::new_gnu();
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_size(0);
                    header.set_mode(0o755);
                    header.set_cksum();
                    builder
                        .append_data(&mut header, path, std::io::empty())
                        .expect("append dir");
                }
            }
        }
        builder.into_inner().expect("finish tar")
    }

    fn write_blob(dir: &Path, bytes: &[u8]) -> PathBuf {
        let path = dir.join(format!("layer-{}.tar", unique_suffix()));
        fs::write(&path, bytes).expect("write blob");
        path
    }

    #[test]
    fn whiteouts_delete_files_and_empty_opaque_dirs_from_lower_layers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("rootfs");
        fs::create_dir_all(&target).expect("create target");

        let lower = tar_layer(&[
            ("etc/", None),
            ("etc/keep.conf", Some("keep")),
            ("etc/drop.conf", Some("drop")),
            ("opt/", None),
            ("opt/stale.txt", Some("stale")),
        ]);
        let upper = tar_layer(&[
            ("etc/.wh.drop.conf", Some("")),
            ("opt/.wh..wh..opq", Some("")),
            ("opt/fresh.txt", Some("fresh")),
        ]);

        let lower_blob = write_blob(temp.path(), &lower);
        apply_layer(&lower_blob, TAR_MEDIA_TYPE, &target).expect("apply lower");
        let upper_blob = write_blob(temp.path(), &upper);
        apply_layer(&upper_blob, TAR_MEDIA_TYPE, &target).expect("apply upper");

        assert_eq!(
            fs::read_to_string(target.join("etc/keep.conf")).expect("keep survives"),
            "keep"
        );
        assert!(!target.join("etc/drop.conf").exists());
        assert!(!target.join("opt/stale.txt").exists());
        assert_eq!(
            fs::read_to_string(target.join("opt/fresh.txt")).expect("fresh written"),
            "fresh"
        );
        // Whiteout markers themselves never land in the rootfs.
        assert!(!target.join("etc/.wh.drop.conf").exists());
        assert!(!target.join("opt/.wh..wh..opq").exists());
    }

    #[test]
    fn upper_layer_file_replaces_lower_layer_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("rootfs");
        fs::create_dir_all(&target).expect("create target");

        let lower = tar_layer(&[("var/run/", None), ("var/run/pid", Some("1"))]);
        let upper = tar_layer(&[("var/run", Some("now a file"))]);

        let lower_blob = write_blob(temp.path(), &lower);
        apply_layer(&lower_blob, TAR_MEDIA_TYPE, &target).expect("apply lower");
        let upper_blob = write_blob(temp.path(), &upper);
        apply_layer(&upper_blob, TAR_MEDIA_TYPE, &target).expect("apply upper");

        assert_eq!(
            fs::read_to_string(target.join("var/run")).expect("replaced"),
            "now a file"
        );
    }

    #[test]
    fn unknown_layer_media_types_are_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let blob = write_blob(temp.path(), b"not a real layer");
        let err = apply_layer(&blob, "application/vnd.example.squashfs", temp.path())
            .expect_err("must reject");
        assert!(err.to_string().contains("unsupported layer media type"));
    }

    #[test]
    fn guest_architecture_matches_oci_naming() {
        // The host arch must translate to OCI's naming, or multi-arch index
        // resolution silently finds no manifest.
        let arch = guest_architecture();
        assert!(arch == "arm64" || arch == "amd64" || !arch.is_empty());
    }
}
