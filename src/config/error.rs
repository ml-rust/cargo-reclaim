use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum ConfigError {
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse(toml::de::Error),
    UnsupportedVersion(u16),
    InvalidDuration(String),
    InvalidSize(String),
    InvalidPercentage(String),
    InvalidBackgroundMode(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "failed to read config {}: {source}",
                    path.display()
                )
            }
            Self::Parse(error) => write!(formatter, "failed to parse config: {error}"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported config version {version}")
            }
            Self::InvalidDuration(value) => write!(formatter, "invalid config duration `{value}`"),
            Self::InvalidSize(value) => write!(formatter, "invalid config size `{value}`"),
            Self::InvalidPercentage(value) => {
                write!(formatter, "invalid config percentage `{value}`")
            }
            Self::InvalidBackgroundMode(value) => {
                write!(formatter, "invalid background mode `{value}`")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<toml::de::Error> for ConfigError {
    fn from(error: toml::de::Error) -> Self {
        Self::Parse(error)
    }
}
