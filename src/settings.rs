#![allow(dead_code)]

use clap::{crate_name, crate_version, Arg, ArgAction, Command};
use config::{Config, ConfigError, Environment, File as ConfigFile};
use log::LevelFilter;
use std::convert::Infallible;
use std::fs::File;
use std::io::Write;

const CONFIGURATION_FILE_NAME: &str = "lucid_conf";

#[derive(Debug)]
pub enum ConfigurationError {
    Help,
    Version,
    Loading(ConfigError),
    Io(::std::io::Error),
    ParseInt(::std::num::ParseIntError),
}

impl From<ConfigError> for ConfigurationError {
    fn from(e: ConfigError) -> Self {
        ConfigurationError::Loading(e)
    }
}

impl From<::std::num::ParseIntError> for ConfigurationError {
    fn from(e: ::std::num::ParseIntError) -> Self {
        ConfigurationError::ParseInt(e)
    }
}

impl From<::std::io::Error> for ConfigurationError {
    fn from(e: ::std::io::Error) -> Self {
        ConfigurationError::Io(e)
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
    pub loglevel: String,
    pub server: Server,
    pub listener: Listener,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            loglevel: "info".to_string(),
            server: Server {
                uri: "".to_string(),
                maxmind_id: "".to_string(),
                maxmind_password: "".to_string(),
                maxmind_path_inclusions: "".to_string(),
                ip_path_inclusions: "".to_string(),
                path_exclusions: None,
                cache_capacity: 20480,
                cache_duration_secs: 60 * 24,
                forwarded_ip_header: None,
                use_forwarded_ip_header_only: false,
            },
            listener: Default::default(),
        }
    }
}

impl Settings {
    pub fn level_filter(&self) -> LevelFilter {
        match self.loglevel.to_lowercase().as_str() {
            "off" => LevelFilter::Off,
            "error" => LevelFilter::Error,
            "warn" => LevelFilter::Warn,
            "info" => LevelFilter::Info,
            "debug" => LevelFilter::Debug,
            "trace" => LevelFilter::Trace,
            _ => LevelFilter::Off,
        }
    }

    pub fn load() -> Result<Self> {
        use std::path::Path;
        let cli_app = create_command_line_app();
        let matches = cli_app.get_matches();
        let default = Config::try_from(&Settings::default())?;
        let mut conf = Config::builder().add_source(default);

        if let Some(path) = matches.get_one::<String>("config-file") {
            let p = Path::new(&path);
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

        // Apply command line arg

        if let Some(id) = matches.get_one::<String>("maxmind-id") {
            settings.server.maxmind_id = id.to_string();
        }

        if let Some(pass) = matches.get_one::<String>("maxmind-password") {
            settings.server.maxmind_password = pass.to_string();
        }

        if let Some(level) = matches.get_one::<String>("log-level") {
            settings.loglevel = level.to_string();
        };

        if let Some(port) = matches.get_one::<String>("port") {
            settings.listener.port = port.parse()?;
        };

        if let Some(server_uri) = matches.get_one::<String>("server-uri") {
            settings.server.uri = server_uri.to_string();
        }

        if let Some(config_path) = matches.get_one::<String>("save-config") {
            let mut file_path = Path::new(config_path).to_owned();

            file_path.push(CONFIGURATION_FILE_NAME);

            match matches.get_one::<String>("format").unwrap().as_str() {
                "TOML" => {
                    if let Ok(pretty) = toml::to_string_pretty(&settings) {
                        file_path.set_extension("toml");
                        let mut file = File::create(file_path)?;
                        file.write_all(pretty.as_bytes())?;
                    }
                }
                "YAML" => {
                    if let Ok(pretty) = serde_yaml::to_string(&settings) {
                        file_path.set_extension("yaml");
                        let mut file = File::create(file_path)?;
                        file.write_all(pretty.as_bytes())?;
                    }
                }
                "JSON" => {
                    if let Ok(pretty) = serde_json::to_string_pretty(&settings) {
                        file_path.set_extension("json");
                        let mut file = File::create(file_path)?;
                        file.write_all(pretty.as_bytes())?;
                    }
                }
                wrong => {
                    println!("Specified configuration format is invalid {}", wrong);
                    ::std::process::exit(1);
                }
            }
        }

        if matches.contains_id("show-config") {
            use toml::to_string_pretty;

            if let Ok(pretty) = to_string_pretty(&settings) {
                println!("------------------------PRUX CONFIGURATION------------------------\n{}\n---------------------------------------------------------------------", pretty);
            }
        }

        Ok(settings)
    }
}

fn create_command_line_app() -> Command {
    Command::new(crate_name!())
        .author("Richer Archambault & Seb Aubin - Devolutions")
        .version(concat!(crate_version!(), "\n"))
        .about("A simple identity server")
        .disable_version_flag(true)
        .arg(
            Arg::new("version")
                .long("version")
                .short('v')
                .help("Print version information")
                .action(ArgAction::Version)

        )
        .arg(Arg::new("config-file")
            .short('c')
            .long("config")
            .value_name("CONFIGFILE")
            .help("Path of a custom configuration file")
            .action(ArgAction::SetTrue)
            .value_parser(clap::builder::NonEmptyStringValueParser::new())
        )
        .arg(Arg::new("log-level")
            .short('l')
            .long("level")
            .value_name("LOGLEVEL")
            .help("Verbosity level of the logger")
            .action(ArgAction::SetTrue)
            .value_parser(["off", "error", "warn", "info", "debug", "trace"])
            .value_parser(clap::builder::NonEmptyStringValueParser::new())
        )
        .arg(Arg::new("port")
            .short('p')
            .long("port")
            .value_name("LISTENER_PORT")
            .help("Port used by the router on the default interface. Overrides -u <URL>")
            .action(ArgAction::SetTrue)
            .value_parser(clap::builder::NonEmptyStringValueParser::new())
        )
        .arg(Arg::new("server-uri")
            .short('u')
            .long("uri")
            .value_name("SERVER_URI")
            .help("Uri of the server behind the proxy")
            .action(ArgAction::SetTrue)
            .value_parser(clap::builder::NonEmptyStringValueParser::new())
        )
        .arg(Arg::new("maxmind-id")
            .short('i')
            .long("maxmindid")
            .value_name("MAXMIND_ID")
            .help("Maxmind ID")
            .action(ArgAction::SetTrue)
            .value_parser(clap::builder::NonEmptyStringValueParser::new())
        )
        .arg(Arg::new("maxmind-password")
            .short('s')
            .long("maxmindpass")
            .value_name("MAXMIND_PASSWORD")
            .help("Maxmind password")
            .action(ArgAction::SetTrue)
            .value_parser(clap::builder::NonEmptyStringValueParser::new())
        )
        .arg(Arg::new("save-config")
            .long("save-config")
            .value_name("PATH")
            .help("Save the current config at the specified directory (default file format is TOML, see `format` for more)")
            .action(ArgAction::SetTrue)
            .value_parser(clap::builder::NonEmptyStringValueParser::new())
        )
        .arg(Arg::new("format")
            .long("format")
            .value_name("FORMAT")
            .help("Use with --save-config: Specifies which format will be used to save configurations")
            .value_parser(["TOML", "YAML", "JSON"])
            .default_value("TOML")
            .action(ArgAction::SetTrue)
            .value_parser(clap::builder::NonEmptyStringValueParser::new())
        )
        .arg(Arg::new("show-config")
            .long("show-config")
            .help("Show the current config before startup")
            .action(ArgAction::SetFalse)
        )
}
