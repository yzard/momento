use chrono::{DateTime, Utc};
use geohash::{encode, Coord};
use std::path::Path;

use crate::config::MediaProcessConfig;
use crate::io::file::{NormalizedStoragePath, StorageRootId};
use crate::processor::metadata::{
    apply_supplemental_metadata, extract_image_metadata, extract_video_metadata,
    load_supplemental_metadata_storage, normalize_gps_coordinates, MediaMetadata, MetadataSource,
    MetadataSourceType,
};
use crate::runtime::ExecutorHandles;

pub fn build_original_filename(media_id: i64, source_path: &Path) -> String {
    let original_stem = source_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let extension = source_path
        .extension()
        .and_then(|extension| extension.to_str());
    match extension {
        Some(extension) => format!("{}_{}.{}", media_id, original_stem, extension),
        None => format!("{}_{}", media_id, original_stem),
    }
}

#[derive(Debug, Clone)]
pub struct CompleteMediaMetadata {
    pub metadata: MediaMetadata,
    pub sources: Vec<MetadataSource>,
}

pub async fn generate_complete_metadata(
    executors: &ExecutorHandles,
    storage_root: StorageRootId,
    storage_path: &NormalizedStoragePath,
    media_type: &str,
    process_config: &MediaProcessConfig,
) -> Result<CompleteMediaMetadata, String> {
    let source_path = Path::new(storage_path.relative_path());
    let mut extracted = if media_type == "image" {
        extract_image_metadata(
            executors,
            source_path,
            storage_root,
            storage_path,
            process_config,
        )
        .await?
    } else {
        extract_video_metadata(
            executors,
            source_path,
            storage_root,
            storage_path,
            process_config,
        )
        .await?
    };

    if let Some(supplemental_metadata) =
        load_supplemental_metadata_storage(executors, storage_root, storage_path).await?
    {
        apply_supplemental_metadata(&mut extracted.metadata, &supplemental_metadata);
        extracted.sources.push(MetadataSource {
            source_type: MetadataSourceType::SupplementalSidecar,
            payload_json: supplemental_metadata.payload_json.clone(),
        });
    }
    let metadata = &mut extracted.metadata;
    normalize_gps_coordinates(metadata);

    if metadata.date_taken.is_none() {
        let (session, snapshot) = executors
            .file_io
            .open_storage_read_session_durable(storage_root, storage_path.clone())
            .await
            .map_err(|error| error.to_string())?;
        executors
            .file_io
            .close_storage_session_durable(session)
            .await
            .map_err(|error| error.to_string())?;
        metadata.date_taken = DateTime::<Utc>::from_timestamp(
            snapshot.modified_seconds,
            snapshot.modified_nanoseconds,
        );
        if metadata.date_taken.is_none() {
            metadata.date_taken = Some(Utc::now());
        }
    }

    Ok(CompleteMediaMetadata {
        metadata: extracted.metadata,
        sources: extracted.sources,
    })
}

pub fn calculate_geohash(lat: f64, lon: f64) -> Option<String> {
    let coord = Coord { x: lon, y: lat };
    encode(coord, 7).ok()
}
