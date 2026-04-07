use async_trait::async_trait;
use shihaisha_core::traits::config_translator::ConfigEmitter;
use shihaisha_core::traits::init_backend::InitBackend;
use shihaisha_core::{
    Error, HealthState, LogTarget, RestartStrategy, Result, ServiceSpec, ServiceState,
    ServiceStatus, ServiceType,
};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;

/// Systemd backend using `systemctl` and `journalctl` CLI commands.
///
/// Operates in user mode (`--user`) when not root, or system mode otherwise.
/// Unit files are written to `~/.config/systemd/user/` or `/etc/systemd/system/`.
pub struct SystemdBackend {
    user_mode: bool,
    unit_dir: PathBuf,
}

impl SystemdBackend {
    /// Create a new systemd backend, auto-detecting user vs system mode.
    #[must_use]
    pub fn new() -> Self {
        let user_mode = !is_root();
        let unit_dir = if user_mode {
            home_dir().join(".config/systemd/user")
        } else {
            PathBuf::from("/etc/systemd/system")
        };
        Self { user_mode, unit_dir }
    }

    fn systemctl_base_args(&self) -> Vec<&str> {
        if self.user_mode {
            vec!["--user"]
        } else {
            vec![]
        }
    }

    async fn systemctl(&self, args: &[&str]) -> Result<String> {
        let mut cmd = tokio::process::Command::new("systemctl");
        for arg in &self.systemctl_base_args() {
            cmd.arg(arg);
        }
        for arg in args {
            cmd.arg(arg);
        }
        let output = cmd.output().await.map_err(|e| Error::BackendError {
            backend: "systemd".to_owned(),
            operation: "systemctl".to_owned(),
            detail: format!("failed to execute: {e}"),
        })?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(Error::BackendError {
                backend: "systemd".to_owned(),
                operation: "systemctl".to_owned(),
                detail: stderr.into_owned(),
            })
        }
    }

    fn unit_path(&self, name: &str) -> PathBuf {
        self.unit_dir.join(format!("{name}.service"))
    }
}

impl Default for SystemdBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigEmitter for SystemdBackend {
    fn emit(&self, spec: &ServiceSpec) -> Result<String> {
        Ok(spec_to_unit(spec))
    }

    fn extension(&self) -> &'static str {
        "service"
    }

    fn name(&self) -> &'static str {
        "systemd"
    }
}

/// Generate a systemd unit file string from a `ServiceSpec`.
#[must_use]
pub fn spec_to_unit(spec: &ServiceSpec) -> String {
    let mut unit = String::new();
    emit_unit_section(&mut unit, spec);
    unit.push('\n');
    emit_service_section(&mut unit, spec);
    unit.push('\n');
    unit.push_str("[Install]\nWantedBy=default.target\n");
    unit
}

fn emit_unit_section(unit: &mut String, spec: &ServiceSpec) {
    unit.push_str("[Unit]\n");
    let _ = writeln!(unit, "Description={}", spec.description);
    for dep in &spec.depends_on.after {
        let _ = writeln!(unit, "After={dep}.service");
    }
    for dep in &spec.depends_on.before {
        let _ = writeln!(unit, "Before={dep}.service");
    }
    for dep in &spec.depends_on.requires {
        let _ = writeln!(unit, "Requires={dep}.service");
    }
    for dep in &spec.depends_on.wants {
        let _ = writeln!(unit, "Wants={dep}.service");
    }
    for dep in &spec.depends_on.conflicts {
        let _ = writeln!(unit, "Conflicts={dep}.service");
    }
    if let Some(overrides) = spec.overrides.systemd.get("Unit") {
        for (k, v) in overrides {
            let _ = writeln!(unit, "{k}={v}");
        }
    }
}

