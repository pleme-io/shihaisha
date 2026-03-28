use shihaisha_core::traits::config_translator::ConfigTranslator;
use shihaisha_core::types::service_spec::ServiceSpec;
use shihaisha_core::Result;

/// Launchd `ConfigTranslator` implementation.
///
/// Translates `ServiceSpec` to/from launchd plist format.
pub struct LaunchdTranslator;

impl ConfigTranslator for LaunchdTranslator {
    fn translate(&self, spec: &ServiceSpec) -> Result<String> {
        let dict = crate::launchd::spec_to_plist(spec);
        let value = plist::Value::Dictionary(dict);
        let mut buf = Vec::new();
        value.to_writer_xml(&mut buf).map_err(|e| {
            shihaisha_core::Error::Serialization(format!("plist serialization failed: {e}"))
        })?;
        String::from_utf8(buf).map_err(|e| {
            shihaisha_core::Error::Serialization(format!("plist UTF-8 conversion failed: {e}"))
        })
    }

    fn parse_native(&self, _content: &str) -> Result<ServiceSpec> {
        Err(shihaisha_core::Error::ConfigError(
            "parsing native launchd plists is not yet implemented".to_owned(),
        ))
    }

    fn extension(&self) -> &str {
        "plist"
    }

    fn name(&self) -> &str {
        "launchd"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shihaisha_core::*;
    use std::collections::HashMap;

    fn minimal_spec() -> ServiceSpec {
        ServiceSpec {
            name: "com.test.myapp".to_owned(),
            description: "Test".to_owned(),
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
    fn translate_produces_xml() {
        let translator = LaunchdTranslator;
        let result = translator.translate(&minimal_spec()).expect("translate");
        assert!(result.contains("com.test.myapp"));
        assert!(result.contains("ProgramArguments"));
    }

    #[test]
    fn extension_is_plist() {
        let translator = LaunchdTranslator;
        assert_eq!(translator.extension(), "plist");
    }

    #[test]
    fn name_is_launchd() {
        let translator = LaunchdTranslator;
        assert_eq!(translator.name(), "launchd");
    }
}
