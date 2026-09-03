use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono_tz::Tz;
use sha2::{Digest, Sha256};

const ZONEINFO_ROOT: &str = "/usr/share/zoneinfo";
const LOCALTIME_PATH: &str = "/etc/localtime";
const MAX_ZONEINFO_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SystemTimezoneSnapshot {
    name: String,
    timezone: Tz,
    device: u64,
    inode: u64,
    byte_size: u64,
    sha256: [u8; 32],
}

impl SystemTimezoneSnapshot {
    pub(crate) fn load() -> Result<Self, String> {
        let localtime = Path::new(LOCALTIME_PATH);
        let target = std::fs::read_link(localtime)
            .map_err(|error| format!("failed to read {LOCALTIME_PATH}: {error}"))?;
        let canonical_target = if target.is_absolute() {
            target
        } else {
            localtime
                .parent()
                .unwrap_or_else(|| Path::new("/"))
                .join(target)
        };
        let canonical_target = canonical_target
            .canonicalize()
            .map_err(|error| format!("failed to resolve {LOCALTIME_PATH}: {error}"))?;
        let zoneinfo_root = PathBuf::from(ZONEINFO_ROOT);
        let relative = canonical_target
            .strip_prefix(&zoneinfo_root)
            .map_err(|_| format!("{LOCALTIME_PATH} does not identify a compiled IANA timezone"))?;
        let name = relative
            .to_str()
            .filter(|name| !name.is_empty() && !name.contains(".."))
            .ok_or_else(|| format!("{LOCALTIME_PATH} has an invalid IANA timezone name"))?
            .to_string();
        let timezone =
            Tz::from_str(&name).map_err(|_| format!("unsupported system IANA timezone {name}"))?;
        let mut file = std::fs::File::open(&canonical_target)
            .map_err(|error| format!("failed to open system timezone {name}: {error}"))?;
        let before = file
            .metadata()
            .map_err(|error| format!("failed to inspect system timezone {name}: {error}"))?;
        if !before.is_file() || before.len() == 0 || before.len() > MAX_ZONEINFO_BYTES {
            return Err(format!(
                "system timezone {name} must be a non-empty regular file no larger than {MAX_ZONEINFO_BYTES} bytes"
            ));
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(before.len() as usize)
            .map_err(|error| format!("failed to reserve system timezone buffer: {error}"))?;
        file.by_ref()
            .take(MAX_ZONEINFO_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("failed to read system timezone {name}: {error}"))?;
        let after = file
            .metadata()
            .map_err(|error| format!("failed to recheck system timezone {name}: {error}"))?;
        let identity = |metadata: &std::fs::Metadata| {
            (
                metadata.dev(),
                metadata.ino(),
                metadata.len(),
                metadata.mtime(),
                metadata.mtime_nsec(),
            )
        };
        if bytes.len() as u64 != before.len() || identity(&before) != identity(&after) {
            return Err(format!("system timezone {name} changed while loading"));
        }
        Ok(Self {
            name,
            timezone,
            device: after.dev(),
            inode: after.ino(),
            byte_size: after.len(),
            sha256: Sha256::digest(bytes).into(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn timezone(&self) -> Tz {
        self.timezone
    }

    pub fn identity(&self) -> (u64, u64, u64, [u8; 32]) {
        (self.device, self.inode, self.byte_size, self.sha256)
    }
}