fn emit_service_section(unit: &mut String, spec: &ServiceSpec) {
    unit.push_str("[Service]\n");

    let svc_type = match spec.service_type {
        ServiceType::Simple | ServiceType::Timer | ServiceType::Socket => "simple",
        ServiceType::Oneshot => "oneshot",
        ServiceType::Notify => "notify",
        ServiceType::Forking => "forking",
    };
    let _ = writeln!(unit, "Type={svc_type}");

    if spec.args.is_empty() {
        let _ = writeln!(unit, "ExecStart={}", spec.command);
    } else {
        let _ = writeln!(unit, "ExecStart={} {}", spec.command, spec.args.join(" "));
    }

    let restart = match spec.restart.strategy {
        RestartStrategy::Always => "always",
        RestartStrategy::OnFailure => "on-failure",
        RestartStrategy::OnSuccess => "on-success",
        RestartStrategy::Never => "no",
    };
    let _ = writeln!(unit, "Restart={restart}");
    let _ = writeln!(unit, "RestartSec={}", spec.restart.delay_secs);

    if let Some(ref wd) = spec.working_directory {
        let _ = writeln!(unit, "WorkingDirectory={}", wd.display());
    }
    if let Some(ref user) = spec.user {
        let _ = writeln!(unit, "User={user}");
    }
    if let Some(ref group) = spec.group {
        let _ = writeln!(unit, "Group={group}");
    }
    for (k, v) in &spec.environment {
        let _ = writeln!(unit, "Environment=\"{k}={v}\"");
    }

    emit_log_target(unit, "StandardOutput", &spec.logging.stdout);
    emit_log_target(unit, "StandardError", &spec.logging.stderr);

    if spec.notify {
        unit.push_str("NotifyAccess=main\n");
    }
    if spec.watchdog_sec > 0 {
        let _ = writeln!(unit, "WatchdogSec={}", spec.watchdog_sec);
    }
    let _ = writeln!(unit, "TimeoutStartSec={}", spec.timeout_start_sec);
    let _ = writeln!(unit, "TimeoutStopSec={}", spec.timeout_stop_sec);

    if let Some(ref res) = spec.resources {
        emit_resource_limits(unit, res);
    }

    if let Some(overrides) = spec.overrides.systemd.get("Service") {
        for (k, v) in overrides {
            let _ = writeln!(unit, "{k}={v}");
        }
    }
}

fn emit_log_target(unit: &mut String, directive: &str, target: &LogTarget) {
    match target {
        LogTarget::Journal => {}
        LogTarget::File(path) => {
            let _ = writeln!(unit, "{directive}=file:{}", path.display());
        }
        LogTarget::Null => {
            let _ = writeln!(unit, "{directive}=null");
        }
        LogTarget::Inherit => {
            let _ = writeln!(unit, "{directive}=inherit");
        }
    }
}

fn emit_resource_limits(unit: &mut String, res: &shihaisha_core::ResourceLimits) {
    if let Some(ref m) = res.memory_max {
        let _ = writeln!(unit, "MemoryMax={m}");
    }
    if let Some(ref m) = res.memory_high {
        let _ = writeln!(unit, "MemoryHigh={m}");
    }
    if let Some(w) = res.cpu_weight {
        let _ = writeln!(unit, "CPUWeight={w}");
    }
    if let Some(ref q) = res.cpu_quota {
        let _ = writeln!(unit, "CPUQuota={q}");
    }
    if let Some(t) = res.tasks_max {
        let _ = writeln!(unit, "TasksMax={t}");
    }
    if let Some(w) = res.io_weight {
        let _ = writeln!(unit, "IOWeight={w}");
    }
    if let Some(n) = res.nice {
        let _ = writeln!(unit, "Nice={n}");
    }
}

fn parse_systemctl_show(output: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in output.lines() {
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.to_owned(), value.to_owned());
        }
    }
    map
}

