use std::convert::Infallible;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::{fmt, result};

use clap::{Parser, ValueEnum};
use log::LevelFilter;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use toml::to_string_pretty;

use config::{Config, ConfigError, Environment, File as ConfigFile};

const CONFIGURATION_FILE_NAME: &str = "lucid_conf";

#[derive(Debug)]
pub enum ConfigurationError {
    Loading(ConfigError),
    Io(std::io::Error),
    ParseInt(std::num::ParseIntError),
    Json(serde_json::Error),
    Toml(toml::ser::Error),
    Yaml(serde_yaml::Error),
}

impl From<ConfigError> for ConfigurationError {
    fn from(e: ConfigError) -> Self {
        Self::Loading(e)
    }
}

impl From<::std::num::ParseIntError> for ConfigurationError {
    fn from(e: std::num::ParseIntError) -> Self {
        Self::ParseInt(e)
    }
}

impl From<std::io::Error> for ConfigurationError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for ConfigurationError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl From<toml::ser::Error> for ConfigurationError {
    fn from(e: toml::ser::Error) -> Self {
        Self::Toml(e)
    }
}

impl From<serde_yaml::Error> for ConfigurationError {
    fn from(e: serde_yaml::Error) -> Self {
        Self::Yaml(e)
    }
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Loading(e) => e.fmt(f),
            Self::Io(e) => e.fmt(f),
            Self::ParseInt(e) => e.fmt(f),
            Self::Json(e) => e.fmt(f),
            Self::Toml(e) => e.fmt(f),
            Self::Yaml(e) => e.fmt(f),
        }
    }
}

type Result<T> = std::result::Result<T, ConfigurationError>;

impl From<Infallible> for ConfigurationError {
    fn from(_: Infallible) -> Self {
        panic!("Infallible error is not supposed to happen by definition.");
    }
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
#[serde(default)]
pub struct Server {
    pub uri: String,
    pub maxmind_id: String,
    pub maxmind_password: String,
    pub maxmind_path_inclusions: String,
    pub ip_path_inclusions: String,
    pub path_exclusions: Option<String>,
    pub cache_capacity: usize,
    pub cache_duration_secs: u64,
    pub forwarded_ip_header: Option<String>,
    pub use_forwarded_ip_header_only: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct Listener {
    pub port: u16,
}

impl Default for Listener {
    fn default() -> Self {
        Listener { port: 7479 }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct Settings {
    /// The log level.
    ///
    /// It is saved in the configuration file as lowercase.
    #[serde(
        serialize_with = "ser_lowercase",
        deserialize_with = "de_capitalize_first_letter"
    )]
    pub loglevel: LevelFilter,
    pub server: Server,
    pub listener: Listener,
}

/// Serializes a value to a lowercase string.
fn ser_lowercase<S, T>(value: &T, serializer: S) -> result::Result<S::Ok, S::Error>
where
    S: Serializer,
    T: fmt::Display,
{
    let s = value.to_string().to_lowercase();
    serializer.serialize_str(&s)
}

fn de_capitalize_first_letter<'de, D, T>(deserializer: D) -> result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: fmt::Display,
{
    let s = String::deserialize(deserializer)?;
    let mut chars = s.chars();
    let normalized = match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        None => return Err(serde::de::Error::custom("empty string")),
    };
    normalized.parse().map_err(serde::de::Error::custom)
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            loglevel: LevelFilter::Info,
            server: Server {
                cache_capacity: 20480,
                cache_duration_secs: 60 * 24,
                ..Default::default()
            },
            listener: Default::default(),
        }
    }
}

impl Settings {
    pub fn load() -> Result<Self> {
        let cli = Cli::parse();
        let default = Config::try_from(&Settings::default())?;
        let mut conf = Config::builder().add_source(default);

        if let Some(p) = cli.config_file {
            conf = conf.add_source(ConfigFile::from(p).required(true));
        } else {
            conf = conf.add_source(ConfigFile::with_name(CONFIGURATION_FILE_NAME).required(false));
        }

        conf = conf.add_source(
            Environment::with_prefix("prux")
                .prefix_separator("__")
                .separator("__"),
        );

        let mut settings: Settings = conf.build()?.try_deserialize()?;

        if let Some(v) = cli.maxmind_id {
            settings.server.maxmind_id = v;
        }
        if let Some(v) = cli.maxmind_password {
            settings.server.maxmind_password = v;
        }
        if let Some(v) = cli.log_level {
            settings.loglevel = v;
        };
        if let Some(v) = cli.port {
            settings.listener.port = v;
        };
        if let Some(v) = cli.server_uri {
            settings.server.uri = v;
        }
        if let Some(mut path) = cli.save_config {
            path.push(CONFIGURATION_FILE_NAME);

            let s = match cli.format {
                Format::Toml => toml::to_string_pretty(&settings)?,
                Format::Yaml => serde_yaml::to_string(&settings)?,
                Format::Json => serde_json::to_string_pretty(&settings)?,
            };
            path.set_extension(cli.format.to_string().to_lowercase());
            let mut file = File::create(path)?;
            file.write_all(s.as_bytes())?;
        }

        if cli.show_config {
            if let Ok(pretty) = to_string_pretty(&settings) {
                println!("------------------------PRUX CONFIGURATION------------------------\n{pretty}\n---------------------------------------------------------------------");
            }
        }

        Ok(settings)
    }
}

#[derive(Clone, ValueEnum)]
#[clap(rename_all = "UPPERCASE")]
enum Format {
    Toml,
    Yaml,
    Json,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Format::Toml => write!(f, "TOML"),
            Format::Yaml => write!(f, "YAML"),
            Format::Json => write!(f, "JSON"),
        }
    }
}

#[derive(Parser)]
#[command(
    version,
    about,
    author = "Richer Archambault & Seb Aubin - Devolutions"
)]
struct Cli {
    #[arg(short, long = "config", value_name = "CONFIGFILE")]
    config_file: Option<PathBuf>,
    #[arg(short, long = "level", value_name = "LOGLEVEL", value_enum)]
    log_level: Option<LevelFilter>,
    #[arg(short, long, value_name = "LISTENER_PORT")]
    port: Option<u16>,
    #[arg(short = 'u', long = "uri", value_name = "SERVER_URI")]
    server_uri: Option<String>,
    #[arg(short = 'i', long = "maxmindid", value_name = "MAXMIND_ID")]
    maxmind_id: Option<String>,
    #[arg(short = 's', long = "maxmindpass", value_name = "MAXMIND_PASSWORD")]
    maxmind_password: Option<String>,
    #[arg(long)]
    save_config: Option<PathBuf>,
    #[arg(long, default_value_t = Format::Toml)]
    format: Format,
    #[arg(long)]
    show_config: bool,
}
