use async_trait::async_trait;
use plist::Dictionary;
use shihaisha_core::traits::init_backend::InitBackend;
#[allow(unused_imports)]
use shihaisha_core::{
    BackendOverrides, DependencySpec, Error, HealthState, LogTarget, LoggingSpec, ResourceLimits,
    RestartPolicy, RestartStrategy, Result, ServiceSpec, ServiceState, ServiceStatus, ServiceType,
};
#[allow(unused_imports)]
use std::collections::HashMap;
use std::path::PathBuf;

/// launchd backend using `launchctl` CLI and the `plist` crate for XML generation.
///
/// Operates in user (GUI) domain by default. System-level requires root.
pub struct LaunchdBackend {
    user_mode: bool,
    plist_dir: PathBuf,
    uid: u32,
}

impl LaunchdBackend {
    /// Create a new launchd backend, auto-detecting user vs system mode.
    #[must_use]
    pub fn new() -> Self {
        let user_mode = !is_root_macos();
        let uid = current_uid();
        let plist_dir = if user_mode {
            home_dir().join("Library/LaunchAgents")
        } else {
            PathBuf::from("/Library/LaunchDaemons")
        };
        Self {
            user_mode,
            plist_dir,
            uid,
        }
    }

    fn domain_target(&self) -> String {
        if self.user_mode {
            format!("gui/{}", self.uid)
        } else {
            "system".to_owned()
        }
    }

    fn service_target(&self, label: &str) -> String {
        format!("{}/{label}", self.domain_target())
    }

    fn plist_path(&self, name: &str) -> PathBuf {
        self.plist_dir.join(format!("{name}.plist"))
    }

    async fn launchctl(&self, args: &[&str]) -> Result<String> {
        let mut cmd = tokio::process::Command::new("launchctl");
        for arg in args {
            cmd.arg(arg);
        }
        let output = cmd
            .output()
            .await
            .map_err(|e| Error::BackendError(format!("launchctl failed: {e}")))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // launchctl sometimes returns non-zero for benign reasons
            Err(Error::BackendError(format!("launchctl error: {stderr}")))
        }
    }
}

impl Default for LaunchdBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a launchd plist Dictionary from a `ServiceSpec`.
#[must_use]
pub fn spec_to_plist(spec: &ServiceSpec) -> Dictionary {
    let mut dict = Dictionary::new();

    // Label (required)
    dict.insert("Label".to_owned(), plist::Value::String(spec.name.clone()));

    // Program + arguments
    let mut program_args = vec![plist::Value::String(spec.command.clone())];
    for arg in &spec.args {
        program_args.push(plist::Value::String(arg.clone()));
    }
    dict.insert(
        "ProgramArguments".to_owned(),
        plist::Value::Array(program_args),
    );

    // Working directory
    if let Some(ref wd) = spec.working_directory {
        dict.insert(
            "WorkingDirectory".to_owned(),
            plist::Value::String(wd.display().to_string()),
        );
    }

    // User / Group
    if let Some(ref user) = spec.user {
        dict.insert("UserName".to_owned(), plist::Value::String(user.clone()));
    }
    if let Some(ref group) = spec.group {
        dict.insert("GroupName".to_owned(), plist::Value::String(group.clone()));
    }

    // Environment
    if !spec.environment.is_empty() {
        let mut env_dict = Dictionary::new();
        for (k, v) in &spec.environment {
            env_dict.insert(k.clone(), plist::Value::String(v.clone()));
        }
        dict.insert(
            "EnvironmentVariables".to_owned(),
            plist::Value::Dictionary(env_dict),
        );
    }

    // KeepAlive (maps from restart strategy)
    apply_keepalive(&mut dict, &spec.restart, spec.service_type);

    // RunAtLoad for simple/notify services
    if matches!(
        spec.service_type,
        ServiceType::Simple | ServiceType::Notify | ServiceType::Forking
    ) {
        dict.insert("RunAtLoad".to_owned(), plist::Value::Boolean(true));
    }

    // Logging
    apply_logging(&mut dict, &spec.logging, &spec.name);

    // Resource limits
    if let Some(ref res) = spec.resources {
        apply_resource_limits(&mut dict, res);
    }

    // Timeouts
    dict.insert(
        "TimeOut".to_owned(),
        plist::Value::Integer(spec.timeout_stop_sec.into()),
    );

    // Throttle interval (restart delay)
    if spec.restart.delay_secs > 0 {
        dict.insert(
            "ThrottleInterval".to_owned(),
            plist::Value::Integer(spec.restart.delay_secs.into()),
        );
    }

    // Apply raw launchd overrides
    for (k, v) in &spec.overrides.launchd {
        if let Some(pval) = json_value_to_plist(v) {
            dict.insert(k.clone(), pval);
        }
    }

    dict
}

