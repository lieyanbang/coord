use coord::core::{
    store::{AckSpec, CompleteOutcome, Store},
    types::TaskState,
};
use serde_json::json;
use std::sync::{Arc, Barrier};

fn path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("coord-fence-{}.db", uuid::Uuid::new_v4()))
}
fn accepted(out: CompleteOutcome) -> bool {
    matches!(out, CompleteOutcome::Completed { .. })
}

#[test]
fn expiry_cannot_revive_and_same_agent_reclaim_has_new_fence() {
    let path = path();
    let a = Store::open(&path).unwrap();
    let b = Store::open(&path).unwrap();
    let t = a.create_task("fence", json!({})).unwrap();
    let first = a.claim_task(t.id, "same", Some(1)).unwrap().unwrap();
    let old = first.lease_token.unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(a
        .extend_lease(t.id, "same", &old, Some(60))
        .unwrap()
        .is_none());
    assert!(!accepted(
        a.complete_task(t.id, "same", &old, json!(1), None).unwrap()
    ));
    b.reclaim_expired_leases().unwrap();
    let second = b.claim_task(t.id, "same", Some(60)).unwrap().unwrap();
    let token = second.lease_token.unwrap();
    assert_ne!(old, token);
    assert_eq!(second.attempt_no, 2);
    assert!(a
        .extend_lease(t.id, "same", &old, Some(60))
        .unwrap()
        .is_none());
    assert!(!accepted(
        a.complete_task(t.id, "same", &old, json!(1), None).unwrap()
    ));
    let renewed = b
        .extend_lease(t.id, "same", &token, Some(1))
        .unwrap()
        .unwrap();
    assert!(
        renewed >= second.lease_until.unwrap(),
        "short renewal cannot shorten lease"
    );
    drop(a);
    drop(b);
    let restarted = Store::open(&path).unwrap();
    assert!(accepted(
        restarted
            .complete_task(t.id, "same", &token, json!(2), None)
            .unwrap()
    ));
}

#[test]
fn duplicate_completion_is_exact_and_ack_is_written_once() {
    let store = Store::open(&path()).unwrap();
    let t = store.create_task("replay", json!({})).unwrap();
    let c = store.claim_task(t.id, "a", None).unwrap().unwrap();
    let token = c.lease_token.unwrap();
    let ack = Some(AckSpec {
        name: "done".into(),
        priority: None,
        payload: None,
    });
    assert!(accepted(
        store
            .complete_task(t.id, "a", &token, json!({"v":1}), ack.clone())
            .unwrap()
    ));
    assert!(accepted(
        store
            .complete_task(t.id, "a", &token, json!({"v":1}), ack)
            .unwrap()
    ));
    assert!(!accepted(
        store
            .complete_task(t.id, "a", &token, json!({"v":2}), None)
            .unwrap()
    ));
    assert!(!accepted(
        store
            .complete_task(t.id, "other", &token, json!({"v":1}), None)
            .unwrap()
    ));
    assert_eq!(
        store
            .list_tasks(50)
            .unwrap()
            .iter()
            .filter(|t| t.kind == "ack")
            .count(),
        1
    );
    store.cancel_task(t.id).unwrap();
    assert_eq!(
        store.get_task(t.id).unwrap().unwrap().state,
        TaskState::Completed
    );
}

#[test]
fn cancellation_and_completion_from_independent_connections_have_one_terminal_winner() {
    for _ in 0..30 {
        let path = path();
        let a = Store::open(&path).unwrap();
        let b = Store::open(&path).unwrap();
        let t = a.create_task("race", json!({})).unwrap();
        let c = a.claim_task(t.id, "a", None).unwrap().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let ready = barrier.clone();
        let worker = std::thread::spawn(move || {
            ready.wait();
            accepted(
                a.complete_task(t.id, "a", c.lease_token.as_deref().unwrap(), json!(1), None)
                    .unwrap(),
            )
        });
        barrier.wait();
        b.cancel_task(t.id).unwrap();
        let won = worker.join().unwrap();
        let after = b.get_task(t.id).unwrap().unwrap();
        assert_eq!(
            after.state,
            if won {
                TaskState::Completed
            } else {
                TaskState::Cancelled
            }
        );
        assert_eq!(after.result.is_some(), won);
    }
}
