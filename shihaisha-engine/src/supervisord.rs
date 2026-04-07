use async_trait::async_trait;
use shihaisha_core::traits::config_translator::ConfigEmitter;
use shihaisha_core::traits::init_backend::InitBackend;
use shihaisha_core::{
    Error, HealthState, LogTarget, RestartStrategy, Result, ServiceSpec, ServiceState,
    ServiceStatus, ServiceType,
};
use std::path::PathBuf;

/// Supervisord backend using `supervisorctl` CLI commands.
///
/// Generates INI-format `[program:name]` config sections and manages
/// services via the `supervisorctl` command-line interface.
/// Config files are written to `~/.config/shihaisha/supervisord.d/`.
pub struct SupervisordBackend {
    config_dir: PathBuf,
    /// Path to the `supervisorctl` binary.
    supervisorctl: String,
}

impl SupervisordBackend {
    /// Create a new supervisord backend with default config directory.
    #[must_use]
    pub fn new() -> Self {
        let config_dir = home_dir()
            .join(".config")
            .join("shihaisha")
            .join("supervisord.d");
        Self {
            config_dir,
            supervisorctl: "supervisorctl".into(),
        }
    }

    async fn supervisorctl(&self, args: &[&str]) -> Result<String> {
        let mut cmd = tokio::process::Command::new(&self.supervisorctl);
        for arg in args {
            cmd.arg(arg);
        }
        let output = cmd.output().await.map_err(|e| Error::BackendError {
            backend: "supervisord".to_owned(),
            operation: "supervisorctl".to_owned(),
            detail: format!("failed to execute: {e}"),
        })?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(Error::BackendError {
                backend: "supervisord".to_owned(),
                operation: "supervisorctl".to_owned(),
                detail: stderr.into_owned(),
            })
        }
    }

    fn conf_path(&self, name: &str) -> PathBuf {
        self.config_dir.join(format!("{name}.conf"))
    }
}

impl Default for SupervisordBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigEmitter for SupervisordBackend {
    fn emit(&self, spec: &ServiceSpec) -> Result<String> {
        Ok(spec_to_conf(spec))
    }

    fn extension(&self) -> &str {
        "conf"
    }

    fn name(&self) -> &str {
        "supervisord"
    }
}

/// Generate a supervisord `[program:name]` INI config section from a `ServiceSpec`.
#[must_use]
pub fn spec_to_conf(spec: &ServiceSpec) -> String {
    let mut conf = String::new();

    conf.push_str(&format!("[program:{}]\n", spec.name));

    // Command
    let command = if spec.args.is_empty() {
        spec.command.clone()
    } else {
        format!("{} {}", spec.command, spec.args.join(" "))
    };
    conf.push_str(&format!("command={command}\n"));

    // Auto-start: oneshot services should not auto-start
    let autostart = !matches!(spec.service_type, ServiceType::Oneshot);
    conf.push_str(&format!(
        "autostart={}\n",
        if autostart { "true" } else { "false" }
    ));

    // Auto-restart mapping
    let autorestart = match spec.restart.strategy {
        RestartStrategy::Always => "true",
        RestartStrategy::OnFailure => "unexpected",
        RestartStrategy::OnSuccess | RestartStrategy::Never => "false",
    };
    conf.push_str(&format!("autorestart={autorestart}\n"));

    // Start seconds (restart delay)
    conf.push_str(&format!("startsecs={}\n", spec.restart.delay_secs));

    // Max retries (default 3 when unset)
    let retries = if spec.restart.max_retries == 0 {
        3
    } else {
        spec.restart.max_retries
    };
    conf.push_str(&format!("startretries={retries}\n"));

    // Stop timeout
    conf.push_str(&format!("stopwaitsecs={}\n", spec.timeout_stop_sec));

    // Working directory
    if let Some(ref wd) = spec.working_directory {
        conf.push_str(&format!("directory={}\n", wd.display()));
    }

    // User
    if let Some(ref user) = spec.user {
        conf.push_str(&format!("user={user}\n"));
    }

    // Environment variables
    if !spec.environment.is_empty() {
        let env_str: Vec<String> = spec
            .environment
            .iter()
            .map(|(k, v)| format!("{k}=\"{v}\""))
            .collect();
        conf.push_str(&format!("environment={}\n", env_str.join(",")));
    }

    // Logging: stdout
    match &spec.logging.stdout {
        LogTarget::File(path) => {
            conf.push_str(&format!("stdout_logfile={}\n", path.display()));
        }
        LogTarget::Null => {
            conf.push_str("stdout_logfile=/dev/null\n");
        }
        LogTarget::Journal | LogTarget::Inherit => {
            conf.push_str("stdout_logfile=AUTO\n");
        }
    }

    // Logging: stderr
    match &spec.logging.stderr {
        LogTarget::File(path) => {
            conf.push_str(&format!("stderr_logfile={}\n", path.display()));
        }
        LogTarget::Null => {
            conf.push_str("stderr_logfile=/dev/null\n");
        }
        LogTarget::Journal | LogTarget::Inherit => {
            conf.push_str("redirect_stderr=true\n");
        }
    }

    // Priority from nice value
    if let Some(ref res) = spec.resources {
        if let Some(nice) = res.nice {
            // supervisord priority: lower = start first (opposite of nice)
            let priority = 999 + nice.value();
            conf.push_str(&format!("priority={priority}\n"));
        }
    }

    conf
}

