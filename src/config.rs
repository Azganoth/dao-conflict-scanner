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
    /// Load the configuration from file or return the default configuration.
    pub fn load() -> Self {
        match Self::load_saved() {
            Ok(config) => config,
            Err(err) => {
                eprintln!("Warning: Could not load saved config. Using default. Details: {err}");
                Self::default()
            }
        }
    }

    /// Save the current configuration to disk.
    pub fn save(&self) -> AnyhowResult<()> {
        let config_path = Self::config_file_path()?;
        let contents =
            toml::to_string_pretty(self).context("Failed to serialize AppConfig to TOML")?;

        if let Some(parent_dir) = config_path.parent() {
            fs::create_dir_all(parent_dir).context("Failed to create config directory")?;
        }

        fs::write(&config_path, contents).context("Failed to write config file")?;

        Ok(())
    }

    /// Internal: Attempt to load the configuration from disk.
    fn load_saved() -> AnyhowResult<Self> {
        let config_path = Self::config_file_path()?;
        if !config_path.exists() {
            return Err(anyhow!("Configuration file not found"));
        }

        let contents = fs::read_to_string(&config_path).context("Failed to read config file")?;

        toml::from_str(&contents).context("Failed to parse TOML config")
    }

    /// Internal: Get the path to the configuration file.
    fn config_file_path() -> AnyhowResult<PathBuf> {
        ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .map(|proj_dirs| proj_dirs.config_dir().join("config.toml"))
            .ok_or_else(|| anyhow!("Could not determine configuration directory for the app"))
    }
}
