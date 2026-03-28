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
    use shihaisha_core::ServiceSpec;

    #[test]
    fn translate_produces_unit() {
        let spec = ServiceSpec::new("test-svc", "/usr/bin/test");
        let translator = SystemdTranslator;
        let result = translator.translate(&spec).expect("translate");
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
