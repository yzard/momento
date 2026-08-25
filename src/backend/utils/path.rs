use std::path::{Component, Path, PathBuf};

use crate::error::{AppError, AppResult};

pub fn resolve_storage_path(root: &Path, stored_path: &str) -> AppResult<PathBuf> {
    let relative = Path::new(stored_path);
    if stored_path.is_empty()
        || stored_path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(AppError::NotFound(
            "Stored file path is invalid".to_string(),
        ));
    }

    Ok(root.join(relative))
}

pub async fn resolve_existing_storage_path(root: &Path, stored_path: &str) -> AppResult<PathBuf> {
    let candidate = resolve_storage_path(root, stored_path)?;
    let canonical_root = tokio::fs::canonicalize(root)
        .await
        .map_err(|_| AppError::NotFound("Storage directory not found".to_string()))?;
    let canonical_candidate = tokio::fs::canonicalize(candidate)
        .await
        .map_err(|_| AppError::NotFound("Stored file not found".to_string()))?;
    if !canonical_candidate.starts_with(canonical_root) {
        return Err(AppError::NotFound(
            "Stored file path is invalid".to_string(),
        ));
    }

    Ok(canonical_candidate)
}

pub fn resolve_existing_storage_path_sync(root: &Path, stored_path: &str) -> AppResult<PathBuf> {
    let candidate = resolve_storage_path(root, stored_path)?;
    let canonical_root = std::fs::canonicalize(root)
        .map_err(|_| AppError::NotFound("Storage directory not found".to_string()))?;
    let canonical_candidate = std::fs::canonicalize(candidate)
        .map_err(|_| AppError::NotFound("Stored file not found".to_string()))?;
    if !canonical_candidate.starts_with(canonical_root) {
        return Err(AppError::NotFound(
            "Stored file path is invalid".to_string(),
        ));
    }

    Ok(canonical_candidate)
}
