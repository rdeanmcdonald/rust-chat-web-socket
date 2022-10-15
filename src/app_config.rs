use config::{Config, Environment, File};
use std::env;

const CONFIG_FILE_PREFIX: &str = "config";

pub fn get_config() -> Config {
    let env = env::var("APP_ENV").unwrap_or_else(|_| "dev".into());
    let config = Config::builder()
        // Start off by merging in the "default" configuration file
        .add_source(File::with_name(&format!(
            "{}/{}.toml",
            CONFIG_FILE_PREFIX, "default"
        )))
        // Add in the current environment file
        .add_source(
            File::with_name(&format!("{}/{}.toml", CONFIG_FILE_PREFIX, env)).required(false),
        )
        // Add in settings from the environment (with a prefix of APP)
        // Eg.. `APP_KAFKA_HOST=example.com`
        .add_source(Environment::with_prefix("APP").separator("_"))
        .build()
        // panic on error
        .unwrap();

    tracing::info!("{:?}", config);
    config
}