/// Apply `KeepAlive` logic based on restart policy and service type.
fn apply_keepalive(dict: &mut Dictionary, restart: &RestartPolicy, svc_type: ServiceType) {
    match restart.strategy {
        RestartStrategy::Always => {
            dict.insert("KeepAlive".to_owned(), plist::Value::Boolean(true));
        }
        RestartStrategy::OnFailure => {
            let mut keepalive = Dictionary::new();
            keepalive.insert("SuccessfulExit".to_owned(), plist::Value::Boolean(false));
            dict.insert(
                "KeepAlive".to_owned(),
                plist::Value::Dictionary(keepalive),
            );
        }
        RestartStrategy::OnSuccess => {
            // launchd doesn't natively support "restart on success" — use KeepAlive
            // with Crashed=false to approximate (restart only when exit 0).
            let mut keepalive = Dictionary::new();
            keepalive.insert("Crashed".to_owned(), plist::Value::Boolean(false));
            dict.insert(
                "KeepAlive".to_owned(),
                plist::Value::Dictionary(keepalive),
            );
        }
        RestartStrategy::Never => {
            if matches!(svc_type, ServiceType::Oneshot) {
                dict.insert("KeepAlive".to_owned(), plist::Value::Boolean(false));
            }
        }
    }
}

/// Apply logging targets to the plist.
fn apply_logging(dict: &mut Dictionary, logging: &LoggingSpec, name: &str) {
    let default_log_dir = home_dir().join("Library/Logs");

    match &logging.stdout {
        LogTarget::Journal => {
            // macOS uses os_log by default — no explicit config needed.
        }
        LogTarget::File(path) => {
            dict.insert(
                "StandardOutPath".to_owned(),
                plist::Value::String(path.display().to_string()),
            );
        }
        LogTarget::Null => {
            dict.insert(
                "StandardOutPath".to_owned(),
                plist::Value::String("/dev/null".to_owned()),
            );
        }
        LogTarget::Inherit => {
            // Default launchd behavior is to capture; inherit not directly supported.
            // Use a log file as a reasonable fallback.
            dict.insert(
                "StandardOutPath".to_owned(),
                plist::Value::String(
                    default_log_dir
                        .join(format!("{name}.stdout.log"))
                        .display()
                        .to_string(),
                ),
            );
        }
    }

    match &logging.stderr {
        LogTarget::Journal => {}
        LogTarget::File(path) => {
            dict.insert(
                "StandardErrorPath".to_owned(),
                plist::Value::String(path.display().to_string()),
            );
        }
        LogTarget::Null => {
            dict.insert(
                "StandardErrorPath".to_owned(),
                plist::Value::String("/dev/null".to_owned()),
            );
        }
        LogTarget::Inherit => {
            dict.insert(
                "StandardErrorPath".to_owned(),
                plist::Value::String(
                    default_log_dir
                        .join(format!("{name}.stderr.log"))
                        .display()
                        .to_string(),
                ),
            );
        }
    }
}

