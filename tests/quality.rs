use coord::core::store::Store;
use rusqlite::{params, Connection};
use serde_json::json;
use std::time::Duration;

fn dir() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("coord-quality-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn pre_lease_schema_migrates_before_indexes() {
    let path = dir().join("old.db");
    let c = Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE tasks(id TEXT PRIMARY KEY,name TEXT NOT NULL,state TEXT NOT NULL,claimed_by TEXT,payload TEXT NOT NULL,result TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE agents(id TEXT PRIMARY KEY,name TEXT NOT NULL,last_seen TEXT NOT NULL);").unwrap();
    drop(c);
    let store = Store::open(&path).expect("upgrade original schema");
    let t = store.create_task("old compatible", json!({})).unwrap();
    assert_eq!(store.get_task(t.id).unwrap().unwrap().name, t.name);
    drop(store);
    Store::open(&path).expect("migration is repeatable");
}

#[test]
fn corrupt_rows_never_silently_become_pending_or_null() {
    for (column, value) in [
        ("state", "unknown"),
        ("payload", "{"),
        ("result", "{"),
        ("claimed_at", ""),
    ] {
        let path = dir().join("bad.db");
        let store = Store::open(&path).unwrap();
        let t = store.create_task("corrupt", json!({})).unwrap();
        Connection::open(path)
            .unwrap()
            .execute(
                &format!("UPDATE tasks SET {column}=?1 WHERE id=?2"),
                params![value, t.id.to_string()],
            )
            .unwrap();
        assert!(
            store.get_task(t.id).is_err(),
            "corrupt {column} must surface"
        );
    }
}

#[test]
fn vault_same_name_and_agent_slug_collisions_are_distinct() {
    let root = dir();
    let vault = root.join("vault");
    let store = Store::open_with_vault(&root.join("db"), Some(vault.clone())).unwrap();
    store.create_task("../../same", json!({})).unwrap();
    store.create_task("../../same", json!({})).unwrap();
    store.heartbeat("a/b", "one").unwrap();
    store.heartbeat("a-b", "two").unwrap();
    assert_eq!(std::fs::read_dir(vault.join("tasks")).unwrap().count(), 2);
    assert_eq!(std::fs::read_dir(vault.join("agents")).unwrap().count(), 2);
}

#[test]
fn vault_event_before_heartbeat_does_not_deadlock() {
    let vault = coord::vault::Vault::open(dir()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        tx.send(vault.append_agent_event("new", "event").is_ok())
            .unwrap();
    });
    assert!(rx
        .recv_timeout(Duration::from_secs(2))
        .expect("event writer deadlocked"));
}

#[test]
fn batch_status_rejects_corrupt_state_instead_of_reporting_terminal() {
    use coord::core::types::{BatchConfig, TaskInput};
    let path = dir().join("corrupt-progress.db");
    let store = Store::open(&path).unwrap();
    let batch = store
        .create_batch("bad status", "", BatchConfig::default())
        .unwrap();
    let id = uuid::Uuid::parse_str(batch["id"].as_str().unwrap()).unwrap();
    store
        .add_tasks(
            id,
            vec![TaskInput {
                key: "one".into(),
                name: "bad".into(),
                payload: json!({}),
            }],
        )
        .unwrap();
    Connection::open(&path)
        .unwrap()
        .execute("UPDATE tasks SET state='garbled'", [])
        .unwrap();
    assert!(
        store.get_batch_status(id).is_err(),
        "corruption must never become an apparently terminal batch"
    );
}

#[test]
fn failed_migration_rolls_back_columns_added_before_failure() {
    let path = dir().join("rollback.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE tasks(id TEXT PRIMARY KEY,name TEXT NOT NULL,state TEXT NOT NULL,claimed_by TEXT,payload TEXT NOT NULL,result TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE VIEW agents AS SELECT 'agent' AS id;").unwrap();
    drop(conn);
    assert!(Store::open(&path).is_err());
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn.prepare("PRAGMA table_info(tasks)").unwrap();
    let columns = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(
        !columns.iter().any(|c| c == "kind"),
        "failed migration must not leave added task columns"
    );
}
