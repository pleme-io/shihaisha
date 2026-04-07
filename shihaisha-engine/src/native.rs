use async_trait::async_trait;
use shihaisha_core::traits::config_translator::ConfigEmitter;
use shihaisha_core::traits::init_backend::InitBackend;
#[allow(unused_imports)]
use shihaisha_core::{
    BackendOverrides, DependencySpec, Error, HealthState, LogTarget, LoggingSpec,
    RestartStrategy, Result, ServiceSpec, ServiceState, ServiceStatus,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Pure Rust process supervisor backend.
///
/// Stores service specs as YAML files under `~/.config/shihaisha/services/`
/// and manages processes directly with `tokio::process::Command`.
/// Available on all platforms as a fallback when systemd/launchd are absent.
pub struct NativeBackend {
    services_dir: PathBuf,
    state: Arc<RwLock<HashMap<String, NativeServiceState>>>,
}

/// Runtime state for a natively managed service.
#[derive(Debug, Clone)]
struct NativeServiceState {
    spec: ServiceSpec,
    pid: Option<u32>,
    service_state: ServiceState,
    exit_code: Option<i32>,
    restart_count: u32,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl NativeBackend {
    /// Create a new native backend with default config directory.
    #[must_use]
    pub fn new() -> Self {
        let services_dir = config_dir().join("services");
        Self {
            services_dir,
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a native backend with a custom services directory (for testing).
    #[must_use]
    pub fn with_dir(services_dir: PathBuf) -> Self {
        Self {
            services_dir,
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn spec_path(&self, name: &str) -> PathBuf {
        self.services_dir.join(format!("{name}.yaml"))
    }

    /// Load a service spec from disk.
    async fn load_spec(&self, name: &str) -> Result<ServiceSpec> {
        let path = self.spec_path(name);
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|_| Error::ServiceNotFound(name.to_owned()))?;
        serde_yaml_ng::from_str(&content)
            .map_err(|e| Error::ConfigError(format!("failed to parse spec for {name}: {e}")))
    }

    /// Spawn a process for a service and track it.
    fn spawn_process(&self, spec: &ServiceSpec) -> Result<Option<u32>> {
        let mut cmd = tokio::process::Command::new(&spec.command);
        cmd.args(&spec.args);

        if let Some(ref wd) = spec.working_directory {
            cmd.current_dir(wd);
        }

        for (k, v) in &spec.environment {
            cmd.env(k, v);
        }

        // Configure stdout/stderr
        match &spec.logging.stdout {
            LogTarget::Null => {
                cmd.stdout(std::process::Stdio::null());
            }
            LogTarget::File(path) => {
                cmd.stdout(open_log_file(path, "stdout")?);
            }
            LogTarget::Journal | LogTarget::Inherit => {
                cmd.stdout(std::process::Stdio::inherit());
            }
        }

        match &spec.logging.stderr {
            LogTarget::Null => {
                cmd.stderr(std::process::Stdio::null());
            }
            LogTarget::File(path) => {
                cmd.stderr(open_log_file(path, "stderr")?);
            }
            LogTarget::Journal | LogTarget::Inherit => {
                cmd.stderr(std::process::Stdio::inherit());
            }
        }

        let child = cmd.spawn().map_err(|e| Error::BackendError {
            backend: "native".to_owned(),
            operation: "start".to_owned(),
            detail: format!("failed to spawn process: {e}"),
        })?;

        let pid = child.id();

        spawn_process_watcher(
            Arc::clone(&self.state),
            child,
            spec.clone(),
        );

        Ok(pid)
    }
}

fn spawn_process_watcher(
    state: Arc<tokio::sync::RwLock<HashMap<String, NativeServiceState>>>,
    initial_child: tokio::process::Child,
    spec: ServiceSpec,
) {
    let name = spec.name.clone();
    let restart_strategy = spec.restart.strategy;
    let restart_delay = spec.restart.delay_secs;
    let max_retries = spec.restart.max_retries;

    tokio::spawn(async move {
        let mut child = initial_child;
        loop {
            let exit_code = child
                .wait()
                .await
                .ok()
                .and_then(|s| s.code())
                .unwrap_or(-1);

            let mut state_lock = state.write().await;
            let Some(svc_state) = state_lock.get_mut(&name) else {
                break;
            };

            svc_state.pid = None;
            svc_state.exit_code = Some(exit_code);

            let should_restart = match restart_strategy {
                RestartStrategy::Always => true,
                RestartStrategy::OnFailure => exit_code != 0,
                RestartStrategy::OnSuccess => exit_code == 0,
                RestartStrategy::Never => false,
            };

            let within_retries = max_retries == 0 || svc_state.restart_count < max_retries;

            if should_restart && within_retries {
                svc_state.service_state = ServiceState::Starting;
                svc_state.restart_count += 1;
                drop(state_lock);

                tokio::time::sleep(std::time::Duration::from_secs(restart_delay)).await;

                let mut cmd = tokio::process::Command::new(&spec.command);
                cmd.args(&spec.args);
                if let Some(ref wd) = spec.working_directory {
                    cmd.current_dir(wd);
                }
                for (k, v) in &spec.environment {
                    cmd.env(k, v);
                }
                cmd.stdout(std::process::Stdio::null());
                cmd.stderr(std::process::Stdio::null());

                if let Ok(new_child) = cmd.spawn() {
                    let new_pid = new_child.id();
                    let mut state_lock = state.write().await;
                    if let Some(svc_state) = state_lock.get_mut(&name) {
                        svc_state.pid = new_pid;
                        svc_state.service_state = ServiceState::Running;
                        svc_state.started_at = Some(chrono::Utc::now());
                    }
                    child = new_child;
                } else {
                    let mut state_lock = state.write().await;
                    if let Some(svc_state) = state_lock.get_mut(&name) {
                        svc_state.service_state = ServiceState::Failed;
                    }
                    break;
                }
            } else {
                svc_state.service_state = if exit_code == 0 {
                    ServiceState::Stopped
                } else {
                    ServiceState::Failed
                };
                break;
            }
        }
    });
}

impl Default for NativeBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigEmitter for NativeBackend {
    fn emit(&self, spec: &ServiceSpec) -> Result<String> {
        serde_yaml_ng::to_string(spec)
            .map_err(|e| Error::ConfigError(format!("failed to serialize spec: {e}")))
    }

    fn extension(&self) -> &'static str {
        "yaml"
    }

    fn name(&self) -> &'static str {
        "native"
    }
}

#[async_trait]
impl InitBackend for NativeBackend {
    async fn install(&self, spec: &ServiceSpec) -> Result<()> {
        // Write spec to YAML file
        tokio::fs::create_dir_all(&self.services_dir)
            .await
            .map_err(|e| Error::BackendError {
                backend: "native".to_owned(),
                operation: "install".to_owned(),
                detail: format!("failed to create services dir: {e}"),
            })?;

        let yaml = serde_yaml_ng::to_string(spec)
            .map_err(|e| Error::Serialization(format!("failed to serialize spec: {e}")))?;

        let path = self.spec_path(&spec.name);
        tokio::fs::write(&path, yaml)
            .await
            .map_err(|e| Error::BackendError {
                backend: "native".to_owned(),
                operation: "install".to_owned(),
                detail: format!("failed to write spec: {e}"),
            })?;

        // Initialize state entry
        let mut state = self.state.write().await;
        state.insert(
            spec.name.clone(),
            NativeServiceState {
                spec: spec.clone(),
                pid: None,
                service_state: ServiceState::Inactive,
                exit_code: None,
                restart_count: 0,
                started_at: None,
            },
        );

        Ok(())
    }

    async fn uninstall(&self, name: &str) -> Result<()> {
        // Stop if running, continuing on errors
        if let Err(e) = self.stop(name).await {
            tracing::warn!(service = name, error = %e, "failed to stop during uninstall, continuing");
        }

        // Remove spec file
        let path = self.spec_path(name);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| Error::BackendError {
                    backend: "native".to_owned(),
                    operation: "uninstall".to_owned(),
                    detail: format!("failed to remove spec: {e}"),
                })?;
        }

        // Remove from state
        let mut state = self.state.write().await;
        state.remove(name);

        Ok(())
    }

    async fn start(&self, name: &str) -> Result<()> {
        // Load spec from disk or from cached state
        let spec = {
            let state = self.state.read().await;
            if let Some(svc) = state.get(name) {
                svc.spec.clone()
            } else {
                drop(state);
                self.load_spec(name).await?
            }
        };

        // Check if already running
        {
            let state = self.state.read().await;
            if state.get(name).is_some_and(|svc| {
                svc.service_state == ServiceState::Running && svc.pid.is_some()
            }) {
                return Ok(());
            }
        }

        let pid = self.spawn_process(&spec)?;

        let mut state = self.state.write().await;
        let entry = state.entry(name.to_owned()).or_insert_with(|| NativeServiceState {
            spec: spec.clone(),
            pid: None,
            service_state: ServiceState::Inactive,
            exit_code: None,
            restart_count: 0,
            started_at: None,
        });
        entry.pid = pid;
        entry.service_state = ServiceState::Running;
        entry.started_at = Some(chrono::Utc::now());
        entry.spec = spec;

        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        let pid = {
            let state = self.state.read().await;
            state.get(name).and_then(|s| s.pid)
        };

        if let Some(pid) = pid.filter(|&p| p > 0) {
            // Send SIGTERM via kill command (guard against PID 0 which
            // would signal the entire process group).
            let output = tokio::process::Command::new("kill")
                .arg(pid.to_string())
                .output()
                .await
                .map_err(|e| Error::BackendError {
                    backend: "native".to_owned(),
                    operation: "stop".to_owned(),
                    detail: format!("kill failed: {e}"),
                })?;

            if !output.status.success() {
                // Process may have already exited; that's fine.
                tracing::debug!("kill {pid} returned non-zero, process may have already exited");
            }

            let mut state = self.state.write().await;
            if let Some(svc) = state.get_mut(name) {
                svc.pid = None;
                svc.service_state = ServiceState::Stopped;
            }
        } else {
            // Check if service exists at all
            let state = self.state.read().await;
            if !state.contains_key(name) && !self.spec_path(name).exists() {
                return Err(Error::ServiceNotFound(name.to_owned()));
            }
        }

        Ok(())
    }

    async fn restart(&self, name: &str) -> Result<()> {
        self.stop(name).await?;
        self.start(name).await
    }

    async fn reload(&self, name: &str) -> Result<()> {
        // Send SIGHUP for config reload
        let pid = {
            let state = self.state.read().await;
            state.get(name).and_then(|s| s.pid)
        };

        if let Some(pid) = pid.filter(|&p| p > 0) {
            let output = tokio::process::Command::new("kill")
                .args(["-HUP", &pid.to_string()])
                .output()
                .await
                .map_err(|e| Error::BackendError {
                    backend: "native".to_owned(),
                    operation: "reload".to_owned(),
                    detail: format!("kill -HUP failed: {e}"),
                })?;
            if !output.status.success() {
                return Err(Error::BackendError {
                    backend: "native".to_owned(),
                    operation: "reload".to_owned(),
                    detail: format!("failed to send SIGHUP to {name}"),
                });
            }
            Ok(())
        } else {
            Err(Error::BackendError {
                backend: "native".to_owned(),
                operation: "reload".to_owned(),
                detail: format!("service {name} is not running, cannot reload"),
            })
        }
    }

    async fn status(&self, name: &str) -> Result<ServiceStatus> {
        let state = self.state.read().await;
        if let Some(svc) = state.get(name) {
            let uptime_secs = svc.started_at.map(|started| {
                let duration = chrono::Utc::now() - started;
                u64::try_from(duration.num_seconds().max(0)).unwrap_or(0)
            });

            Ok(ServiceStatus {
                name: name.to_owned(),
                state: svc.service_state,
                pid: svc.pid,
                exit_code: svc.exit_code,
                started_at: svc.started_at,
                uptime_secs,
                restart_count: svc.restart_count,
                health: HealthState::Unknown,
                backend: "native".to_owned(),
                memory_bytes: None,
                cpu_usage_percent: None,
            })
        } else if self.spec_path(name).exists() {
            // Installed but not in in-memory state (e.g., after daemon restart)
            Ok(ServiceStatus {
                name: name.to_owned(),
                state: ServiceState::Inactive,
                pid: None,
                exit_code: None,
                started_at: None,
                uptime_secs: None,
                restart_count: 0,
                health: HealthState::Unknown,
                backend: "native".to_owned(),
                memory_bytes: None,
                cpu_usage_percent: None,
            })
        } else {
            Err(Error::ServiceNotFound(name.to_owned()))
        }
    }

    async fn logs(&self, name: &str, lines: u32) -> Result<Vec<String>> {
        // Try to find the log file from the spec
        let spec = self.load_spec(name).await.ok();

        if let Some(LogTarget::File(path)) = spec.as_ref().map(|s| &s.logging.stdout)
            && path.exists()
        {
            let text = tokio::fs::read_to_string(path)
                .await
                .map_err(|e| Error::BackendError {
                    backend: "native".to_owned(),
                    operation: "logs".to_owned(),
                    detail: format!("failed to read log: {e}"),
                })?;
            let all_lines: Vec<String> = text.lines().map(String::from).collect();
            let start = all_lines.len().saturating_sub(lines as usize);
            return Ok(all_lines[start..].to_vec());
        }

        Ok(vec![format!(
            "No log file configured for {name} — set logging.stdout to a file path"
        )])
    }

    async fn enable(&self, name: &str) -> Result<()> {
        // For native backend, "enable" means the spec file exists.
        // Check that it does.
        if !self.spec_path(name).exists() {
            return Err(Error::ServiceNotFound(name.to_owned()));
        }
        Ok(())
    }

    async fn disable(&self, name: &str) -> Result<()> {
        // For native backend, "disable" is a no-op since we don't have
        // boot-time integration. The service just won't auto-start.
        if !self.spec_path(name).exists() {
            return Err(Error::ServiceNotFound(name.to_owned()));
        }
        Ok(())
    }

    async fn list(&self) -> Result<Vec<ServiceStatus>> {
        let mut services = Vec::new();

        // Read from in-memory state
        let state = self.state.read().await;
        for (name, svc) in state.iter() {
            let uptime_secs = svc.started_at.map(|started| {
                let duration = chrono::Utc::now() - started;
                u64::try_from(duration.num_seconds().max(0)).unwrap_or(0)
            });

            services.push(ServiceStatus {
                name: name.clone(),
                state: svc.service_state,
                pid: svc.pid,
                exit_code: svc.exit_code,
                started_at: svc.started_at,
                uptime_secs,
                restart_count: svc.restart_count,
                health: HealthState::Unknown,
                backend: "native".to_owned(),
                memory_bytes: None,
                cpu_usage_percent: None,
            });
        }
        drop(state);

        // Also scan the services directory for installed but not-in-memory services
        if self.services_dir.exists() {
            let mut entries = tokio::fs::read_dir(&self.services_dir)
                .await
                .map_err(|e| Error::BackendError {
                    backend: "native".to_owned(),
                    operation: "list".to_owned(),
                    detail: format!("failed to read services dir: {e}"),
                })?;

            while let Some(entry) = entries.next_entry().await.map_err(|e| {
                Error::BackendError {
                    backend: "native".to_owned(),
                    operation: "list".to_owned(),
                    detail: format!("failed to read dir entry: {e}"),
                }
            })?
            {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "yaml")
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                    && !services.iter().any(|s| s.name == stem)
                {
                    services.push(ServiceStatus::new(stem, ServiceState::Inactive, "native"));
                }
            }
        }

        Ok(services)
    }

    async fn daemon_reload(&self) -> Result<()> {
        // No-op for native backend. State is in-memory.
        Ok(())
    }

    fn available(&self) -> bool {
        // Native backend is always available.
        true
    }

    fn name(&self) -> &'static str {
        "native"
    }
}

