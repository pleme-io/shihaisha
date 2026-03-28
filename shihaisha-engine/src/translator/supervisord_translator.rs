use shihaisha_core::traits::config_translator::ConfigTranslator;
use shihaisha_core::types::service_spec::ServiceSpec;
use shihaisha_core::Result;

/// Supervisord `ConfigTranslator` implementation.
///
/// Translates `ServiceSpec` to supervisord INI `[program:name]` config format.
pub struct SupervisordTranslator;

impl ConfigTranslator for SupervisordTranslator {
    fn translate(&self, spec: &ServiceSpec) -> Result<String> {
        Ok(crate::supervisord::spec_to_conf(spec))
    }

    fn parse_native(&self, _content: &str) -> Result<ServiceSpec> {
        Err(shihaisha_core::Error::ConfigError(
            "parsing native supervisord configs is not yet implemented".to_owned(),
        ))
    }

    fn extension(&self) -> &str {
        "conf"
    }

    fn name(&self) -> &str {
        "supervisord"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shihaisha_core::ServiceSpec;

    #[test]
    fn translate_produces_program_section() {
        let spec = ServiceSpec::new("test-svc", "/usr/bin/test");
        let translator = SupervisordTranslator;
        let result = translator.translate(&spec).expect("translate");
        assert!(result.contains("[program:test-svc]"));
        assert!(result.contains("command=/usr/bin/test"));
        assert!(result.contains("autostart=true"));
    }

    #[test]
    fn extension_is_conf() {
        let translator = SupervisordTranslator;
        assert_eq!(translator.extension(), "conf");
    }

    #[test]
    fn name_is_supervisord() {
        let translator = SupervisordTranslator;
        assert_eq!(translator.name(), "supervisord");
    }

    #[test]
    fn parse_native_returns_error() {
        let translator = SupervisordTranslator;
        let result = translator.parse_native("[program:test]\ncommand=/bin/true");
        assert!(result.is_err());
    }
}