/// Apply resource limits to the plist.
fn apply_resource_limits(dict: &mut Dictionary, res: &ResourceLimits) {
    // Nice value maps directly
    if let Some(nice) = res.nice {
        dict.insert("Nice".to_owned(), plist::Value::Integer(nice.into()));
    }

    // ProcessType based on CPU weight heuristic
    if let Some(weight) = res.cpu_weight {
        let process_type = if weight <= 100 {
            "Background"
        } else if weight >= 5000 {
            "Interactive"
        } else {
            "Standard"
        };
        dict.insert(
            "ProcessType".to_owned(),
            plist::Value::String(process_type.to_owned()),
        );
    }

    // Soft/Hard resource limits
    let mut soft_limits = Dictionary::new();
    let mut hard_limits = Dictionary::new();

    if let Some(ref mem_max) = res.memory_max {
        if let Some(bytes) = parse_memory_string(mem_max) {
            hard_limits.insert(
                "MemoryLock".to_owned(),
                plist::Value::Integer(bytes.into()),
            );
        }
    }

    if let Some(ref mem_high) = res.memory_high {
        if let Some(bytes) = parse_memory_string(mem_high) {
            soft_limits.insert(
                "MemoryLock".to_owned(),
                plist::Value::Integer(bytes.into()),
            );
        }
    }

    if let Some(tasks_max) = res.tasks_max {
        hard_limits.insert(
            "NumberOfProcesses".to_owned(),
            plist::Value::Integer(tasks_max.into()),
        );
        soft_limits.insert(
            "NumberOfProcesses".to_owned(),
            plist::Value::Integer(tasks_max.into()),
        );
    }

    if !soft_limits.is_empty() {
        dict.insert(
            "SoftResourceLimits".to_owned(),
            plist::Value::Dictionary(soft_limits),
        );
    }
    if !hard_limits.is_empty() {
        dict.insert(
            "HardResourceLimits".to_owned(),
            plist::Value::Dictionary(hard_limits),
        );
    }
}

/// Parse memory strings like "512M", "1G", "1024K" to bytes.
fn parse_memory_string(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, multiplier) = if let Some(n) = s.strip_suffix('G') {
        (n, 1024 * 1024 * 1024_i64)
    } else if let Some(n) = s.strip_suffix('M') {
        (n, 1024 * 1024_i64)
    } else if let Some(n) = s.strip_suffix('K') {
        (n, 1024_i64)
    } else {
        (s, 1_i64)
    };

    num_str.trim().parse::<i64>().ok().map(|n| n * multiplier)
}

/// Convert a `serde_json::Value` to a `plist::Value`.
fn json_value_to_plist(v: &serde_json::Value) -> Option<plist::Value> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(b) => Some(plist::Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(plist::Value::Integer(i.into()))
            } else if let Some(f) = n.as_f64() {
                Some(plist::Value::Real(f))
            } else {
                None
            }
        }
        serde_json::Value::String(s) => Some(plist::Value::String(s.clone())),
        serde_json::Value::Array(arr) => {
            let items: Vec<plist::Value> = arr.iter().filter_map(json_value_to_plist).collect();
            Some(plist::Value::Array(items))
        }
        serde_json::Value::Object(obj) => {
            let mut dict = Dictionary::new();
            for (k, v) in obj {
                if let Some(pv) = json_value_to_plist(v) {
                    dict.insert(k.clone(), pv);
                }
            }
            Some(plist::Value::Dictionary(dict))
        }
    }
}

/// Render a plist Dictionary to XML string.
fn dict_to_xml(dict: &Dictionary) -> Result<String> {
    let value = plist::Value::Dictionary(dict.clone());
    let mut buf = Vec::new();
    value
        .to_writer_xml(&mut buf)
        .map_err(|e| Error::BackendError(format!("plist serialization failed: {e}")))?;
    String::from_utf8(buf)
        .map_err(|e| Error::BackendError(format!("plist UTF-8 conversion failed: {e}")))
}

/// Parse `launchctl print` output to extract service state.
fn parse_launchctl_print(output: &str) -> (ServiceState, Option<u32>, Option<i32>) {
    let mut state = ServiceState::Unknown;
    let mut pid = None;
    let mut exit_code = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("state = ") {
            state = match rest.trim() {
                "running" => ServiceState::Running,
                "waiting" => ServiceState::Inactive,
                "not running" => ServiceState::Stopped,
                _ => ServiceState::Unknown,
            };
        } else if let Some(rest) = trimmed.strip_prefix("pid = ") {
            pid = rest.trim().parse::<u32>().ok().filter(|&p| p > 0);
        } else if let Some(rest) = trimmed.strip_prefix("last exit code = ") {
            exit_code = rest.trim().parse::<i32>().ok();
        }
    }

    (state, pid, exit_code)
}