fn open_log_file(path: &std::path::Path, stream: &str) -> Result<std::fs::File> {
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!(path = %parent.display(), error = %e, "failed to create {stream} log dir, continuing");
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| Error::BackendError {
            backend: "native".to_owned(),
            operation: "start".to_owned(),
            detail: format!("failed to open {stream} log: {e}"),
        })
}

/// Get the shihaisha config directory.
fn config_dir() -> PathBuf {
    crate::util::home_dir().join(".config/shihaisha")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_spec(name: &str) -> ServiceSpec {
        let mut spec = ServiceSpec::new(name, "/bin/echo");
        spec.args = vec!["hello".to_owned()];
        spec.restart.strategy = RestartStrategy::Never;
        spec.restart.delay_secs = 1;
        spec
    }

    #[tokio::test]
    async fn install_and_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());

        let spec = test_spec("test-native");
        backend.install(&spec).await.expect("install");

        // Check that the YAML file was written
        let spec_path = dir.path().join("test-native.yaml");
        assert!(spec_path.exists(), "spec file should exist");

        // Check that the service appears in the list
        let services = backend.list().await.expect("list");
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "test-native");
        assert_eq!(services[0].state, ServiceState::Inactive);
        assert_eq!(services[0].backend, "native");
    }

    #[tokio::test]
    async fn service_lifecycle() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());

        let mut spec = test_spec("lifecycle-test");
        // Use a command that exits immediately
        spec.command = "/bin/echo".to_owned();
        spec.args = vec!["hello".to_owned()];

        // Install
        backend.install(&spec).await.expect("install");

        // Status before start
        let status = backend.status("lifecycle-test").await.expect("status");
        assert_eq!(status.state, ServiceState::Inactive);

        // Start
        backend.start("lifecycle-test").await.expect("start");

        // Brief pause to let process spawn
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // The echo command exits immediately, so the status should reflect that
        let status = backend.status("lifecycle-test").await.expect("status after start");
        // echo exits quickly — state will be Stopped or still Running depending on timing
        assert!(
            matches!(
                status.state,
                ServiceState::Running | ServiceState::Stopped
            ),
            "state should be Running or Stopped, got {:?}",
            status.state
        );
    }

    #[tokio::test]
    async fn service_not_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());

        let result = backend.status("nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            Error::ServiceNotFound(_)
        ));
    }

    #[tokio::test]
    async fn install_uninstall() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());

        let spec = test_spec("removable");
        backend.install(&spec).await.expect("install");
        assert!(dir.path().join("removable.yaml").exists());

        backend.uninstall("removable").await.expect("uninstall");
        assert!(!dir.path().join("removable.yaml").exists());

        let services = backend.list().await.expect("list");
        assert!(services.is_empty());
    }

    #[tokio::test]
    async fn available_always() {
        let backend = NativeBackend::new();
        assert!(backend.available(), "native backend should always be available");
    }

    #[tokio::test]
    async fn name_is_native() {
        let backend = NativeBackend::new();
        assert_eq!(InitBackend::name(&backend), "native");
        assert_eq!(ConfigEmitter::name(&backend), "native");
    }

    #[tokio::test]
    async fn enable_disable_require_installed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());

        // Not installed => error
        let result = backend.enable("not-installed").await;
        assert!(result.is_err());

        let result = backend.disable("not-installed").await;
        assert!(result.is_err());

        // Install and try again
        let spec = test_spec("enableable");
        backend.install(&spec).await.expect("install");
        backend.enable("enableable").await.expect("enable");
        backend.disable("enableable").await.expect("disable");
    }

    #[tokio::test]
    async fn spec_roundtrip_through_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());

        let mut spec = test_spec("roundtrip");
        spec.environment
            .insert("FOO".to_owned(), "bar".to_owned());
        spec.working_directory = Some(PathBuf::from("/tmp"));

        backend.install(&spec).await.expect("install");

        let loaded = backend.load_spec("roundtrip").await.expect("load");
        assert_eq!(loaded.name, "roundtrip");
        assert_eq!(loaded.command, "/bin/echo");
        assert_eq!(loaded.environment.get("FOO").unwrap(), "bar");
        assert_eq!(
            loaded.working_directory,
            Some(PathBuf::from("/tmp"))
        );
    }

    #[tokio::test]
    async fn daemon_reload_is_noop() {
        let backend = NativeBackend::new();
        backend.daemon_reload().await.expect("daemon_reload should succeed");
    }

    #[tokio::test]
    async fn list_includes_disk_only_services() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());

        // Write a YAML file directly to disk (simulating a previous session)
        let spec = test_spec("disk-only");
        let yaml = serde_yaml_ng::to_string(&spec).expect("serialize");
        tokio::fs::create_dir_all(dir.path())
            .await
            .expect("mkdir");
        tokio::fs::write(dir.path().join("disk-only.yaml"), yaml)
            .await
            .expect("write");

        let services = backend.list().await.expect("list");
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "disk-only");
        assert_eq!(services[0].state, ServiceState::Inactive);
    }

    // Phase 5a: Restart behavior test
    #[tokio::test]
    async fn restart_on_failure_exhausts_retries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());

        let mut spec = ServiceSpec::new("fail-restart", "false");
        spec.restart.strategy = RestartStrategy::OnFailure;
        spec.restart.max_retries = 2;
        spec.restart.delay_secs = 1;

        backend.install(&spec).await.expect("install");
        backend.start("fail-restart").await.expect("start");

        // Wait enough for the process to exit + 2 restart cycles (1s delay each) + margin
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            async {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let status = backend.status("fail-restart").await.expect("status");
                    if status.state == ServiceState::Failed {
                        return status;
                    }
                }
            },
        )
        .await;

        let status = result.expect("should reach Failed state within timeout");
        assert_eq!(status.state, ServiceState::Failed);
        // restart_count should be > 0 (attempted restarts before giving up)
        assert!(
            status.restart_count > 0,
            "expected restart_count > 0, got {}",
            status.restart_count
        );

        // Clean up
        backend.uninstall("fail-restart").await.expect("uninstall");
    }

    #[test]
    fn config_emitter_emit_produces_valid_yaml() {
        let backend = NativeBackend::new();
        let spec = test_spec("emit-test");
        let yaml = ConfigEmitter::emit(&backend, &spec).expect("emit");
        let parsed: ServiceSpec =
            serde_yaml_ng::from_str(&yaml).expect("emitted YAML should parse");
        assert_eq!(parsed.name, "emit-test");
        assert_eq!(parsed.command, "/bin/echo");
    }

    #[test]
    fn config_emitter_extension_is_yaml() {
        let backend = NativeBackend::new();
        assert_eq!(backend.extension(), "yaml");
    }

    #[tokio::test]
    async fn logs_no_spec_returns_message() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());
        let logs = backend.logs("nonexistent", 50).await.expect("logs");
        assert_eq!(logs.len(), 1);
        assert!(logs[0].contains("No log file configured"));
    }

    #[tokio::test]
    async fn logs_with_file_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());

        let log_file = dir.path().join("test.log");
        tokio::fs::write(&log_file, "line1\nline2\nline3\nline4\nline5\n")
            .await
            .expect("write log");

        let mut spec = test_spec("log-test");
        spec.logging.stdout = LogTarget::File(log_file);
        backend.install(&spec).await.expect("install");

        let logs = backend.logs("log-test", 3).await.expect("logs");
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[0], "line3");
        assert_eq!(logs[1], "line4");
        assert_eq!(logs[2], "line5");
    }

    #[tokio::test]
    async fn logs_with_file_target_more_lines_than_available() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());

        let log_file = dir.path().join("small.log");
        tokio::fs::write(&log_file, "only-one\n")
            .await
            .expect("write log");

        let mut spec = test_spec("small-log");
        spec.logging.stdout = LogTarget::File(log_file);
        backend.install(&spec).await.expect("install");

        let logs = backend.logs("small-log", 100).await.expect("logs");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0], "only-one");
    }

    #[tokio::test]
    async fn start_nonexistent_service_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());
        let result = backend.start("no-such-service").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn stop_nonexistent_service_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());
        let result = backend.stop("no-such-service").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn uninstall_nonexistent_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());
        backend
            .uninstall("no-such-service")
            .await
            .expect("uninstall nonexistent should succeed");
    }

    #[test]
    fn default_trait_creates_backend() {
        let backend = NativeBackend::default();
        assert!(backend.available());
    }

    // Phase 5b: Concurrent access test
    #[tokio::test]
    async fn concurrent_service_lifecycle() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = NativeBackend::with_dir(dir.path().to_path_buf());

        // Install 3 services with long-running commands
        let names = ["conc-a", "conc-b", "conc-c"];
        for name in &names {
            let mut spec = ServiceSpec::new(*name, "sleep");
            spec.args = vec!["10".to_owned()];
            spec.restart.strategy = RestartStrategy::Never;
            spec.restart.delay_secs = 1;
            backend.install(&spec).await.expect("install");
        }

        // Start all concurrently
        let (ra, rb, rc) = tokio::join!(
            backend.start("conc-a"),
            backend.start("conc-b"),
            backend.start("conc-c"),
        );
        ra.expect("start a");
        rb.expect("start b");
        rc.expect("start c");

        // Brief pause for processes to spawn
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // All should be listed
        let services = backend.list().await.expect("list");
        assert_eq!(
            services.len(),
            3,
            "expected 3 services, got {}",
            services.len()
        );

        // Stop all concurrently
        let (sa, sb, sc) = tokio::join!(
            backend.stop("conc-a"),
            backend.stop("conc-b"),
            backend.stop("conc-c"),
        );
        sa.expect("stop a");
        sb.expect("stop b");
        sc.expect("stop c");

        // Brief pause for state updates
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Verify all no longer running (they may be Stopped or Failed since
        // SIGTERM causes a non-zero exit code which the watcher task sees as Failed)
        for name in &names {
            let status = backend.status(name).await.expect("status");
            assert!(
                matches!(
                    status.state,
                    ServiceState::Stopped | ServiceState::Inactive | ServiceState::Failed
                ),
                "service {name} should not be running, got {:?}",
                status.state
            );
        }

        // Clean up
        for name in &names {
            backend.uninstall(name).await.expect("uninstall");
        }
    }
}
