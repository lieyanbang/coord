//! Deliberately opt-in capacity run; ordinary regression gates stay quick.
mod common;
use common::{Daemon, Mcp};
use serde_json::json;
use std::{collections::HashSet, time::Instant};

#[cfg(windows)]
fn peak_memory(pid: u32) -> u64 {
    #[repr(C)]
    struct Counters {
        size: u32,
        faults: u32,
        peak_ws: usize,
        ws: usize,
        paged_peak: usize,
        paged: usize,
        nonpaged_peak: usize,
        nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            handle: *mut std::ffi::c_void,
            data: *mut Counters,
            size: u32,
        ) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    unsafe {
        let h = OpenProcess(0x410, 0, pid);
        assert!(!h.is_null());
        let mut c: Counters = std::mem::zeroed();
        c.size = std::mem::size_of::<Counters>() as u32;
        let ok = K32GetProcessMemoryInfo(h, &mut c, c.size);
        CloseHandle(h);
        assert_ne!(ok, 0);
        c.peak_ws as u64
    }
}
#[cfg(not(windows))]
fn peak_memory(_: u32) -> u64 {
    0
}

fn percentile(values: &mut [f64], p: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * p).ceil() as usize]
}

#[test]
#[ignore = "Stage E explicit release-daemon capacity run; see STAGE_E.md"]
fn capacity_100_1000_10000_real_daemon_and_six_mcp_processes() {
    assert!(
        std::env::var_os("COORD_TEST_BIN").is_some(),
        "explicit release binary required"
    );
    let mut reports = Vec::new();
    for n in [100, 1000, 10000] {
        let d = Daemon::new();
        let b = d.rpc("batches/create", json!({"name":format!("capacity-{n}")}));
        let id = b["id"].as_str().unwrap().to_string();
        let enqueue = Instant::now();
        for offset in (0..n).step_by(1000) {
            d.rpc("batches/add_tasks",json!({"batch_id":id,"tasks":(offset..(offset+1000).min(n)).map(|i|json!({"key":format!("{i}"),"name":"capacity","payload":{"i":i}})).collect::<Vec<_>>()}));
        }
        let enqueue_seconds = enqueue.elapsed().as_secs_f64();
        let started = Instant::now();
        let mut threads = Vec::new();
        for w in 0..6 {
            let url = d.url.clone();
            let id = id.clone();
            threads.push(std::thread::spawn(move||{
            let mut m=Mcp::new(&url);let mut ids=Vec::new();let mut latency=Vec::new();let agent=format!("capacity-{w}");
            loop {let begin=Instant::now();let c=m.tool("claim_task",json!({"agent_id":agent,"batch_id":id}));latency.push(begin.elapsed().as_secs_f64()*1000.0);if c["empty"]==true{break}let t=&c["task"];m.tool("submit_result",json!({"id":t["id"],"agent_id":agent,"lease_token":t["lease_token"],"result":t["payload"]}));ids.push(t["id"].as_str().unwrap().to_string());}
            (ids,latency,peak_memory(m.pid()))
        }));
        }
        let mut ids = HashSet::new();
        let mut latency = Vec::new();
        let mut workers_peak = 0u64;
        for thread in threads {
            let (done, times, peak) = thread.join().unwrap();
            for id in done {
                assert!(ids.insert(id), "duplicate effective claim");
            }
            latency.extend(times);
            workers_peak += peak;
        }
        let seconds = started.elapsed().as_secs_f64();
        assert_eq!(ids.len(), n);
        let progress = d.rpc("batches/status", json!({"batch_id":id}));
        assert_eq!(progress["states"]["completed"], n);
        assert_eq!(progress["terminal"], n);
        let conn = rusqlite::Connection::open(d.root.join("state.db")).unwrap();
        let receipts: usize = conn
            .query_row(
                "SELECT count(*) FROM task_receipts WHERE operation='complete'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(receipts, n);
        let idle = Instant::now();
        assert!(d
            .rpc("tasks/list", json!({"state":"pending","wait_ms":200}))
            .as_array()
            .unwrap()
            .is_empty());
        let idle_wait_ms = idle.elapsed().as_secs_f64() * 1000.0;
        assert!(idle_wait_ms >= 180.0);
        let log = std::fs::read_to_string(d.root.join("daemon.log")).unwrap();
        let busy = log
            .lines()
            .filter(|l| l.contains("database is locked") || l.contains("database is busy"))
            .count();
        let p50 = percentile(&mut latency, 0.5);
        let p95 = percentile(&mut latency, 0.95);
        let report = json!({"tasks":n,"workers":6,"enqueue_seconds":enqueue_seconds,"seconds":seconds,"tasks_per_second":n as f64/seconds,"claim_end_to_end_ms_p50":p50,"claim_end_to_end_ms_p95":p95,"idle_wait_ms":idle_wait_ms,"sqlite_busy_log_lines":busy,"daemon_peak_working_set_bytes":peak_memory(d.child.id()),"sum_worker_peak_working_set_bytes":workers_peak,"unique_completed":ids.len(),"receipts":receipts,"terminal_consistent":true});
        println!("{report}");
        reports.push(report);
        std::fs::write(
            "docs/verification/stage-e-capacity.json",
            serde_json::to_string_pretty(&reports).unwrap(),
        )
        .unwrap();
    }
}