#[async_trait]
impl InitBackend for LaunchdBackend {
    async fn install(&self, spec: &ServiceSpec) -> Result<()> {
        let dict = spec_to_plist(spec);
        let xml = dict_to_xml(&dict)?;
        let path = self.plist_path(&spec.name);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| Error::BackendError(format!("failed to create plist dir: {e}")))?;
        }

        tokio::fs::write(&path, xml)
            .await
            .map_err(|e| Error::BackendError(format!("failed to write plist: {e}")))?;

        Ok(())
    }

    async fn uninstall(&self, name: &str) -> Result<()> {
        // Try to stop first, ignore errors
        let _ = self.stop(name).await;

        let path = self.plist_path(name);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| Error::BackendError(format!("failed to remove plist: {e}")))?;
        }

        Ok(())
    }

    async fn start(&self, name: &str) -> Result<()> {
        let plist_path = self.plist_path(name);
        let path_str = plist_path.display().to_string();
        self.launchctl(&["bootstrap", &self.domain_target(), &path_str])
            .await?;
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        let target = self.service_target(name);
        self.launchctl(&["bootout", &target]).await?;
        Ok(())
    }

    async fn restart(&self, name: &str) -> Result<()> {
        // launchd doesn't have a native restart; bootout then bootstrap
        let _ = self.stop(name).await;
        self.start(name).await
    }

    async fn reload(&self, name: &str) -> Result<()> {
        // launchd has no SIGHUP-like reload. Send HUP manually if PID is known.
        let status = self.status(name).await?;
        if let Some(pid) = status.pid {
            let output = tokio::process::Command::new("kill")
                .args(["-HUP", &pid.to_string()])
                .output()
                .await
                .map_err(|e| Error::BackendError(format!("kill -HUP failed: {e}")))?;
            if !output.status.success() {
                return Err(Error::BackendError(
                    "failed to send SIGHUP to service".to_owned(),
                ));
            }
            Ok(())
        } else {
            Err(Error::BackendError(format!(
                "service {name} is not running, cannot reload"
            )))
        }
    }

    async fn status(&self, name: &str) -> Result<ServiceStatus> {
        let target = self.service_target(name);
        let output = self.launchctl(&["print", &target]).await;

        match output {
            Ok(text) => {
                let (state, pid, exit_code) = parse_launchctl_print(&text);
                Ok(ServiceStatus {
                    name: name.to_owned(),
                    state,
                    pid,
                    exit_code,
                    started_at: None,
                    uptime_secs: None,
                    restart_count: 0,
                    health: HealthState::Unknown,
                    backend: "launchd".to_owned(),
                    memory_bytes: None,
                    cpu_usage_percent: None,
                })
            }
            Err(_) => {
                // Service not loaded
                Ok(ServiceStatus {
                    name: name.to_owned(),
                    state: ServiceState::Inactive,
                    pid: None,
                    exit_code: None,
                    started_at: None,
                    uptime_secs: None,
                    restart_count: 0,
                    health: HealthState::Unknown,
                    backend: "launchd".to_owned(),
                    memory_bytes: None,
                    cpu_usage_percent: None,
                })
            }
        }
    }

    async fn logs(&self, name: &str, lines: u32) -> Result<Vec<String>> {
        // launchd doesn't have a unified log viewer for stdout/stderr.
        // Read from StandardOutPath if it exists in the plist, otherwise try system log.
        let plist_path = self.plist_path(name);
        if plist_path.exists() {
            let content = tokio::fs::read(&plist_path)
                .await
                .map_err(|e| Error::BackendError(format!("failed to read plist: {e}")))?;
            if let Ok(plist::Value::Dictionary(dict)) = plist::from_bytes::<plist::Value>(&content)
            {
                if let Some(plist::Value::String(path)) = dict.get("StandardOutPath") {
                    let log_path = PathBuf::from(path);
                    if log_path.exists() {
                        let text = tokio::fs::read_to_string(&log_path).await.map_err(|e| {
                            Error::BackendError(format!("failed to read log file: {e}"))
                        })?;
                        let all_lines: Vec<String> =
                            text.lines().map(|l| l.to_owned()).collect();
                        let start = all_lines.len().saturating_sub(lines as usize);
                        return Ok(all_lines[start..].to_vec());
                    }
                }
            }
        }

        // Fallback: try `log show` for the process
        let output = tokio::process::Command::new("log")
            .args([
                "show",
                "--predicate",
                &format!("processImagePath contains '{name}'"),
                "--last",
                "5m",
                "--style",
                "compact",
            ])
            .output()
            .await
            .map_err(|e| Error::BackendError(format!("log show failed: {e}")))?;

        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            let all_lines: Vec<String> = text.lines().map(|l| l.to_owned()).collect();
            let start = all_lines.len().saturating_sub(lines as usize);
            Ok(all_lines[start..].to_vec())
        } else {
            Ok(vec![format!(
                "No logs available for {name} — configure StandardOutPath in the service spec"
            )])
        }
    }

    async fn enable(&self, name: &str) -> Result<()> {
        let target = self.service_target(name);
        self.launchctl(&["enable", &target]).await?;
        Ok(())
    }

    async fn disable(&self, name: &str) -> Result<()> {
        let target = self.service_target(name);
        self.launchctl(&["disable", &target]).await?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<ServiceStatus>> {
        let output = self.launchctl(&["list"]).await?;
        let mut services = Vec::new();

        for line in output.lines().skip(1) {
            // Format: PID\tStatus\tLabel
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let pid = parts[0].trim().parse::<u32>().ok().filter(|&p| p > 0);
                let exit_code = parts[1].trim().parse::<i32>().ok();
                let label = parts[2].trim();

                let state = if pid.is_some() {
                    ServiceState::Running
                } else if exit_code == Some(0) {
                    ServiceState::Stopped
                } else if exit_code.is_some() {
                    ServiceState::Failed
                } else {
                    ServiceState::Inactive
                };

                services.push(ServiceStatus {
                    name: label.to_owned(),
                    state,
                    pid,
                    exit_code,
                    started_at: None,
                    uptime_secs: None,
                    restart_count: 0,
                    health: HealthState::Unknown,
                    backend: "launchd".to_owned(),
                    memory_bytes: None,
                    cpu_usage_percent: None,
                });
            }
        }

        Ok(services)
    }

    async fn daemon_reload(&self) -> Result<()> {
        // launchd has no daemon-reload equivalent.
        // Individual services must be bootout/bootstrapped.
        // This is a no-op for launchd.
        Ok(())
    }

    fn available(&self) -> bool {
        cfg!(target_os = "macos")
            && std::process::Command::new("launchctl")
                .arg("version")
                .output()
                .is_ok_and(|o| o.status.success())
    }

    fn name(&self) -> &str {
        "launchd"
    }
}

