use std::net::IpAddr;
use std::str::FromStr;
use std::{fs, path::PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

#[derive(clap::ValueEnum, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeType {
    Aero,
    AquaClassic,
    Blue,
    Classic,
    Dark,
    Greybird,
    HighContrast,
    Metro,
}

const THEME_LIST: [ThemeType; 8] = [
    ThemeType::Aero,
    ThemeType::AquaClassic,
    ThemeType::Blue,
    ThemeType::Classic,
    ThemeType::Dark,
    ThemeType::Greybird,
    ThemeType::HighContrast,
    ThemeType::Metro,
];

impl Default for ThemeType {
    fn default() -> Self {
        Self::Greybird
    }
}

impl ThemeType {
    pub fn apply(&self) {
        let theme = match self {
            ThemeType::Classic => fltk_theme::ThemeType::Classic,
            ThemeType::Aero => fltk_theme::ThemeType::Aero,
            ThemeType::Metro => fltk_theme::ThemeType::Metro,
            ThemeType::AquaClassic => fltk_theme::ThemeType::AquaClassic,
            ThemeType::Greybird => fltk_theme::ThemeType::Greybird,
            ThemeType::Blue => fltk_theme::ThemeType::Blue,
            ThemeType::Dark => fltk_theme::ThemeType::Dark,
            ThemeType::HighContrast => fltk_theme::ThemeType::HighContrast,
        };
        let theme = fltk_theme::WidgetTheme::new(theme);
        theme.apply();
    }

    pub fn name(&self) -> String {
        format!("{self:?}")
    }

    pub fn to_index(&self) -> i32 {
        THEME_LIST.iter().position(|th| th == self).unwrap() as i32
    }

    pub fn from_index(i: i32) -> Self {
        let i = i.clamp(0, THEME_LIST.len() as i32 - 1) as usize;
        THEME_LIST[i]
    }

    pub fn themes() -> &'static [ThemeType] {
        &THEME_LIST
    }
}

#[derive(Serialize, Deserialize, Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
pub struct Config {
    #[arg(long, help = "Access code")]
    pub access_code: Option<String>,
    #[arg(long, default_value = "0.0.0.0", help = "Bind address")]
    pub bind_address: IpAddr,
    #[arg(long, default_value = "1701", help = "Web port")]
    pub web_port: u16,
    #[cfg(target_os = "linux")]
    #[arg(
        long,
        help = "Try to use hardware acceleration through the Video Acceleration API."
    )]
    pub try_vaapi: bool,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[arg(long, help = "Try to use Nvidia's NVENC to encode the video via GPU.")]
    #[serde(default)]
    pub try_nvenc: bool,
    #[cfg(target_os = "macos")]
    #[arg(
        long,
        help = "Try to use hardware acceleration through the VideoToolbox API."
    )]
    #[serde(default)]
    pub try_videotoolbox: bool,
    #[cfg(target_os = "windows")]
    #[arg(
        long,
        help = "Try to use hardware acceleration through the MediaFoundation API."
    )]
    #[serde(default)]
    pub try_mediafoundation: bool,
    #[arg(long, help = "Start Weylus server immediately on program start.")]
    #[serde(default)]
    pub auto_start: bool,
    #[arg(long, help = "Gui Theme")]
    pub gui_theme: Option<ThemeType>,
    #[arg(long, help = "Run Weylus without gui and start immediately.")]
    #[serde(default)]
    pub no_gui: bool,
    #[cfg(target_os = "linux")]
    #[arg(long, help = "Wayland/PipeWire Support.")]
    #[serde(default)]
    pub wayland_support: bool,

    #[arg(long, help = "Print template of index.html served by Weylus.")]
    #[serde(skip)]
    pub print_index_html: bool,
    #[arg(long, help = "Print access.html served by Weylus.")]
    #[serde(skip)]
    pub print_access_html: bool,
    #[arg(long, help = "Print style.css served by Weylus.")]
    #[serde(skip)]
    pub print_style_css: bool,
    #[arg(long, help = "Print lib.js served by Weylus.")]
    #[serde(skip)]
    pub print_lib_js: bool,

    #[arg(
        long,
        help = "Use custom template of index.html to be served by Weylus."
    )]
    #[serde(skip)]
    pub custom_index_html: Option<PathBuf>,
    #[arg(long, help = "Use custom access.html to be served by Weylus.")]
    #[serde(skip)]
    pub custom_access_html: Option<PathBuf>,
    #[arg(long, help = "Use custom style.css to be served by Weylus.")]
    #[serde(skip)]
    pub custom_style_css: Option<PathBuf>,
    #[arg(long, help = "Use custom lib.js to be served by Weylus.")]
    #[serde(skip)]
    pub custom_lib_js: Option<PathBuf>,

    #[arg(long, help = "Print shell completions for given shell.")]
    #[serde(skip)]
    pub completions: Option<clap_complete::Shell>,
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
                match err.kind() {
                    std::io::ErrorKind::NotFound => {
                        debug!("Failed to read configuration file: {}", err)
                    }
                    _ => warn!("Failed to read configuration file: {}", err),
                }
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
    let args = std::env::args();
    if let Some(mut config) = read_config() {
        if args.len() > 1 {
            config.update_from(args);
        }
        config
    } else {
        Config::parse()
    }
}
