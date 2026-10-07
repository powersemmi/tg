use std::env;
use std::fmt;
use std::fs;
use std::path::PathBuf;

pub struct Config {
    pub api_id: i32,
    pub api_hash: String,
}

pub enum ConfigError {
    /// No credentials found anywhere (config file or env vars).
    Missing(String),
    /// Config file exists but cannot be read or parsed.
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Missing(msg) | ConfigError::Invalid(msg) => write!(f, "{msg}"),
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .expect("cannot determine home directory")
        .join(".config")
        .join("tgcli")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn session_path() -> PathBuf {
    config_dir().join("session.db")
}

pub fn load_config() -> Result<Config, ConfigError> {
    let path = config_path();
    let mut api_id: Option<String> = None;
    let mut api_hash: Option<String> = None;

    if path.exists() {
        let text = fs::read_to_string(&path)
            .map_err(|e| ConfigError::Invalid(format!("cannot read {}: {e}", path.display())))?;
        (api_id, api_hash) = parse_config_text(&text).map_err(|e| {
            ConfigError::Invalid(format!("invalid TOML in {}: {e}", path.display()))
        })?;
    }

    if api_id.is_none() {
        api_id = env::var("TELEGRAM_API_ID").ok().filter(|s| !s.is_empty());
    }
    if api_hash.is_none() {
        api_hash = env::var("TELEGRAM_API_HASH").ok().filter(|s| !s.is_empty());
    }

    let (Some(api_id), Some(api_hash)) = (api_id, api_hash) else {
        return Err(ConfigError::Missing(format!(
            "Telegram API credentials not found.\nSet them in {} or via \
             TELEGRAM_API_ID / TELEGRAM_API_HASH env vars.",
            path.display()
        )));
    };

    let api_id: i32 = api_id.trim().parse().map_err(|_| {
        ConfigError::Invalid(format!("api_id must be an integer, got \"{api_id}\""))
    })?;

    Ok(Config { api_id, api_hash })
}

/// Extract api_id and api_hash (both as strings) from config TOML text.
fn parse_config_text(text: &str) -> Result<(Option<String>, Option<String>), String> {
    let value: toml::Table = text.parse().map_err(|e| format!("{e}"))?;
    let api_id = match value.get("api_id") {
        None => None,
        Some(toml::Value::Integer(n)) => Some(n.to_string()),
        Some(toml::Value::String(s)) => Some(s.clone()),
        Some(_) => return Err("api_id must be an integer".into()),
    };
    let api_hash = match value.get("api_hash") {
        None => None,
        Some(toml::Value::String(s)) => Some(s.clone()),
        Some(other) => Some(other.to_string()),
    };
    Ok((api_id, api_hash))
}

#[cfg(test)]
mod tests {
    use super::parse_config_text;

    #[test]
    fn parses_full_document() {
        let (id, hash) = parse_config_text(
            "api_id = 23417584\napi_hash = \"abc123\"\nsession_store = \"file\"\n",
        )
        .unwrap();
        assert_eq!(id.as_deref(), Some("23417584"));
        assert_eq!(hash.as_deref(), Some("abc123"));
    }

    #[test]
    fn missing_keys_are_none() {
        assert_eq!(parse_config_text("").unwrap(), (None, None));
    }

    #[test]
    fn rejects_non_integer_api_id() {
        assert!(parse_config_text("api_id = [1]\n").is_err());
    }
}

pub fn write_config(api_id: i32, api_hash: &str) -> std::io::Result<PathBuf> {
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    restrict_permissions(&dir, 0o700)?;
    let path = config_path();
    fs::write(
        &path,
        format!("api_id = {api_id}\napi_hash = \"{api_hash}\"\n"),
    )?;
    restrict_permissions(&path, 0o600)?;
    Ok(path)
}

#[cfg(unix)]
pub fn restrict_permissions(path: &std::path::Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
pub fn restrict_permissions(_path: &std::path::Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}
