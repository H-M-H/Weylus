use std::fs;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use tracing::warn;

#[derive(Serialize, Deserialize, StructOpt, Debug, Clone)]
#[structopt(name = "weylus")]
pub struct Config {
    #[structopt(long, help = "Access code")]
    pub access_code: Option<String>,
    #[structopt(long, default_value = "0.0.0.0", help = "Bind address")]
    pub bind_address: IpAddr,
    #[structopt(long, default_value = "1701", help = "Web port")]
    pub web_port: u16,
    #[structopt(long, default_value = "9001", help = "Websocket port")]
    pub websocket_port: u16,
    #[cfg(target_os = "linux")]
    #[structopt(
        long,
        help = "Try to use hardware acceleration through the Video Acceleration API."
    )]
    pub try_vaapi: bool,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[structopt(long, help = "Try to use Nvidia's NVENC to encode the video via GPU.")]
    #[serde(default)]
    pub try_nvenc: bool,
    #[cfg(target_os = "macos")]
    #[structopt(
        long,
        help = "Try to use hardware acceleration through the VideoToolbox API."
    )]
    #[serde(default)]
    pub try_videotoolbox: bool,
    #[cfg(target_os = "windows")]
    #[structopt(
        long,
        help = "Try to use hardware acceleration through the MediaFoundation API."
    )]
    #[serde(default)]
    pub try_mediafoundation: bool,
    #[structopt(long, help = "Start Weylus server immediately on program start.")]
    #[serde(default)]
    pub auto_start: bool,
    #[structopt(long, help = "Run Weylus without gui and start immediately.")]
    #[serde(default)]
    pub no_gui: bool,
    #[cfg(target_os = "linux")]
    #[structopt(long, help = "Wayland/PipeWire Support.")]
    #[serde(default)]
    pub wayland_support: bool,

    #[structopt(long, help = "Print template of index.html served by Weylus.")]
    #[serde(skip)]
    pub print_index_html: bool,
    #[structopt(long, help = "Print access.html served by Weylus.")]
    #[serde(skip)]
    pub print_access_html: bool,
    #[structopt(long, help = "Print style.css served by Weylus.")]
    #[serde(skip)]
    pub print_style_css: bool,
    #[structopt(long, help = "Print lib.js served by Weylus.")]
    #[serde(skip)]
    pub print_lib_js: bool,

    pub virtual_keys_profiles: Option<String>,

    #[structopt(
        long,
        help = "Use custom template of index.html to be served by Weylus."
    )]
    #[serde(skip)]
    pub custom_index_html: Option<String>,
    #[structopt(long, help = "Use custom access.html to be served by Weylus.")]
    #[serde(skip)]
    pub custom_access_html: Option<String>,
    #[structopt(long, help = "Use custom style.css to be served by Weylus.")]
    #[serde(skip)]
    pub custom_style_css: Option<String>,
    #[structopt(long, help = "Use custom lib.js to be served by Weylus.")]
    #[serde(skip)]
    pub custom_lib_js: Option<String>,

    #[structopt(long, help = "Print shell completions for given shell.")]
    #[serde(skip)]
    pub completions: Option<structopt::clap::Shell>,
}

pub fn read_config() -> Option<Config> {
    if let Some(mut config_path) = dirs::config_dir() {
        config_path.push("weylus");
        config_path.push("weylus.toml");
        match fs::read_to_string(&config_path) {
            Ok(s) => match toml::from_str(&s) {
                Ok(c) => Some(c),
                Err(e) => {
                    warn!("Failed to read configuration file: {}", e);
                    None
                }
            },
            Err(err) => {
                warn!("Failed to read configuration file: {}", err);
                None
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
    // TODO: once https://github.com/clap-rs/clap/issues/748 is resolved use
    // the configfile to provide default values that override hardcoded defaults

    // read config from file if no args are specified
    if std::env::args().len() == 1 {
        // (ab)use parsing an empty args array to provide a default config
        read_config().unwrap_or_else(crate::config::Config::from_args)
    } else {
        crate::config::Config::from_args()
    }
}