/// Parse the state portion of a `supervisorctl status` output line.
fn parse_supervisord_state(text: &str) -> ServiceState {
    if text.contains("RUNNING") {
        ServiceState::Running
    } else if text.contains("STOPPED") {
        ServiceState::Stopped
    } else if text.contains("STARTING") {
        ServiceState::Starting
    } else if text.contains("FATAL") || text.contains("BACKOFF") {
        ServiceState::Failed
    } else {
        ServiceState::Unknown
    }
}

/// Extract a PID from `supervisorctl status` output (format: `pid NNNN,`).
fn parse_supervisord_pid(text: &str) -> Option<u32> {
    text.split("pid ")
        .nth(1)
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse().ok())
}

#[async_trait]
impl InitBackend for SupervisordBackend {
    async fn install(&self, spec: &ServiceSpec) -> Result<()> {
        tokio::fs::create_dir_all(&self.config_dir)
            .await
            .map_err(|e| Error::BackendError {
                backend: "supervisord".to_owned(),
                operation: "install".to_owned(),
                detail: format!("failed to create config dir: {e}"),
            })?;

        let conf = spec_to_conf(spec);
        tokio::fs::write(self.conf_path(&spec.name), conf)
            .await
            .map_err(|e| Error::BackendError {
                backend: "supervisord".to_owned(),
                operation: "install".to_owned(),
                detail: format!("failed to write config: {e}"),
            })?;

        // Tell supervisord to re-read and apply config changes
        if let Err(e) = self.supervisorctl(&["reread"]).await {
            tracing::warn!(error = %e, "failed to reread after install, continuing");
        }
        if let Err(e) = self.supervisorctl(&["update"]).await {
            tracing::warn!(error = %e, "failed to update after install, continuing");
        }
        Ok(())
    }

