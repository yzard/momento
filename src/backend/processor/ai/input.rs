use std::path::PathBuf;

use crate::constants::paths;
use crate::utils::path::{resolve_existing_storage_path, resolve_existing_storage_path_sync};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AiInputStorage {
    Originals,
    Previews,
}

impl AiInputStorage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Originals => "originals",
            Self::Previews => "previews",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "originals" => Ok(Self::Originals),
            "previews" => Ok(Self::Previews),
            _ => Err(format!("unsupported AI input storage root: {value}")),
        }
    }

    pub async fn resolve_existing(self, stored_path: &str) -> Result<PathBuf, String> {
        resolve_existing_storage_path(self.root(), stored_path)
            .await
            .map_err(|error| error.to_string())
    }

    pub fn resolve_existing_sync(self, stored_path: &str) -> Result<PathBuf, String> {
        resolve_existing_storage_path_sync(self.root(), stored_path)
            .map_err(|error| error.to_string())
    }

    fn root(self) -> &'static std::path::Path {
        match self {
            Self::Originals => &paths().originals,
            Self::Previews => &paths().previews,
        }
    }
}
