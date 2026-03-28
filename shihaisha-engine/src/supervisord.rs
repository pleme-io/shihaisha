use async_trait::async_trait;
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
        let output = cmd
            .output()
            .await
            .map_err(|e| Error::BackendError(format!("supervisorctl failed: {e}")))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(Error::BackendError(format!("supervisorctl error: {stderr}")))
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
            let priority = 999 + nice;
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
            .map_err(|e| Error::BackendError(format!("failed to create config dir: {e}")))?;

        let conf = spec_to_conf(spec);
        tokio::fs::write(self.conf_path(&spec.name), conf)
            .await
            .map_err(|e| Error::BackendError(format!("failed to write config: {e}")))?;

        // Tell supervisord to re-read and apply config changes
        let _ = self.supervisorctl(&["reread"]).await;
        let _ = self.supervisorctl(&["update"]).await;
        Ok(())
    }

    async fn uninstall(&self, name: &str) -> Result<()> {
        // Stop first, ignore errors
        let _ = self.stop(name).await;

        let path = self.conf_path(name);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| Error::BackendError(format!("failed to remove config: {e}")))?;
        }

        let _ = self.supervisorctl(&["reread"]).await;
        let _ = self.supervisorctl(&["update"]).await;
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
        let output = self
            .supervisorctl(&["status"])
            .await
            .unwrap_or_default();

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
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use shihaisha_core::{
        BackendOverrides, DependencySpec, LoggingSpec, ResourceLimits, RestartPolicy,
    };
    use std::collections::HashMap;

    fn minimal_spec() -> ServiceSpec {
        ServiceSpec {
            name: "test-svc".to_owned(),
            description: "Test service".to_owned(),
            command: "/usr/bin/test-app".to_owned(),
            args: vec![],
            service_type: ServiceType::Simple,
            working_directory: None,
            user: None,
            group: None,
            environment: HashMap::new(),
            restart: RestartPolicy::default(),
            depends_on: DependencySpec::default(),
            health: None,
            sockets: vec![],
            resources: None,
            logging: LoggingSpec::default(),
            notify: false,
            watchdog_sec: 0,
            timeout_start_sec: 90,
            timeout_stop_sec: 90,
            overrides: BackendOverrides::default(),
        }
    }

    #[test]
    fn conf_generation_basic() {
        let spec = minimal_spec();
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
        let mut spec = minimal_spec();
        spec.restart.strategy = RestartStrategy::Always;
        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("autorestart=true"),
            "RestartStrategy::Always should map to autorestart=true"
        );
    }

    #[test]
    fn conf_restart_on_failure() {
        let mut spec = minimal_spec();
        spec.restart.strategy = RestartStrategy::OnFailure;
        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("autorestart=unexpected"),
            "RestartStrategy::OnFailure should map to autorestart=unexpected"
        );
    }

    #[test]
    fn conf_restart_never() {
        let mut spec = minimal_spec();
        spec.restart.strategy = RestartStrategy::Never;
        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("autorestart=false"),
            "RestartStrategy::Never should map to autorestart=false"
        );
    }

    #[test]
    fn conf_restart_on_success() {
        let mut spec = minimal_spec();
        spec.restart.strategy = RestartStrategy::OnSuccess;
        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("autorestart=false"),
            "RestartStrategy::OnSuccess should map to autorestart=false (supervisord has no equivalent)"
        );
    }

    #[test]
    fn conf_environment() {
        let mut spec = minimal_spec();
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
        let mut spec = minimal_spec();
        spec.working_directory = Some(PathBuf::from("/var/www"));

        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("directory=/var/www"),
            "should contain directory directive"
        );
    }

    #[test]
    fn conf_with_user() {
        let mut spec = minimal_spec();
        spec.user = Some("www-data".to_owned());

        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("user=www-data"),
            "should contain user directive"
        );
    }

    #[test]
    fn conf_oneshot_no_autostart() {
        let mut spec = minimal_spec();
        spec.service_type = ServiceType::Oneshot;

        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("autostart=false"),
            "oneshot services should not auto-start"
        );
    }

    #[test]
    fn conf_with_args() {
        let mut spec = minimal_spec();
        spec.args = vec!["--port".to_owned(), "8080".to_owned(), "--verbose".to_owned()];

        let conf = spec_to_conf(&spec);

        assert!(conf.contains("command=/usr/bin/test-app --port 8080 --verbose"));
    }

    #[test]
    fn conf_priority_from_nice() {
        let mut spec = minimal_spec();
        spec.resources = Some(ResourceLimits {
            memory_max: None,
            memory_high: None,
            cpu_weight: None,
            cpu_quota: None,
            tasks_max: None,
            io_weight: None,
            nice: Some(5),
        });

        let conf = spec_to_conf(&spec);

        assert!(
            conf.contains("priority=1004"),
            "nice=5 should map to priority=1004 (999+5)"
        );
    }

    #[test]
    fn conf_logging_file_stdout() {
        let mut spec = minimal_spec();
        spec.logging.stdout = LogTarget::File(PathBuf::from("/var/log/app/stdout.log"));

        let conf = spec_to_conf(&spec);

        assert!(conf.contains("stdout_logfile=/var/log/app/stdout.log"));
    }

    #[test]
    fn conf_logging_null_stderr() {
        let mut spec = minimal_spec();
        spec.logging.stderr = LogTarget::Null;

        let conf = spec_to_conf(&spec);

        assert!(conf.contains("stderr_logfile=/dev/null"));
    }

    #[test]
    fn conf_max_retries_custom() {
        let mut spec = minimal_spec();
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
        assert_eq!(backend.name(), "supervisord");
    }
}
