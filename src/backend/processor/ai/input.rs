use crate::io::file::{NormalizedStoragePath, StorageRootId};

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

    pub const fn storage_root_id(self) -> StorageRootId {
        match self {
            Self::Originals => StorageRootId::Originals,
            Self::Previews => StorageRootId::Previews,
        }
    }

    pub fn normalized_path(self, stored_path: &str) -> Result<NormalizedStoragePath, String> {
        NormalizedStoragePath::parse(stored_path).map_err(|error| error.to_string())
    }
}