#[async_trait]
impl InitBackend for SystemdBackend {
    async fn install(&self, spec: &ServiceSpec) -> Result<()> {
        let unit_content = spec_to_unit(spec);
        let path = self.unit_path(&spec.name);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| Error::BackendError {
                    backend: "systemd".to_owned(),
                    operation: "install".to_owned(),
                    detail: format!("failed to create unit dir: {e}"),
                })?;
        }

        tokio::fs::write(&path, unit_content)
            .await
            .map_err(|e| Error::BackendError {
                backend: "systemd".to_owned(),
                operation: "install".to_owned(),
                detail: format!("failed to write unit file: {e}"),
            })?;

        self.daemon_reload().await?;
        Ok(())
    }

    async fn uninstall(&self, name: &str) -> Result<()> {
        // Stop and disable first, continuing on errors
        if let Err(e) = self.stop(name).await {
            tracing::warn!(service = name, error = %e, "failed to stop during uninstall, continuing");
        }
        if let Err(e) = self.disable(name).await {
            tracing::warn!(service = name, error = %e, "failed to disable during uninstall, continuing");
        }

        let path = self.unit_path(name);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| Error::BackendError {
                    backend: "systemd".to_owned(),
                    operation: "uninstall".to_owned(),
                    detail: format!("failed to remove unit file: {e}"),
                })?;
        }

        self.daemon_reload().await?;
        Ok(())
    }

    async fn start(&self, name: &str) -> Result<()> {
        self.systemctl(&["start", &format!("{name}.service")])
            .await?;
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        self.systemctl(&["stop", &format!("{name}.service")])
            .await?;
        Ok(())
    }

    async fn restart(&self, name: &str) -> Result<()> {
        self.systemctl(&["restart", &format!("{name}.service")])
            .await?;
        Ok(())
    }

    async fn reload(&self, name: &str) -> Result<()> {
        self.systemctl(&["reload", &format!("{name}.service")])
            .await?;
        Ok(())
    }

    async fn status(&self, name: &str) -> Result<ServiceStatus> {
        let props = [
            "ActiveState",
            "SubState",
            "MainPID",
            "ExecMainStatus",
            "ActiveEnterTimestamp",
            "NRestarts",
            "MemoryCurrent",
        ];
        let property_arg = props.join(",");
        let output = self
            .systemctl(&[
                "show",
                &format!("{name}.service"),
                &format!("--property={property_arg}"),
            ])
            .await?;

        let props_map = parse_systemctl_show(&output);

        let state = match props_map.get("ActiveState").map(String::as_str) {
            Some("active") => ServiceState::Running,
            Some("inactive") => ServiceState::Inactive,
            Some("failed") => ServiceState::Failed,
            Some("activating") => ServiceState::Starting,
            Some("deactivating") => ServiceState::Stopping,
            Some("reloading") => ServiceState::Reloading,
            _ => ServiceState::Unknown,
        };

        let pid = props_map
            .get("MainPID")
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|&p| p > 0);

        let exit_code = props_map
            .get("ExecMainStatus")
            .and_then(|s| s.parse::<i32>().ok());

        let restart_count = props_map
            .get("NRestarts")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        let memory_bytes = props_map
            .get("MemoryCurrent")
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|&m| m < u64::MAX); // systemd uses max for "not available"

        Ok(ServiceStatus {
            name: name.to_owned(),
            state,
            pid,
            exit_code,
            started_at: None,
            uptime_secs: None,
            restart_count,
            health: HealthState::Unknown,
            backend: "systemd".to_owned(),
            memory_bytes,
            cpu_usage_percent: None,
        })
    }

    async fn logs(&self, name: &str, lines: u32) -> Result<Vec<String>> {
        let mut cmd = tokio::process::Command::new("journalctl");
        if self.user_mode {
            cmd.arg("--user");
        }
        cmd.args([
            "-u",
            &format!("{name}.service"),
            "-n",
            &lines.to_string(),
            "--no-pager",
        ]);

        let output = cmd.output().await.map_err(|e| Error::BackendError {
            backend: "systemd".to_owned(),
            operation: "logs".to_owned(),
            detail: format!("journalctl failed to execute: {e}"),
        })?;

        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            Ok(text.lines().map(ToOwned::to_owned).collect())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(Error::BackendError {
                backend: "systemd".to_owned(),
                operation: "logs".to_owned(),
                detail: format!("journalctl error: {stderr}"),
            })
        }
    }

    async fn enable(&self, name: &str) -> Result<()> {
        self.systemctl(&["enable", &format!("{name}.service")])
            .await?;
        Ok(())
    }

    async fn disable(&self, name: &str) -> Result<()> {
        self.systemctl(&["disable", &format!("{name}.service")])
            .await?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<ServiceStatus>> {
        let output = self
            .systemctl(&["list-units", "--type=service", "--no-pager", "--plain", "--no-legend"])
            .await?;

        let mut services = Vec::new();
        for line in output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let unit_name = parts[0].trim_end_matches(".service");
                let state = match parts[2] {
                    "running" => ServiceState::Running,
                    "exited" => ServiceState::Stopped,
                    "failed" => ServiceState::Failed,
                    "waiting" => ServiceState::Inactive,
                    "start" | "start-pre" | "start-post" => ServiceState::Starting,
                    "stop" | "stop-pre" | "stop-post" => ServiceState::Stopping,
                    _ => ServiceState::Unknown,
                };

                services.push(ServiceStatus {
                    name: unit_name.to_owned(),
                    state,
                    pid: None,
                    exit_code: None,
                    started_at: None,
                    uptime_secs: None,
                    restart_count: 0,
                    health: HealthState::Unknown,
                    backend: "systemd".to_owned(),
                    memory_bytes: None,
                    cpu_usage_percent: None,
                });
            }
        }

        Ok(services)
    }

    async fn daemon_reload(&self) -> Result<()> {
        self.systemctl(&["daemon-reload"]).await?;
        Ok(())
    }

    fn available(&self) -> bool {
        std::process::Command::new("systemctl")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn name(&self) -> &'static str {
        "systemd"
    }
}

