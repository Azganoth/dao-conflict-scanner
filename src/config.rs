#![allow(dead_code)]
use std::{collections::HashMap, fs, path::PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const QUALIFIER: &str = "com";
const ORGANIZATION: &str = "Azlands";
const APPLICATION: &str = "DAO-Conflict-Scanner";

#[derive(Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub ignored: HashMap<String, Vec<PathBuf>>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ignored: HashMap::new(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        Self::load_from_path().unwrap_or_default()
    }

    fn load_from_path() -> Result<Self> {
        let config_path = Self::config_path().context("Failed to determine config directory")?;

        let contents = fs::read_to_string(&config_path).context("Failed to read config file")?;

        toml::from_str(&contents).context("Failed to parse config file")
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path().context("Failed to determine config directory")?;

        let contents = toml::to_string_pretty(self).context("Failed to serialize config")?;

        fs::create_dir_all(config_path.parent().unwrap())?;
        fs::write(&config_path, contents).context("Failed to write config file")?;

        Ok(())
    }

    fn config_path() -> Option<PathBuf> {
        ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .map(|proj_dirs| proj_dirs.config_dir().join("config.toml"))
    }
}
