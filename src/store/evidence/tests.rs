use super::*;
use crate::store::{MIGRATIONS, NewEvent};
use std::time::Duration;

fn event(session: &str) -> NewEvent {
    NewEvent::new(
        "codex",
        session,
        "message",
        "user",
        "2026-01-01T00:00:00Z",
        "synthetic data",
        "fixture:v1",
    )
}

#[test]
fn v2_migration_backfills_unsealed_counters_and_pages_ties_without_loss() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY NOT NULL,name TEXT NOT NULL,applied_at TEXT NOT NULL,checksum TEXT NOT NULL);").unwrap();
    for migration in &MIGRATIONS[..2] {
        conn.execute_batch(migration.sql).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations VALUES(?1,?2,'fixture',?3)",
            params![
                migration.version,
                migration.name,
                content_hash(migration.sql)
            ],
        )
        .unwrap();
    }
    let mut old = Store {
        conn,
        db_path: path.clone(),
    };
    let project = old.ensure_project_link(dir.path()).unwrap();
    let a = old.insert_event(&project.id, &event("old-a")).unwrap();
    let b = old.insert_event(&project.id, &event("old-b")).unwrap();
    old.conn
        .execute("UPDATE sessions SET ended_at='2025-01-01T00:00:00Z'", [])
        .unwrap();
    drop(old);
    let mut migrated = Store::open(&path).unwrap();
    assert_eq!(migrated.schema_version().unwrap(), 3);
    let tiny = EvidenceBudget {
        max_sessions: 1,
        ..EvidenceBudget::default()
    };
    let first = migrated
        .evidence_changes(&project.id, None, None, tiny)
        .unwrap();
    let second = migrated
        .evidence_changes(&project.id, first.next.as_ref(), Some(&first.through), tiny)
        .unwrap();
    assert_eq!(first.changes.len(), 1);
    assert_eq!(second.changes.len(), 1);
    assert!(second.next.is_none());
    assert_eq!(first.changes[0].change_sequence, 1);
    assert_eq!(second.changes[0].change_sequence, 1);
    assert!(!first.changes[0].completed && !second.changes[0].completed);
    assert_eq!(
        BTreeSet::from([
            first.changes[0].session_id.clone(),
            second.changes[0].session_id.clone()
        ]),
        BTreeSet::from([a.session_id.clone(), b.session_id])
    );
    assert_eq!(first.changes[0].source_rows, 1);
    assert!(first.changes[0].source_bytes > 0);
    assert!(
        migrated
            .create_evidence_snapshot(
                &project.id,
                std::slice::from_ref(&a.session_id),
                "fixture:v1",
                EvidenceBudget::default()
            )
            .is_err()
    );
    let before = migrated.evidence_high_watermark().unwrap();
    drop(migrated);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.evidence_high_watermark().unwrap(), before);
    reopened
        .conn
        .execute(
            "UPDATE schema_migrations SET checksum='invalid' WHERE version=3",
            [],
        )
        .unwrap();
    drop(reopened);
    assert!(
        Store::open(&path)
            .unwrap_err()
            .to_string()
            .contains("checksum drift")
    );
}

#[test]
fn immediate_validation_excludes_concurrent_writer_and_rollback_retains_seal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("concurrent.sqlite3");
    let mut store = Store::open(&path).unwrap();
    let project = store.ensure_project_link(dir.path()).unwrap();
    let event = store
        .insert_event(&project.id, &event("concurrent"))
        .unwrap();
    store
        .attest_session_completion(
            &event.session_id,
            "trusted fixture",
            EvidenceBudget::default(),
        )
        .unwrap();
    let snapshot = store
        .create_evidence_snapshot(
            &project.id,
            std::slice::from_ref(&event.session_id),
            "fixture:v1",
            EvidenceBudget::default(),
        )
        .unwrap();
    let mut other = Store::open(&path).unwrap();
    other.conn.busy_timeout(Duration::ZERO).unwrap();
    let tx = store
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    validate_snapshot_on_tx(&tx, &snapshot.id, EvidenceBudget::default()).unwrap();
    let error = other
        .conn
        .execute(
            "UPDATE events SET role='assistant' WHERE id=?1",
            [&event.id],
        )
        .unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    // Simulate a process leaving an uncommitted mutation; SQLite rollback restores
    // both raw semantics and the trigger-updated revision/seal atomically.
    tx.execute("UPDATE events SET role='tool' WHERE id=?1", [&event.id])
        .unwrap();
    drop(tx);
    drop(store);
    let mut reopened = Store::open(&path).unwrap();
    assert!(
        reopened
            .read_current_evidence_snapshot(&snapshot.id, EvidenceBudget::default())
            .is_ok()
    );
    other
        .conn
        .execute(
            "UPDATE events SET role='assistant' WHERE id=?1",
            [&event.id],
        )
        .unwrap();
    assert!(
        other
            .read_current_evidence_snapshot(&snapshot.id, EvidenceBudget::default())
            .is_err()
    );
    assert!(
        reopened
            .commit_snapshot_proposals(
                &snapshot.id,
                serde_json::from_str("{\"observations\":[]}").unwrap(),
                EvidenceBudget::default()
            )
            .is_err()
    );
    assert_eq!(
        reopened
            .conn
            .query_row("SELECT count(*) FROM evidence_proposal_receipts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn bounded_snapshot_metadata_deserialization_rejects_collection_amplification() {
    let refs = std::iter::repeat_n(
        "{\"event_id\":\"e\",\"semantic_hash\":\"h\"}",
        MAX_EVENTS + 1,
    )
    .collect::<Vec<_>>()
    .join(",");
    let body = format!(
        "{{\"seal\":{{\"session_id\":\"s\",\"revision\":1,\"control_sequence\":1,\"digest\":\"d\",\"attestation\":\"a\",\"completed_at\":\"t\"}},\"metadata\":{{\"id\":\"s\",\"project_id\":\"p\",\"source\":\"codex\",\"source_session_id\":\"s\",\"started_at\":\"t\",\"ended_at\":null,\"title\":null,\"raw_local_ref\":null,\"adapter_version\":\"v1\",\"source_enabled\":true}},\"source_rows\":1,\"events\":[{refs}]}}"
    );
    assert!(body.len() < MAX_BYTES);
    let error = serde_json::from_str::<SnapshotSession>(&body).unwrap_err();
    assert!(error.to_string().contains("count bound"));
}

#[test]
fn change_pages_seek_into_composite_index_including_large_sequence_ties() {
    let store = Store::open_in_memory().unwrap();
    let sql = format!(
        "EXPLAIN QUERY PLAN SELECT session_id FROM evidence_sessions WHERE {} ORDER BY changed_seq,session_id LIMIT ?5",
        change_predicate(true)
    );
    let plan: String = store
        .conn
        .query_row(
            &sql,
            params!["fixture-project", 200, 100, "last-session", 2],
            |r| r.get(3),
        )
        .unwrap();
    assert!(plan.contains("idx_evidence_session_changes"), "{plan}");
    assert!(plan.contains("(changed_seq,session_id)>(?,?)"), "{plan}");
}

#[test]
fn store_connections_enable_required_foreign_key_and_recursive_delete_triggers() {
    let store = Store::open_in_memory().unwrap();
    for pragma in ["foreign_keys", "recursive_triggers"] {
        assert_eq!(
            store
                .conn
                .query_row(&format!("PRAGMA {pragma}"), [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
