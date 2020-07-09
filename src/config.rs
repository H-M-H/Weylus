use std::fs;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use tracing::warn;

#[derive(Serialize, Deserialize, StructOpt, Debug)]
#[structopt(name = "weylus")]
pub struct Config {
    #[structopt(long)]
    pub password: Option<String>,
    #[structopt(long, default_value = "0.0.0.0")]
    pub bind_address: IpAddr,
    #[structopt(long, default_value = "1701")]
    pub web_port: u16,
    #[structopt(long, default_value = "9001")]
    pub websocket_port: u16,
    #[cfg(target_os = "linux")]
    #[structopt(long)]
    pub try_vaapi: bool,
    #[cfg(target_os = "linux")]
    #[structopt(long)]
    pub try_nvenc: bool,
}

pub fn read_config() -> Option<Config> {
    if let Some(mut config_path) = dirs::config_dir() {
        config_path.push("weylus");
        config_path.push("weylus.toml");
        match fs::read_to_string(&config_path) {
            Ok(s) => {
                let config: Result<Config, _> = toml::from_str(&s);
                if let Err(err) = config {
                    warn!("Failed to read configuration file: {}", err);
                    return None;
                }
                Some(config.unwrap())
            }
            Err(err) => {
                warn!("Failed to read configuration file: {}", err);
                return None;
            }
        }
    } else {
        None
    }
}

pub fn write_config(conf: &Config) {
    match dirs::config_dir() {
        Some(mut config_path) => {
            config_path.push("weylus");
            if !config_path.exists() {
                if let Err(err) = fs::create_dir_all(&config_path) {
                    warn!("Failed create directory for configuration: {}", err);
                    return;
                }
            }
            config_path.push("weylus.toml");
            if let Err(err) = fs::write(
                config_path,
                &toml::to_string_pretty(&conf).expect("Failed to encode config to toml."),
            ) {
                warn!("Failed to write configuration file: {}", err);
            }
        }
        None => {
            warn!("Failed to find configuration directory!");
        }
    }
}

pub fn get_config() -> Config {
    if std::env::args().len() == 1 {
        read_config().unwrap_or_else(|| crate::config::Config::from_args())
    } else {
        crate::config::Config::from_args()
    }
}