    async fn uninstall(&self, name: &str) -> Result<()> {
        // Stop first, continuing on errors
        if let Err(e) = self.stop(name).await {
            tracing::warn!(service = name, error = %e, "failed to stop during uninstall, continuing");
        }

        let path = self.conf_path(name);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| Error::BackendError {
                    backend: "supervisord".to_owned(),
                    operation: "uninstall".to_owned(),
                    detail: format!("failed to remove config: {e}"),
                })?;
        }

        if let Err(e) = self.supervisorctl(&["reread"]).await {
            tracing::warn!(service = name, error = %e, "failed to reread after uninstall, continuing");
        }
        if let Err(e) = self.supervisorctl(&["update"]).await {
            tracing::warn!(service = name, error = %e, "failed to update after uninstall, continuing");
        }
        Ok(())
    }

    async fn start(&self, name: &str) -> Result<()> {
        self.supervisorctl(&["start", name]).await?;
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        self.supervisorctl(&["stop", name]).await?;
        Ok(())
    }

    async fn restart(&self, name: &str) -> Result<()> {
        self.supervisorctl(&["restart", name]).await?;
        Ok(())
    }

    async fn reload(&self, name: &str) -> Result<()> {
        // supervisord has no per-service reload; restart is the closest equivalent
        self.restart(name).await
    }

    async fn status(&self, name: &str) -> Result<ServiceStatus> {
        let output = self.supervisorctl(&["status", name]).await?;

        let state = parse_supervisord_state(&output);
        let pid = parse_supervisord_pid(&output);

        Ok(ServiceStatus {
            name: name.to_owned(),
            state,
            pid,
            exit_code: None,
            started_at: None,
            uptime_secs: None,
            restart_count: 0,
            health: HealthState::Unknown,
            backend: "supervisord".to_owned(),
            memory_bytes: None,
            cpu_usage_percent: None,
        })
    }

    async fn logs(&self, name: &str, lines: u32) -> Result<Vec<String>> {
        let output = self
            .supervisorctl(&["tail", &format!("-{lines}"), name])
            .await?;
        Ok(output.lines().map(|l| l.to_owned()).collect())
    }

    async fn enable(&self, name: &str) -> Result<()> {
        // supervisord auto-starts by default; enable = start
        self.supervisorctl(&["start", name]).await?;
        Ok(())
    }

    async fn disable(&self, name: &str) -> Result<()> {
        self.supervisorctl(&["stop", name]).await?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<ServiceStatus>> {
        let output = match self.supervisorctl(&["status"]).await {
            Ok(out) => out,
            Err(e) => {
                tracing::debug!(error = %e, "supervisorctl status failed, returning empty list");
                return Ok(Vec::new());
            }
        };

        let mut services = Vec::new();
        for line in output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[0].to_owned();
                let state = match parts[1] {
                    "RUNNING" => ServiceState::Running,
                    "STOPPED" => ServiceState::Stopped,
                    "STARTING" => ServiceState::Starting,
                    "FATAL" | "BACKOFF" => ServiceState::Failed,
                    _ => ServiceState::Unknown,
                };
                let pid = parts
                    .iter()
                    .position(|&p| p == "pid")
                    .and_then(|i| parts.get(i + 1))
                    .and_then(|s| s.trim_end_matches(',').parse().ok());

                services.push(ServiceStatus {
                    name,
                    state,
                    pid,
                    exit_code: None,
                    started_at: None,
                    uptime_secs: None,
                    restart_count: 0,
                    health: HealthState::Unknown,
                    backend: "supervisord".to_owned(),
                    memory_bytes: None,
                    cpu_usage_percent: None,
                });
            }
        }

        Ok(services)
    }

    async fn daemon_reload(&self) -> Result<()> {
        self.supervisorctl(&["reread"]).await?;
        self.supervisorctl(&["update"]).await?;
        Ok(())
    }

    fn available(&self) -> bool {
        std::process::Command::new("which")
            .arg("supervisorctl")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn name(&self) -> &str {
        "supervisord"
    }
}

