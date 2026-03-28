use shihaisha_core::traits::config_translator::ConfigTranslator;
use shihaisha_core::types::service_spec::ServiceSpec;
use shihaisha_core::Result;

/// Systemd `ConfigTranslator` implementation.
///
/// Translates `ServiceSpec` to/from systemd unit file format.
pub struct SystemdTranslator;

impl ConfigTranslator for SystemdTranslator {
    fn translate(&self, spec: &ServiceSpec) -> Result<String> {
        Ok(crate::systemd::spec_to_unit(spec))
    }

    fn parse_native(&self, _content: &str) -> Result<ServiceSpec> {
        Err(shihaisha_core::Error::ConfigError(
            "parsing native systemd units is not yet implemented".to_owned(),
        ))
    }

    fn extension(&self) -> &str {
        "service"
    }

    fn name(&self) -> &str {
        "systemd"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shihaisha_core::*;
    use std::collections::HashMap;

    fn minimal_spec() -> ServiceSpec {
        ServiceSpec {
            name: "test-svc".to_owned(),
            description: "Test".to_owned(),
            command: "/usr/bin/test".to_owned(),
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
    fn translate_produces_unit() {
        let translator = SystemdTranslator;
        let result = translator.translate(&minimal_spec()).expect("translate");
        assert!(result.contains("[Unit]"));
        assert!(result.contains("[Service]"));
        assert!(result.contains("[Install]"));
    }

    #[test]
    fn extension_is_service() {
        let translator = SystemdTranslator;
        assert_eq!(translator.extension(), "service");
    }

    #[test]
    fn name_is_systemd() {
        let translator = SystemdTranslator;
        assert_eq!(translator.name(), "systemd");
    }
}
