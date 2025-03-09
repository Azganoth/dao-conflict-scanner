#![allow(dead_code)]
use std::{collections::HashMap, fs, path::PathBuf};

use directories::{ProjectDirs, UserDirs};
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

fn default_override_dir() -> Option<PathBuf> {
    Some(
        UserDirs::new()?
            .document_dir()?
            .join("BioWare/Dragon Age/packages/core/override"),
    )
}

impl AppConfig {
    pub fn load() -> Self {
        let config_path = match get_config_path() {
            Some(path) => path,
            None => return Self::default(),
        };

        match fs::read_to_string(&config_path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(config) => config,
                Err(_) => Self::default(),
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = match get_config_path() {
            Some(path) => path,
            None => return Err("Could not determine config path".into()),
        };

        let contents = toml::to_string_pretty(self)?;
        fs::create_dir_all(config_path.parent().unwrap())?;
        fs::write(&config_path, contents)?;
        Ok(())
    }
}

fn get_config_path() -> Option<PathBuf> {
    ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .map(|proj_dirs| proj_dirs.config_dir().join("config.toml"))
}
