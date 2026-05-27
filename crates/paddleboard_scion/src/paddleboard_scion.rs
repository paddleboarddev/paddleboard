pub mod compat;
pub mod types;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};

pub use compat::Compatibility;
pub use types::*;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct ScionCli {
    scion_path: Option<PathBuf>,
    project_dir: Option<PathBuf>,
    timeout: Duration,
}

impl ScionCli {
    pub fn new() -> Self {
        Self {
            scion_path: None,
            project_dir: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_scion_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.scion_path = Some(path.into());
        self
    }

    pub fn with_project_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.project_dir = Some(dir.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn resolve_binary(&self) -> Result<PathBuf> {
        if let Some(ref path) = self.scion_path {
            return Ok(path.clone());
        }
        which::which("scion").context(
            "scion binary not found on PATH. Install it with: \
             go install github.com/GoogleCloudPlatform/scion/cmd/scion@latest",
        )
    }

    pub fn is_available(&self) -> bool {
        self.resolve_binary().is_ok()
    }

    async fn run_command(&self, args: &[&str]) -> Result<String> {
        let binary = self.resolve_binary()?;
        let mut cmd = tokio::process::Command::new(&binary);
        cmd.args(args);
        cmd.args(["--format", "json", "--non-interactive"]);

        if let Some(ref dir) = self.project_dir {
            cmd.current_dir(dir);
        }

        cmd.stdin(std::process::Stdio::null());

        let output = tokio::time::timeout(self.timeout, cmd.output())
            .await
            .context("scion command timed out")?
            .with_context(|| format!("failed to execute scion {}", args.first().unwrap_or(&"")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "scion {} exited with {}: {}",
                args.first().unwrap_or(&""),
                output.status,
                stderr.trim()
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    async fn run_raw_command(&self, args: &[&str]) -> Result<String> {
        let binary = self.resolve_binary()?;
        let mut cmd = tokio::process::Command::new(&binary);
        cmd.args(args);
        cmd.arg("--non-interactive");

        if let Some(ref dir) = self.project_dir {
            cmd.current_dir(dir);
        }

        cmd.stdin(std::process::Stdio::null());

        let output = tokio::time::timeout(self.timeout, cmd.output())
            .await
            .context("scion command timed out")?
            .with_context(|| format!("failed to execute scion {}", args.first().unwrap_or(&"")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "scion {} exited with {}: {}",
                args.first().unwrap_or(&""),
                output.status,
                stderr.trim()
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if stdout.is_empty() && !stderr.is_empty() {
            return Ok(stderr);
        }
        Ok(stdout)
    }

    // --- Prereq checking ---

    pub async fn version(&self) -> Result<ScionVersion> {
        let output = self.run_command(&["version"]).await?;
        serde_json::from_str(&output).context("failed to parse scion version output")
    }

    pub async fn check_compatibility(&self) -> Result<Compatibility> {
        let version = self.version().await?;
        let result = compat::check_compatibility(&version.version);
        match &result {
            Compatibility::NewerThanTested {
                installed, tested, ..
            } => {
                log::warn!(
                    "scion {installed} is newer than tested version {tested}; \
                     some JSON fields may be unrecognized"
                );
            }
            Compatibility::OlderThanTested {
                installed, tested, ..
            } => {
                log::warn!(
                    "scion {installed} is older than tested version {tested}; \
                     some expected fields may be missing"
                );
            }
            _ => {}
        }
        Ok(result)
    }

    // --- Agent lifecycle ---

    pub async fn list_agents(
        &self,
        all: bool,
        running_only: bool,
    ) -> Result<Vec<AgentInfo>> {
        let mut args = vec!["list"];
        if all {
            args.push("--all");
        }
        if running_only {
            args.push("--running");
        }
        let output = self.run_command(&args).await?;
        serde_json::from_str(&output).context("failed to parse scion list output")
    }

    pub async fn start_agent(
        &self,
        name: &str,
        task: Option<&str>,
        options: &StartAgentOptions,
    ) -> Result<String> {
        let mut args = vec!["start".to_string(), name.to_string()];
        if let Some(task_text) = task {
            args.push(task_text.to_string());
        }
        args.extend(options.to_args());

        let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.run_raw_command(&str_args).await
    }

    pub async fn stop_agent(&self, name: Option<&str>, all: bool) -> Result<()> {
        let mut args = vec!["stop"];
        if let Some(agent_name) = name {
            args.push(agent_name);
        }
        if all {
            args.push("--all");
        }
        self.run_raw_command(&args).await?;
        Ok(())
    }

    pub async fn delete_agent(&self, name: &str) -> Result<()> {
        self.run_raw_command(&["delete", name, "--yes"]).await?;
        Ok(())
    }

    pub async fn resume_agent(
        &self,
        name: &str,
        task: Option<&str>,
    ) -> Result<String> {
        let mut args = vec!["resume", name];
        if let Some(task_text) = task {
            args.push(task_text);
        }
        self.run_raw_command(&args).await
    }

    // --- Agent communication ---

    pub async fn send_message(&self, name: &str, message: &str) -> Result<()> {
        self.run_raw_command(&["message", name, message]).await?;
        Ok(())
    }

    pub async fn look(
        &self,
        name: &str,
        num_lines: Option<usize>,
    ) -> Result<String> {
        let mut args = vec!["look", name];
        let lines_str;
        if let Some(count) = num_lines {
            lines_str = count.to_string();
            args.extend_from_slice(&["-n", &lines_str]);
        }
        self.run_raw_command(&args).await
    }

    pub async fn agent_logs(
        &self,
        name: &str,
        tail: Option<usize>,
    ) -> Result<String> {
        let mut args = vec!["logs", name];
        let tail_str;
        if let Some(count) = tail {
            tail_str = count.to_string();
            args.extend_from_slice(&["--tail", &tail_str]);
        }
        self.run_raw_command(&args).await
    }

    // --- Project and templates ---

    pub async fn project_init(&self, global: bool) -> Result<()> {
        let mut args = vec!["init"];
        if global {
            args.push("--global");
        }
        self.run_raw_command(&args).await?;
        Ok(())
    }

    pub async fn list_templates(&self) -> Result<Vec<TemplateInfo>> {
        let output = self.run_command(&["templates", "list"]).await?;
        serde_json::from_str(&output).context("failed to parse scion templates list output")
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn default_timeout_is_30s() {
        let cli = ScionCli::new();
        assert_eq!(cli.timeout, Duration::from_secs(30));
    }

    #[test]
    fn builder_methods_chain() {
        let cli = ScionCli::new()
            .with_project_dir("/tmp/project")
            .with_timeout(Duration::from_secs(10));
        assert_eq!(cli.timeout, Duration::from_secs(10));
        assert_eq!(
            cli.project_dir.as_deref(),
            Some(Path::new("/tmp/project"))
        );
    }

    #[test]
    fn builder_with_scion_path() {
        let cli = ScionCli::new().with_scion_path("/usr/local/bin/scion");
        assert_eq!(
            cli.scion_path.as_deref(),
            Some(Path::new("/usr/local/bin/scion"))
        );
    }

    #[test]
    fn resolve_binary_uses_override() {
        let cli = ScionCli::new().with_scion_path("/tmp/fake-scion");
        let result = cli.resolve_binary();
        assert_eq!(result.ok(), Some(PathBuf::from("/tmp/fake-scion")));
    }
}
