use anyhow::{Context as _, Result};
use futures::{AsyncReadExt as _, AsyncWriteExt as _};
use http_client::HttpClient;
use sha2::{Digest as _, Sha256};
use std::path::Path;

/// Progress of an in-flight download. `total` is `None` when the server does not
/// report a content length and the caller did not supply an expected size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadProgress {
    pub received: u64,
    pub total: Option<u64>,
}

/// Streams `url` into `destination`, reporting bytes-received via `on_progress`
/// and verifying the SHA-256 against `expected_sha256` when supplied.
///
/// The bytes land in a sibling `*.partial` file that is only renamed onto
/// `destination` once the full body has been written and (if requested) its
/// digest matches, so a canceled or corrupt transfer never leaves a file that
/// looks complete. Dropping the returned future cancels the download.
pub async fn download_file_with_progress(
    http_client: &dyn HttpClient,
    url: &str,
    destination: &Path,
    expected_sha256: Option<&str>,
    expected_total: Option<u64>,
    mut on_progress: impl FnMut(DownloadProgress),
) -> Result<()> {
    let parent = destination
        .parent()
        .with_context(|| format!("destination {destination:?} has no parent directory"))?;
    smol::fs::create_dir_all(parent)
        .await
        .with_context(|| format!("creating download directory {parent:?}"))?;

    let mut response = http_client
        .get(url, Default::default(), true)
        .await
        .with_context(|| format!("requesting {url}"))?;
    anyhow::ensure!(
        response.status().is_success(),
        "{url} returned HTTP status {}",
        response.status()
    );

    // Prefer the caller-supplied size (from the pinned catalog) and fall back to
    // the response's Content-Length so the UI can still show a determinate bar.
    let total = expected_total.or_else(|| {
        response
            .headers()
            .get(http_client::http::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
    });

    let partial_path = partial_path(destination);
    let result = stream_to_file(
        response.body_mut(),
        &partial_path,
        expected_sha256,
        total,
        &mut on_progress,
    )
    .await;

    match result {
        Ok(()) => {
            smol::fs::rename(&partial_path, destination)
                .await
                .with_context(|| format!("moving {partial_path:?} to {destination:?}"))?;
            Ok(())
        }
        Err(error) => {
            if let Err(cleanup_error) = smol::fs::remove_file(&partial_path).await {
                log::warn!("failed to remove partial download {partial_path:?}: {cleanup_error:?}");
            }
            Err(error)
        }
    }
}

/// The staging path a download is written to before it is verified and renamed.
fn partial_path(destination: &Path) -> std::path::PathBuf {
    let mut name = destination
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(".partial");
    destination.with_file_name(name)
}

async fn stream_to_file(
    body: &mut http_client::AsyncBody,
    partial_path: &Path,
    expected_sha256: Option<&str>,
    total: Option<u64>,
    on_progress: &mut impl FnMut(DownloadProgress),
) -> Result<()> {
    let mut file = smol::fs::File::create(partial_path)
        .await
        .with_context(|| format!("creating {partial_path:?}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 128 * 1024];
    let mut received: u64 = 0;

    on_progress(DownloadProgress { received, total });
    loop {
        let read = body
            .read(&mut buffer)
            .await
            .context("reading response body")?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        if expected_sha256.is_some() {
            hasher.update(chunk);
        }
        file.write_all(chunk)
            .await
            .with_context(|| format!("writing {partial_path:?}"))?;
        received += read as u64;
        on_progress(DownloadProgress { received, total });
    }
    // Close the handle before the caller renames it: Windows refuses to rename a
    // file with an open handle, and the flush guarantees the bytes are durable.
    file.flush().await.context("flushing download")?;
    file.close().await.context("closing download")?;
    drop(file);

    if let Some(expected) = expected_sha256 {
        let actual = format!("{:x}", hasher.finalize());
        anyhow::ensure!(
            actual == expected.to_ascii_lowercase(),
            "SHA-256 mismatch: expected {expected}, got {actual}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::BoxFuture;
    use http_client::{AsyncBody, Response, http::HeaderValue};
    use url::Url;

    struct StaticClient {
        body: Vec<u8>,
    }

    impl HttpClient for StaticClient {
        fn send(
            &self,
            _request: http_client::http::Request<AsyncBody>,
        ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
            let body = self.body.clone();
            Box::pin(async move {
                Ok(Response::builder()
                    .status(200)
                    .body(AsyncBody::from(body))
                    .unwrap())
            })
        }

        fn user_agent(&self) -> Option<&HeaderValue> {
            None
        }

        fn proxy(&self) -> Option<&Url> {
            None
        }
    }

    #[test]
    fn reports_monotonic_progress_and_writes_file() {
        futures::executor::block_on(async {
            let temp_dir = tempfile::tempdir().unwrap();
            let destination = temp_dir.path().join("model.gguf");
            // Larger than the 128 KiB read buffer, so progress ticks more than once.
            let contents = vec![7u8; 300 * 1024];
            let expected_sha = format!("{:x}", Sha256::digest(&contents));
            let client = StaticClient {
                body: contents.clone(),
            };

            let mut updates = Vec::new();
            download_file_with_progress(
                &client,
                "https://example.com/model.gguf",
                &destination,
                Some(&expected_sha),
                Some(contents.len() as u64),
                |progress| updates.push(progress),
            )
            .await
            .unwrap();

            assert_eq!(std::fs::read(&destination).unwrap(), contents);
            assert!(
                updates.len() >= 3,
                "expected several progress ticks, got {}",
                updates.len()
            );
            assert_eq!(updates.first().unwrap().received, 0);
            assert_eq!(
                updates.last().unwrap().received,
                contents.len() as u64,
                "final tick should report the full size"
            );
            // Received bytes never decrease and never exceed the total.
            let mut previous = 0;
            for update in &updates {
                assert!(update.received >= previous);
                assert_eq!(update.total, Some(contents.len() as u64));
                assert!(update.received <= contents.len() as u64);
                previous = update.received;
            }
            // The staging file must not survive a successful download.
            assert!(!partial_path(&destination).exists());
        });
    }

    #[test]
    fn digest_mismatch_fails_and_leaves_no_file() {
        futures::executor::block_on(async {
            let temp_dir = tempfile::tempdir().unwrap();
            let destination = temp_dir.path().join("model.gguf");
            let client = StaticClient {
                body: b"the real bytes".to_vec(),
            };

            let error = download_file_with_progress(
                &client,
                "https://example.com/model.gguf",
                &destination,
                Some("0000000000000000000000000000000000000000000000000000000000000000"),
                Some(14),
                |_| {},
            )
            .await
            .unwrap_err();

            assert!(error.to_string().contains("SHA-256 mismatch"));
            assert!(!destination.exists(), "destination must not be created");
            assert!(
                !partial_path(&destination).exists(),
                "partial file must be cleaned up"
            );
        });
    }
}
