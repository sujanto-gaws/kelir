#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_name: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            app_name: String::from("Bhuvarloka"),
        }
    }
}