/// Get the user's home directory.
fn home_dir() -> PathBuf {
    crate::util::home_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shihaisha_core::ResourceLimits;

    fn test_spec() -> ServiceSpec {
        let mut spec = ServiceSpec::new("test-svc", "/usr/bin/test-app");
        spec.description = "Test service".to_owned();
        spec
    }

    #[test]
    fn conf_generation_basic() {
        let spec = test_spec();
        let conf = spec_to_conf(&spec);

        assert!(conf.contains("[program:test-svc]"));
        assert!(conf.contains("command=/usr/bin/test-app"));
        assert!(conf.contains("autostart=true"));
        assert!(conf.contains("autorestart=unexpected")); // OnFailure default
        assert!(conf.contains("startsecs=5"));
        assert!(conf.contains("startretries=3"));
        assert!(conf.contains("stopwaitsecs=90"));
    }

    #[test]
    fn conf_restart_always() {
        let mut spec = test_spec();
        spec.restart.strategy = RestartStrategy::Always;
        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("autorestart=true"),
            "RestartStrategy::Always should map to autorestart=true"
        );
    }

    #[test]
    fn conf_restart_on_failure() {
        let mut spec = test_spec();
        spec.restart.strategy = RestartStrategy::OnFailure;
        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("autorestart=unexpected"),
            "RestartStrategy::OnFailure should map to autorestart=unexpected"
        );
    }

    #[test]
    fn conf_restart_never() {
        let mut spec = test_spec();
        spec.restart.strategy = RestartStrategy::Never;
        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("autorestart=false"),
            "RestartStrategy::Never should map to autorestart=false"
        );
    }

    #[test]
    fn conf_restart_on_success() {
        let mut spec = test_spec();
        spec.restart.strategy = RestartStrategy::OnSuccess;
        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("autorestart=false"),
            "RestartStrategy::OnSuccess should map to autorestart=false (supervisord has no equivalent)"
        );
    }

    #[test]
    fn conf_environment() {
        let mut spec = test_spec();
        spec.environment
            .insert("RUST_LOG".to_owned(), "info".to_owned());

        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("environment="),
            "should contain environment directive"
        );
        assert!(
            conf.contains("RUST_LOG=\"info\""),
            "should contain quoted env var"
        );
    }

    #[test]
    fn conf_with_working_directory() {
        let mut spec = test_spec();
        spec.working_directory = Some(PathBuf::from("/var/www"));

        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("directory=/var/www"),
            "should contain directory directive"
        );
    }

    #[test]
    fn conf_with_user() {
        let mut spec = test_spec();
        spec.user = Some("www-data".to_owned());

        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("user=www-data"),
            "should contain user directive"
        );
    }

    #[test]
    fn conf_oneshot_no_autostart() {
        let mut spec = test_spec();
        spec.service_type = ServiceType::Oneshot;

        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("autostart=false"),
            "oneshot services should not auto-start"
        );
    }

    #[test]
    fn conf_with_args() {
        let mut spec = test_spec();
        spec.args = vec!["--port".to_owned(), "8080".to_owned(), "--verbose".to_owned()];

        let conf = spec_to_conf(&spec);

        assert!(conf.contains("command=/usr/bin/test-app --port 8080 --verbose"));
    }

    #[test]
    fn conf_priority_from_nice() {
        use shihaisha_core::types::resource_limits::NiceValue;
        let mut spec = test_spec();
        spec.resources = Some(ResourceLimits {
            memory_max: None,
            memory_high: None,
            cpu_weight: None,
            cpu_quota: None,
            tasks_max: None,
            io_weight: None,
            nice: Some(NiceValue::new(5).unwrap()),
        });

        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("priority=1004"),
            "nice=5 should map to priority=1004 (999+5)"
        );
    }

    #[test]
    fn conf_logging_file_stdout() {
        let mut spec = test_spec();
        spec.logging.stdout = LogTarget::File(PathBuf::from("/var/log/app/stdout.log"));

        let conf = spec_to_conf(&spec);

        assert!(conf.contains("stdout_logfile=/var/log/app/stdout.log"));
    }

    #[test]
    fn conf_logging_null_stderr() {
        let mut spec = test_spec();
        spec.logging.stderr = LogTarget::Null;

        let conf = spec_to_conf(&spec);

        assert!(conf.contains("stderr_logfile=/dev/null"));
    }

    #[test]
    fn conf_max_retries_custom() {
        let mut spec = test_spec();
        spec.restart.max_retries = 10;

        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("startretries=10"),
            "custom max_retries should be used"
        );
    }

    #[test]
    fn parse_state_running() {
        assert_eq!(
            parse_supervisord_state("test-svc    RUNNING    pid 12345, uptime 0:05:00"),
            ServiceState::Running
        );
    }

    #[test]
    fn parse_state_stopped() {
        assert_eq!(
            parse_supervisord_state("test-svc    STOPPED    Not started"),
            ServiceState::Stopped
        );
    }

    #[test]
    fn parse_state_fatal() {
        assert_eq!(
            parse_supervisord_state("test-svc    FATAL      Exited too quickly"),
            ServiceState::Failed
        );
    }

    #[test]
    fn parse_state_backoff() {
        assert_eq!(
            parse_supervisord_state("test-svc    BACKOFF    Exited too quickly"),
            ServiceState::Failed
        );
    }

    #[test]
    fn parse_state_starting() {
        assert_eq!(
            parse_supervisord_state("test-svc    STARTING"),
            ServiceState::Starting
        );
    }

    #[test]
    fn parse_state_unknown() {
        assert_eq!(
            parse_supervisord_state("test-svc    EXITED"),
            ServiceState::Unknown
        );
    }

    #[test]
    fn parse_pid_present() {
        assert_eq!(
            parse_supervisord_pid("test-svc    RUNNING    pid 12345, uptime 0:05:00"),
            Some(12345)
        );
    }

    #[test]
    fn parse_pid_absent() {
        assert_eq!(
            parse_supervisord_pid("test-svc    STOPPED    Not started"),
            None
        );
    }

    #[test]
    fn available_check_does_not_panic() {
        let backend = SupervisordBackend::new();
        // On most dev machines supervisorctl is not installed; just verify no panic.
        let _ = backend.available();
    }

    #[test]
    fn backend_name_is_supervisord() {
        let backend = SupervisordBackend::new();
        assert_eq!(InitBackend::name(&backend), "supervisord");
    }

    #[test]
    fn conf_logging_journal_uses_auto() {
        let spec = test_spec();
        let conf = spec_to_conf(&spec);
        assert!(
            conf.contains("stdout_logfile=AUTO"),
            "Journal logging should map to AUTO: {conf}"
        );
    }

    #[test]
    fn conf_logging_inherit_uses_redirect() {
        let mut spec = test_spec();
        spec.logging.stdout = LogTarget::Inherit;
        spec.logging.stderr = LogTarget::Inherit;
        let conf = spec_to_conf(&spec);
        assert!(
            conf.contains("redirect_stderr=true"),
            "Inherit logging should set redirect_stderr"
        );
    }

    #[test]
    fn conf_logging_file_stderr() {
        let mut spec = test_spec();
        spec.logging.stderr = LogTarget::File(PathBuf::from("/var/log/err.log"));
        let conf = spec_to_conf(&spec);
        assert!(conf.contains("stderr_logfile=/var/log/err.log"));
    }

    #[test]
    fn conf_logging_null_stdout() {
        let mut spec = test_spec();
        spec.logging.stdout = LogTarget::Null;
        let conf = spec_to_conf(&spec);
        assert!(conf.contains("stdout_logfile=/dev/null"));
    }

    #[test]
    fn parse_state_stopping_maps_to_unknown() {
        assert_eq!(
            parse_supervisord_state("test-svc    STOPPING"),
            ServiceState::Unknown,
            "supervisord STOPPING falls through to Unknown (not modeled)"
        );
    }

    #[test]
    fn parse_state_empty_string() {
        assert_eq!(
            parse_supervisord_state(""),
            ServiceState::Unknown
        );
    }

    #[test]
    fn conf_stop_wait_from_timeout() {
        let mut spec = test_spec();
        spec.timeout_stop_sec = 120;
        let conf = spec_to_conf(&spec);
        assert!(conf.contains("stopwaitsecs=120"));
    }

    #[test]
    fn conf_delay_secs_maps_to_startsecs() {
        let mut spec = test_spec();
        spec.restart.delay_secs = 15;
        let conf = spec_to_conf(&spec);
        assert!(conf.contains("startsecs=15"));
    }

    #[test]
    fn config_emitter_extension_is_conf() {
        let backend = SupervisordBackend::new();
        assert_eq!(ConfigEmitter::extension(&backend), "conf");
        assert_eq!(ConfigEmitter::name(&backend), "supervisord");
    }

    #[test]
    fn conf_no_resources_omits_priority() {
        let mut spec = test_spec();
        spec.resources = None;
        let conf = spec_to_conf(&spec);
        assert!(
            !conf.contains("priority="),
            "no resources should not add priority"
        );
    }

    #[test]
    fn conf_resources_without_nice_omits_priority() {
        let spec_with_mem = {
            let mut spec = test_spec();
            spec.resources = Some(ResourceLimits {
                memory_max: Some(shihaisha_core::MemorySize::parse("1G").unwrap()),
                ..ResourceLimits::default()
            });
            spec
        };
        let conf = spec_to_conf(&spec_with_mem);
        assert!(
            !conf.contains("priority="),
            "resources without nice should not add priority"
        );
    }
}
