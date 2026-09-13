use momento_api::database::queries::file_operations;
use rusqlite::OptionalExtension;

#[test]
fn sidecar_recovery_waits_for_live_import_but_startup_recovers_stale_claims() {
    let pool = crate::test_utils::create_test_db();
    let connection = pool.get().unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    connection.execute(
        "INSERT INTO import_content_hash_claims(content_hash, claim_token, import_source) VALUES (?, ?, 'mobile_backup')",
        rusqlite::params!["a".repeat(64), token],
    ).unwrap();
    connection.execute(
        "INSERT INTO file_operation_groups(id,kind,owner_kind,owner_id,claim_token,state,entry_count) VALUES ('sidecar', 'import_sidecar_publication', 'import', 'sidecar', ?, 'publishing', 1)",
        [&token],
    ).unwrap();
    for state in ["publishing", "files_committed"] {
        connection
            .execute("UPDATE file_operation_groups SET state = ?", [state])
            .unwrap();
        for query in [
            file_operations::SELECT_NEXT_GENERIC_RECOVERY_GROUP,
            file_operations::SELECT_NEXT_BLOCKING_RECOVERY_GROUP,
        ] {
            let selected = connection
                .query_row(query, ["[]"], |row| row.get::<_, String>(0))
                .optional()
                .unwrap();
            assert_eq!(selected, None, "live import must own {state}");
        }
        let delay: Option<i64> = connection
            .query_row(file_operations::NEXT_RECOVERY_DELAY, [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            delay, None,
            "live imports must not cause a recovery busy loop"
        );
        let startup: String = connection
            .query_row(
                file_operations::SELECT_NEXT_STARTUP_CRITICAL_RECOVERY_GROUP,
                ["[]"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(startup, "sidecar");
    }
    connection
        .execute("DELETE FROM import_content_hash_claims", [])
        .unwrap();
    let recovered: String = connection
        .query_row(
            file_operations::SELECT_NEXT_GENERIC_RECOVERY_GROUP,
            ["[]"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(recovered, "sidecar");
    connection
        .execute("UPDATE file_operation_groups SET state = 'publishing'", [])
        .unwrap();
    let authorized = connection
        .query_row(
            file_operations::VERIFY_PUBLICATION,
            rusqlite::params!["sidecar", 1],
            |_| Ok(()),
        )
        .optional()
        .unwrap();
    assert_eq!(
        authorized,
        Some(()),
        "orphaned sidecars must remain recoverable"
    );
    assert_eq!(
        connection
            .execute(
                file_operations::CHECKPOINT_PUBLICATION,
                rusqlite::params!["files_committed", "sidecar", 1]
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .execute(
                file_operations::COMPLETE_PUBLICATION,
                rusqlite::params!["sidecar", 2]
            )
            .unwrap(),
        1
    );
}