/// Check if the current process is running as root.
fn is_root() -> bool {
    crate::util::is_root()
}

/// Get the user's home directory.
fn home_dir() -> PathBuf {
    crate::util::home_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shihaisha_core::{MemorySize, NiceValue, ResourceLimits, Weight};

    fn test_spec() -> ServiceSpec {
        let mut spec = ServiceSpec::new("test-svc", "/usr/bin/test-app");
        spec.description = "Test service".to_owned();
        spec
    }

    #[test]
    fn unit_generation_minimal() {
        let spec = test_spec();
        let unit = spec_to_unit(&spec);

        assert!(unit.contains("[Unit]"));
        assert!(unit.contains("Description=Test service"));
        assert!(unit.contains("[Service]"));
        assert!(unit.contains("Type=simple"));
        assert!(unit.contains("ExecStart=/usr/bin/test-app"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("RestartSec=5"));
        assert!(unit.contains("TimeoutStartSec=90"));
        assert!(unit.contains("TimeoutStopSec=90"));
        assert!(unit.contains("[Install]"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn unit_generation_with_deps_and_env() {
        let mut spec = test_spec();
        spec.depends_on.after = vec!["database".to_owned()];
        spec.depends_on.requires = vec!["database".to_owned()];
        spec.depends_on.conflicts = vec!["legacy-app".to_owned()];
        spec.environment
            .insert("RUST_LOG".to_owned(), "info".to_owned());
        spec.environment
            .insert("PORT".to_owned(), "8080".to_owned());
        spec.user = Some("www-data".to_owned());
        spec.group = Some("www-data".to_owned());
        spec.working_directory = Some(PathBuf::from("/var/www"));

        let unit = spec_to_unit(&spec);

        assert!(unit.contains("After=database.service"));
        assert!(unit.contains("Requires=database.service"));
        assert!(unit.contains("Conflicts=legacy-app.service"));
        assert!(unit.contains("User=www-data"));
        assert!(unit.contains("Group=www-data"));
        assert!(unit.contains("WorkingDirectory=/var/www"));
        // Environment entries (order not guaranteed in HashMap)
        assert!(unit.contains("Environment=\"RUST_LOG=info\""));
        assert!(unit.contains("Environment=\"PORT=8080\""));
    }

    #[test]
    fn restart_policy_mapping() {
        let strategies = [
            (RestartStrategy::Always, "Restart=always"),
            (RestartStrategy::OnFailure, "Restart=on-failure"),
            (RestartStrategy::OnSuccess, "Restart=on-success"),
            (RestartStrategy::Never, "Restart=no"),
        ];

        for (strategy, expected) in strategies {
            let mut spec = test_spec();
            spec.restart.strategy = strategy;
            let unit = spec_to_unit(&spec);
            assert!(
                unit.contains(expected),
                "expected {expected} for {strategy:?}, got:\n{unit}"
            );
        }
    }

    #[test]
    fn service_type_mapping() {
        let types = [
            (ServiceType::Simple, "Type=simple"),
            (ServiceType::Oneshot, "Type=oneshot"),
            (ServiceType::Notify, "Type=notify"),
            (ServiceType::Forking, "Type=forking"),
        ];

        for (svc_type, expected) in types {
            let mut spec = test_spec();
            spec.service_type = svc_type;
            let unit = spec_to_unit(&spec);
            assert!(
                unit.contains(expected),
                "expected {expected} for {svc_type:?}, got:\n{unit}"
            );
        }
    }

    #[test]
    fn resource_limits_in_unit() {
        let mut spec = test_spec();
        spec.resources = Some(ResourceLimits {
            memory_max: Some(MemorySize::parse("512M").unwrap()),
            memory_high: Some(MemorySize::parse("384M").unwrap()),
            cpu_weight: Some(Weight::new(500).unwrap()),
            cpu_quota: Some("50%".to_owned()),
            tasks_max: Some(256),
            io_weight: Some(Weight::new(100).unwrap()),
            nice: Some(NiceValue::new(5).unwrap()),
        });

        let unit = spec_to_unit(&spec);

        assert!(unit.contains("MemoryMax=512M"));
        assert!(unit.contains("MemoryHigh=384M"));
        assert!(unit.contains("CPUWeight=500"));
        assert!(unit.contains("CPUQuota=50%"));
        assert!(unit.contains("TasksMax=256"));
        assert!(unit.contains("IOWeight=100"));
        assert!(unit.contains("Nice=5"));
    }

    #[test]
    fn notify_and_watchdog() {
        let mut spec = test_spec();
        spec.notify = true;
        spec.watchdog_sec = 30;

        let unit = spec_to_unit(&spec);

        assert!(unit.contains("NotifyAccess=main"));
        assert!(unit.contains("WatchdogSec=30"));
    }

    #[test]
    fn backend_overrides_applied() {
        let mut spec = test_spec();
        let mut svc_overrides = HashMap::new();
        svc_overrides.insert("LimitNOFILE".to_owned(), "65536".to_owned());
        svc_overrides.insert("ProtectHome".to_owned(), "yes".to_owned());
        spec.overrides.systemd.insert("Service".to_owned(), svc_overrides);

        let unit = spec_to_unit(&spec);

        assert!(unit.contains("LimitNOFILE=65536"));
        assert!(unit.contains("ProtectHome=yes"));
    }

    #[test]
    fn exec_start_with_args() {
        let mut spec = test_spec();
        spec.args = vec!["--port".to_owned(), "8080".to_owned(), "--verbose".to_owned()];

        let unit = spec_to_unit(&spec);

        assert!(unit.contains("ExecStart=/usr/bin/test-app --port 8080 --verbose"));
    }

    #[test]
    fn available_check_on_non_systemd() {
        // On macOS this should return false; on Linux it depends on the system.
        let backend = SystemdBackend::new();
        // We just verify it doesn't panic.
        let _ = backend.available();
    }

    #[test]
    fn parse_systemctl_show_output() {
        let output = "\
ActiveState=active
SubState=running
MainPID=12345
ExecMainStatus=0
NRestarts=2
MemoryCurrent=104857600
";
        let props = parse_systemctl_show(output);
        assert_eq!(props.get("ActiveState").unwrap(), "active");
        assert_eq!(props.get("MainPID").unwrap(), "12345");
        assert_eq!(props.get("NRestarts").unwrap(), "2");
        assert_eq!(props.get("MemoryCurrent").unwrap(), "104857600");
    }

    #[test]
    fn logging_file_targets_in_unit() {
        let mut spec = test_spec();
        spec.logging.stdout = LogTarget::File(PathBuf::from("/var/log/app/stdout.log"));
        spec.logging.stderr = LogTarget::Null;

        let unit = spec_to_unit(&spec);

        assert!(unit.contains("StandardOutput=file:/var/log/app/stdout.log"));
        assert!(unit.contains("StandardError=null"));
    }

    #[test]
    fn timer_and_socket_types_map_to_simple() {
        for svc_type in [ServiceType::Timer, ServiceType::Socket] {
            let mut spec = test_spec();
            spec.service_type = svc_type;
            let unit = spec_to_unit(&spec);
            assert!(
                unit.contains("Type=simple"),
                "{svc_type:?} should map to simple"
            );
        }
    }

    #[test]
    fn before_and_wants_in_unit() {
        let mut spec = test_spec();
        spec.depends_on.before = vec!["proxy".to_owned()];
        spec.depends_on.wants = vec!["cache".to_owned()];
        let unit = spec_to_unit(&spec);
        assert!(unit.contains("Before=proxy.service"), "unit:\n{unit}");
        assert!(unit.contains("Wants=cache.service"), "unit:\n{unit}");
    }

    #[test]
    fn logging_inherit_in_unit() {
        let mut spec = test_spec();
        spec.logging.stdout = LogTarget::Inherit;
        spec.logging.stderr = LogTarget::Inherit;
        let unit = spec_to_unit(&spec);
        assert!(unit.contains("StandardOutput=inherit"));
        assert!(unit.contains("StandardError=inherit"));
    }

    #[test]
    fn logging_journal_omits_standard_output() {
        let spec = test_spec();
        let unit = spec_to_unit(&spec);
        assert!(
            !unit.contains("StandardOutput="),
            "journal logging should not add StandardOutput"
        );
        assert!(
            !unit.contains("StandardError="),
            "journal logging should not add StandardError"
        );
    }

    #[test]
    fn unit_overrides_applied() {
        let mut spec = test_spec();
        let mut unit_overrides = HashMap::new();
        unit_overrides.insert("Documentation".to_owned(), "man:myapp(1)".to_owned());
        spec.overrides.systemd.insert("Unit".to_owned(), unit_overrides);
        let unit = spec_to_unit(&spec);
        assert!(unit.contains("Documentation=man:myapp(1)"));
    }

    #[test]
    fn parse_systemctl_show_empty_input() {
        let props = parse_systemctl_show("");
        assert!(props.is_empty());
    }

    #[test]
    fn parse_systemctl_show_malformed_lines() {
        let output = "no-equals-sign\nAlso bad\nGood=value\n";
        let props = parse_systemctl_show(output);
        assert_eq!(props.len(), 1);
        assert_eq!(props.get("Good").unwrap(), "value");
    }

    #[test]
    fn no_resources_omits_directives() {
        let mut spec = test_spec();
        spec.resources = None;
        let unit = spec_to_unit(&spec);
        assert!(!unit.contains("MemoryMax="));
        assert!(!unit.contains("CPUWeight="));
        assert!(!unit.contains("TasksMax="));
    }

    #[test]
    fn delay_secs_in_restart_sec() {
        let mut spec = test_spec();
        spec.restart.delay_secs = 42;
        let unit = spec_to_unit(&spec);
        assert!(unit.contains("RestartSec=42"));
    }

    #[test]
    fn custom_timeouts_in_unit() {
        let mut spec = test_spec();
        spec.timeout_start_sec = 120;
        spec.timeout_stop_sec = 30;
        let unit = spec_to_unit(&spec);
        assert!(unit.contains("TimeoutStartSec=120"));
        assert!(unit.contains("TimeoutStopSec=30"));
    }

    #[test]
    fn systemd_backend_name() {
        let backend = SystemdBackend::new();
        assert_eq!(InitBackend::name(&backend), "systemd");
    }

    #[test]
    fn config_emitter_emit_produces_same_as_spec_to_unit() {
        let backend = SystemdBackend::new();
        let spec = test_spec();
        let via_trait = ConfigEmitter::emit(&backend, &spec).expect("emit");
        let via_fn = spec_to_unit(&spec);
        assert_eq!(via_trait, via_fn);
    }

    #[test]
    fn config_emitter_extension() {
        let backend = SystemdBackend::new();
        assert_eq!(backend.extension(), "service");
    }

    #[test]
    fn default_trait() {
        let backend = SystemdBackend::default();
        assert_eq!(InitBackend::name(&backend), "systemd");
    }

    #[test]
    fn unit_with_all_dependency_types() {
        let mut spec = test_spec();
        spec.depends_on.after = vec!["db".to_owned()];
        spec.depends_on.before = vec!["proxy".to_owned()];
        spec.depends_on.requires = vec!["db".to_owned()];
        spec.depends_on.wants = vec!["cache".to_owned()];
        spec.depends_on.conflicts = vec!["legacy".to_owned()];

        let unit = spec_to_unit(&spec);
        assert!(unit.contains("After=db.service"));
        assert!(unit.contains("Before=proxy.service"));
        assert!(unit.contains("Requires=db.service"));
        assert!(unit.contains("Wants=cache.service"));
        assert!(unit.contains("Conflicts=legacy.service"));
    }

    #[test]
    fn unit_with_partial_resources() {
        let mut spec = test_spec();
        spec.resources = Some(ResourceLimits {
            memory_max: Some(MemorySize::parse("1G").unwrap()),
            memory_high: None,
            cpu_weight: None,
            cpu_quota: None,
            tasks_max: Some(512),
            io_weight: None,
            nice: None,
        });

        let unit = spec_to_unit(&spec);
        assert!(unit.contains("MemoryMax=1G"));
        assert!(unit.contains("TasksMax=512"));
        assert!(!unit.contains("CPUWeight="));
        assert!(!unit.contains("IOWeight="));
        assert!(!unit.contains("Nice="));
    }

    #[test]
    fn parse_systemctl_show_value_with_equals() {
        let output = "Description=My App with = sign\n";
        let props = parse_systemctl_show(output);
        assert_eq!(props.get("Description").unwrap(), "My App with = sign");
    }
}