/// Check if the current process is running as root on macOS.
fn is_root_macos() -> bool {
    std::env::var("SUDO_USER").is_ok()
        || std::process::Command::new("id")
            .args(["-u"])
            .output()
            .is_ok_and(|o| {
                String::from_utf8_lossy(&o.stdout).trim() == "0"
            })
}

/// Get current user UID.
fn current_uid() -> u32 {
    std::process::Command::new("id")
        .args(["-u"])
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        })
        .unwrap_or(501) // macOS default first user UID
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

    fn minimal_spec() -> ServiceSpec {
        ServiceSpec {
            name: "com.test.myapp".to_owned(),
            description: "Test application".to_owned(),
            command: "/usr/local/bin/myapp".to_owned(),
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
    fn plist_generation_minimal() {
        let spec = minimal_spec();
        let dict = spec_to_plist(&spec);

        assert_eq!(
            dict.get("Label").and_then(|v| v.as_string()),
            Some("com.test.myapp")
        );

        let prog_args = dict
            .get("ProgramArguments")
            .and_then(|v| v.as_array())
            .expect("ProgramArguments should be an array");
        assert_eq!(prog_args.len(), 1);
        assert_eq!(prog_args[0].as_string(), Some("/usr/local/bin/myapp"));

        // Default restart is on-failure, so KeepAlive should be a dict
        let keepalive = dict
            .get("KeepAlive")
            .and_then(|v| v.as_dictionary())
            .expect("KeepAlive should be a dictionary for on-failure");
        assert_eq!(
            keepalive
                .get("SuccessfulExit")
                .and_then(|v| v.as_boolean()),
            Some(false)
        );

        // RunAtLoad for Simple type
        assert_eq!(
            dict.get("RunAtLoad").and_then(|v| v.as_boolean()),
            Some(true)
        );
    }

    #[test]
    fn plist_with_args_and_env() {
        let mut spec = minimal_spec();
        spec.args = vec!["--port".to_owned(), "8080".to_owned()];
        spec.environment
            .insert("RUST_LOG".to_owned(), "debug".to_owned());
        spec.working_directory = Some(PathBuf::from("/var/www"));

        let dict = spec_to_plist(&spec);

        let prog_args = dict
            .get("ProgramArguments")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(prog_args.len(), 3);
        assert_eq!(prog_args[1].as_string(), Some("--port"));
        assert_eq!(prog_args[2].as_string(), Some("8080"));

        let env = dict
            .get("EnvironmentVariables")
            .and_then(|v| v.as_dictionary())
            .unwrap();
        assert_eq!(
            env.get("RUST_LOG").and_then(|v| v.as_string()),
            Some("debug")
        );

        assert_eq!(
            dict.get("WorkingDirectory").and_then(|v| v.as_string()),
            Some("/var/www")
        );
    }

    #[test]
    fn keepalive_mapping_always() {
        let mut spec = minimal_spec();
        spec.restart.strategy = RestartStrategy::Always;

        let dict = spec_to_plist(&spec);

        assert_eq!(
            dict.get("KeepAlive").and_then(|v| v.as_boolean()),
            Some(true)
        );
    }

    #[test]
    fn keepalive_mapping_on_failure() {
        let mut spec = minimal_spec();
        spec.restart.strategy = RestartStrategy::OnFailure;

        let dict = spec_to_plist(&spec);

        let keepalive = dict
            .get("KeepAlive")
            .and_then(|v| v.as_dictionary())
            .expect("on-failure should produce KeepAlive dict");
        assert_eq!(
            keepalive
                .get("SuccessfulExit")
                .and_then(|v| v.as_boolean()),
            Some(false)
        );
    }

    #[test]
    fn keepalive_mapping_never_oneshot() {
        let mut spec = minimal_spec();
        spec.restart.strategy = RestartStrategy::Never;
        spec.service_type = ServiceType::Oneshot;

        let dict = spec_to_plist(&spec);

        assert_eq!(
            dict.get("KeepAlive").and_then(|v| v.as_boolean()),
            Some(false)
        );
    }

    #[test]
    fn resource_limits_mapping() {
        let mut spec = minimal_spec();
        spec.resources = Some(ResourceLimits {
            memory_max: Some("1G".to_owned()),
            memory_high: Some("768M".to_owned()),
            cpu_weight: Some(100),
            cpu_quota: None,
            tasks_max: Some(512),
            io_weight: None,
            nice: Some(-5),
        });

        let dict = spec_to_plist(&spec);

        // Nice
        assert_eq!(
            dict.get("Nice").and_then(|v| v.as_signed_integer()),
            Some(-5)
        );

        // ProcessType from cpu_weight <= 100
        assert_eq!(
            dict.get("ProcessType").and_then(|v| v.as_string()),
            Some("Background")
        );

        // Hard resource limits
        let hard = dict
            .get("HardResourceLimits")
            .and_then(|v| v.as_dictionary())
            .expect("should have HardResourceLimits");
        assert!(hard.get("MemoryLock").is_some());
        assert!(hard.get("NumberOfProcesses").is_some());

        // Soft resource limits
        let soft = dict
            .get("SoftResourceLimits")
            .and_then(|v| v.as_dictionary())
            .expect("should have SoftResourceLimits");
        assert!(soft.get("MemoryLock").is_some());
        assert!(soft.get("NumberOfProcesses").is_some());
    }

    #[test]
    fn resource_limits_high_cpu_weight() {
        let mut spec = minimal_spec();
        spec.resources = Some(ResourceLimits {
            memory_max: None,
            memory_high: None,
            cpu_weight: Some(5000),
            cpu_quota: None,
            tasks_max: None,
            io_weight: None,
            nice: None,
        });

        let dict = spec_to_plist(&spec);

        assert_eq!(
            dict.get("ProcessType").and_then(|v| v.as_string()),
            Some("Interactive")
        );
    }

    #[test]
    fn plist_xml_roundtrip() {
        let spec = minimal_spec();
        let dict = spec_to_plist(&spec);
        let xml = dict_to_xml(&dict).expect("should serialize to XML");

        assert!(xml.contains("com.test.myapp"));
        assert!(xml.contains("/usr/local/bin/myapp"));
        assert!(xml.contains("Label"));
        assert!(xml.contains("ProgramArguments"));

        // Parse it back
        let parsed: plist::Value =
            plist::from_bytes(xml.as_bytes()).expect("should parse XML back");
        let parsed_dict = parsed.as_dictionary().expect("should be a dictionary");
        assert_eq!(
            parsed_dict.get("Label").and_then(|v| v.as_string()),
            Some("com.test.myapp")
        );
    }

    #[test]
    fn throttle_interval_from_restart_delay() {
        let mut spec = minimal_spec();
        spec.restart.delay_secs = 10;

        let dict = spec_to_plist(&spec);

        assert_eq!(
            dict.get("ThrottleInterval")
                .and_then(|v| v.as_signed_integer()),
            Some(10)
        );
    }

    #[test]
    fn available_check() {
        let backend = LaunchdBackend::new();
        // On macOS this should return true; on other platforms false.
        if cfg!(target_os = "macos") {
            assert!(backend.available(), "launchd should be available on macOS");
        }
    }

    #[test]
    fn parse_memory_string_various() {
        assert_eq!(parse_memory_string("512M"), Some(512 * 1024 * 1024));
        assert_eq!(parse_memory_string("1G"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_memory_string("2048K"), Some(2048 * 1024));
        assert_eq!(parse_memory_string("1024"), Some(1024));
        assert_eq!(parse_memory_string(""), None);
        assert_eq!(parse_memory_string("abc"), None);
    }

    #[test]
    fn json_to_plist_conversion() {
        let json = serde_json::json!({
            "key": "value",
            "num": 42,
            "flag": true,
            "nested": {"inner": "data"}
        });
        let pval = json_value_to_plist(&json).expect("should convert");
        let dict = pval.as_dictionary().expect("should be dict");
        assert_eq!(dict.get("key").and_then(|v| v.as_string()), Some("value"));
        assert_eq!(
            dict.get("num").and_then(|v| v.as_signed_integer()),
            Some(42)
        );
        assert_eq!(dict.get("flag").and_then(|v| v.as_boolean()), Some(true));
    }

    #[test]
    fn launchd_overrides_applied() {
        let mut spec = minimal_spec();
        spec.overrides.launchd.insert(
            "LowPriorityIO".to_owned(),
            serde_json::Value::Bool(true),
        );
        spec.overrides.launchd.insert(
            "ThrottleInterval".to_owned(),
            serde_json::Value::Number(serde_json::Number::from(30)),
        );

        let dict = spec_to_plist(&spec);

        assert_eq!(
            dict.get("LowPriorityIO").and_then(|v| v.as_boolean()),
            Some(true)
        );
        // ThrottleInterval should be overridden by the explicit override
        assert_eq!(
            dict.get("ThrottleInterval")
                .and_then(|v| v.as_signed_integer()),
            Some(30)
        );
    }

    #[test]
    fn parse_launchctl_print_running() {
        let output = r"
com.test.myapp = {
    active count = 1
    state = running
    pid = 54321
    last exit code = 0
}
";
        let (state, pid, exit_code) = parse_launchctl_print(output);
        assert_eq!(state, ServiceState::Running);
        assert_eq!(pid, Some(54321));
        assert_eq!(exit_code, Some(0));
    }

    #[test]
    fn parse_launchctl_print_not_running() {
        let output = r"
com.test.myapp = {
    state = not running
    last exit code = 1
}
";
        let (state, pid, exit_code) = parse_launchctl_print(output);
        assert_eq!(state, ServiceState::Stopped);
        assert!(pid.is_none());
        assert_eq!(exit_code, Some(1));
    }

    #[test]
    fn logging_file_targets_in_plist() {
        let mut spec = minimal_spec();
        spec.logging.stdout = LogTarget::File(PathBuf::from("/var/log/app/out.log"));
        spec.logging.stderr = LogTarget::Null;

        let dict = spec_to_plist(&spec);

        assert_eq!(
            dict.get("StandardOutPath").and_then(|v| v.as_string()),
            Some("/var/log/app/out.log")
        );
        assert_eq!(
            dict.get("StandardErrorPath").and_then(|v| v.as_string()),
            Some("/dev/null")
        );
    }
}
