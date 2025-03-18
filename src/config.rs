use std::{fs, path::PathBuf};

use anyhow::{Context, Result as AnyhowResult, anyhow};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::scanner::Conflicts;

const QUALIFIER: &str = "com";
const ORGANIZATION: &str = "Azlands";
const APPLICATION: &str = "DAO-Conflict-Scanner";

#[derive(Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub ignored: Conflicts,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ignored: Conflicts::new(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        Self::load_saved().unwrap_or_else(|err| {
            eprintln!("Warning: Could not load saved config. Using default. Details: {err}");
            Self::default()
        })
    }

    pub fn save(&self) -> AnyhowResult<()> {
        let config_path = Self::config_file_path()?;

        if let Some(parent_dir) = config_path.parent() {
            fs::create_dir_all(parent_dir).context("Failed to create config directory")?;
        }

        let contents =
            toml::to_string_pretty(self).context("Failed to serialize AppConfig to TOML")?;

        fs::write(&config_path, contents).context("Failed to write config file")?;

        Ok(())
    }

    fn load_saved() -> AnyhowResult<Self> {
        let config_path = Self::config_file_path()?;
        if !config_path.exists() {
            return Err(anyhow!("Configuration file not found"));
        }

        let contents = fs::read_to_string(&config_path).context("Failed to read config file")?;
        toml::from_str(&contents).context("Failed to parse TOML config")
    }

    fn config_file_path() -> AnyhowResult<PathBuf> {
        ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .map(|proj_dirs| proj_dirs.config_dir().join("config.toml"))
            .ok_or_else(|| anyhow!("Could not determine configuration directory for the app"))
    }
}
