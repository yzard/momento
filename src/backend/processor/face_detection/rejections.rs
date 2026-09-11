use crate::config::FaceGroupConfig;
use crate::database::queries::{face_rejections as sql, faces};
use crate::models::{FaceDetectionResponse, RejectFacesRequest};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeSet, HashSet};

pub fn visible_faces(
    connection: &Connection,
    group: i64,
    user: i64,
) -> rusqlite::Result<Vec<FaceDetectionResponse>> {
    let rows = connection
        .prepare(sql::VISIBLE_FACES)?
        .query_map(params![group, user], |row| {
            Ok(FaceDetectionResponse {
                face_id: row.get(0)?,
                media_id: row.get(1)?,
                input_sequence: row.get(2)?,
                x: row.get(3)?,
                y: row.get(4)?,
                width: row.get(5)?,
                height: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.len() > 4096 {
        return Err(rusqlite::Error::InvalidParameterName(
            "too many selected faces".into(),
        ));
    }
    Ok(rows)
}

pub fn reject(
    connection: &Connection,
    user: i64,
    request: RejectFacesRequest,
    config: &FaceGroupConfig,
) -> rusqlite::Result<Option<usize>> {
    let selection = serde_json::to_string(&request)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let tx = connection.unchecked_transaction()?;
    if let Some((owner, previous, count)) = tx
        .query_row(sql::PREVIOUS_OPERATION, [&request.request_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, usize>(2)?,
            ))
        })
        .optional()?
    {
        return Ok((owner == user && previous == selection).then_some(count));
    }
    let mut selected = BTreeSet::new();
    let groups = if let Some(group) = request.face_group_id {
        vec![group]
    } else {
        request.group_ids.clone()
    };
    let explicit: HashSet<i64> = request.face_ids.iter().copied().collect();
    for group in groups {
        let visible = visible_faces(&tx, group, user)?;
        if visible.is_empty() {
            return Ok(None);
        }
        for face in visible {
            if request.face_group_id.is_none() || explicit.contains(&face.face_id) {
                selected.insert(face.face_id);
            }
        }
        if selected.len() > 4096 {
            return Ok(None);
        }
    }
    if selected.is_empty() || (request.face_group_id.is_some() && selected.len() != explicit.len())
    {
        return Ok(None);
    }
    let mut affected = BTreeSet::new();
    for face in &selected {
        affected.extend(
            tx.prepare(sql::AFFECTED_GROUPS)?
                .query_map([face], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        );
        tx.execute(sql::RECORD, params![face, user])?;
        tx.execute(sql::DELETE_FACE, [face])?;
    }
    let mut crop_paths = BTreeSet::new();
    for face in &selected {
        if let Some(path) = tx
            .query_row(sql::ORPHAN_CROP, [face], |row| row.get::<_, String>(0))
            .optional()?
        {
            crop_paths.insert(path);
        }
    }
    let crop_paths = crop_paths.into_iter().collect::<Vec<_>>();
    for (batch, paths) in crop_paths
        .chunks(crate::io::journal::MAX_FILE_OPERATION_ENTRIES_PER_GROUP)
        .enumerate()
    {
        use crate::io::file::{
            NormalizedStoragePath, PathClaimMode, PathClaimScope, StorageRootId,
        };
        use crate::io::journal::{
            FileEntryAction, FileEntryPlan, FileOperationPlan, FilePathClaimPlan,
            PrepareJournalOutcome,
        };
        let paths = paths
            .iter()
            .map(|path| {
                NormalizedStoragePath::parse(path).map_err(|_| rusqlite::Error::InvalidQuery)
            })
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let plan = FileOperationPlan {
            group_id: format!("face-reject-{}-{batch}", request.request_id),
            kind: "face_rejection_cleanup".into(),
            owner_kind: "face_rejection".into(),
            owner_id: request.request_id.clone(),
            claim_token: None,
            product_target: None,
            product_version: None,
            space_reservation: None,
            entries: paths
                .iter()
                .map(|path| FileEntryPlan {
                    action: FileEntryAction::Cleanup,
                    storage_root: StorageRootId::Previews,
                    source_path: Some(path.clone()),
                    temporary_path: None,
                    destination_path: None,
                    tombstone_path: None,
                    expected_size: None,
                    expected_sha256: None,
                    expected_version: None,
                })
                .collect(),
            claims: paths
                .into_iter()
                .map(|path| FilePathClaimPlan {
                    storage_root: StorageRootId::Previews,
                    path,
                    mode: PathClaimMode::Write,
                    scope: PathClaimScope::Exact,
                    role: "rejected_face_crop".into(),
                    expected_version: None,
                })
                .collect(),
        };
        if crate::io::journal::prepare_committed_cleanup(&tx, plan)?
            == PrepareJournalOutcome::PathConflict
        {
            return Ok(None);
        }
    }
    // Existing finalization checks restart on this revision; FK cascades remove stale snapshots.
    tx.execute(faces::INCREMENT_MANUAL_REVISION, [])?;
    for group in affected {
        super::update_group_representative(&tx, group, config)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    }
    tx.execute(
        sql::SAVE_OPERATION,
        params![request.request_id, user, selection, selected.len()],
    )?;
    tx.commit()?;
    Ok(Some(selected.len()))
}
