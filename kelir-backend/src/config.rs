/// Application configuration.
///
/// Phase 1 replaces the defaults with `KELIR_*` environment loading (database
/// URL, JWT secret, storage driver, SMTP host, frontend URL) and fails fast on
/// missing required values.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_name: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            app_name: String::from("Kelir"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_the_framework_name() {
        assert_eq!(AppConfig::default().app_name, "Kelir");
    }
}
