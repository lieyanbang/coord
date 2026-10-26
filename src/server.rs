//! Server-side actions: start the daemon, run the MCP bridge, print
//! version metadata. The CLI in [`crate::cli`] wraps these.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::{routing::get, Json};

use coord::a2a;
use coord::core::action_runner::RuntimeHealth;
use coord::core::actions::ActionRegistry;
use coord::core::store::Store;
use coord::mcp::Bridge;

/// How often the background ticker calls `reclaim_expired_leases`.
/// Short enough that a dead agent doesn't hold a task for long;
/// long enough that the sweep is cheap on a busy bulletin.
const RECLAIM_TICK: Duration = Duration::from_secs(30);
const ACTION_TICK: Duration = Duration::from_millis(100);
const ACTION_CONCURRENCY: usize = 2;

type BrokerSetup =
    std::result::Result<Option<(SocketAddr, axum::Router)>, (&'static str, anyhow::Error)>;

fn prepare_broker(
    registry: Option<Arc<ActionRegistry>>,
    store: Arc<Store>,
    health: RuntimeHealth,
) -> BrokerSetup {
    let Some(registry) = registry else {
        return Ok(None);
    };
    let config = registry
        .runner_config()
        .map_err(|error| ("invalid_runner_configuration", error))?;
    let Some(config) = config else {
        return Ok(None);
    };
    config
        .validate()
        .map_err(|error| ("invalid_runner_configuration", error))?;
    let identity = coord::core::windows_security::process_identity()
        .map_err(|error| ("daemon_identity_unavailable", error))?;
    if identity.sid != config.daemon_sid {
        return Err((
            "daemon_identity_mismatch",
            anyhow::anyhow!("configured daemon SID does not match the current process"),
        ));
    }
    let address = config.broker_addr;
    let router = coord::core::action_runner::broker_router(store, config, health)
        .map_err(|error| ("invalid_or_unreadable_public_key_configuration", error))?;
    Ok(Some((address, router)))
}

pub async fn serve(
    addr: SocketAddr,
    db: Option<PathBuf>,
    vault: Option<PathBuf>,
    actions_config: Option<PathBuf>,
) -> Result<()> {
    if !addr.ip().is_loopback() {
        tracing::warn!(%addr, "unauthenticated RPC exposed beyond loopback; only use a trusted proxy/tunnel");
    }
    let db_path = db.unwrap_or_else(default_db_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    tracing::info!(?db_path, ?vault, ?actions_config, "opening state");
    let actions = actions_config
        .as_deref()
        .map(coord::core::actions::ActionRegistry::load)
        .transpose()?
        .map(Arc::new);
    let store = Arc::new(Store::open_with_vault_and_actions(
        &db_path,
        vault,
        actions.clone(),
    )?);
    let bus = a2a::ChangeBus::new();
    if actions
        .as_ref()
        .is_some_and(|registry| registry.external_runner.is_none() && !registry.isolated_in_process)
    {
        tracing::warn!(
            "completion actions are queue-only: configure external_runner for production execution"
        );
    }
    let health = coord::core::action_runner::RuntimeHealth::new();
    let mut app = a2a::router_with_bus(store.clone(), bus.clone());
    let health_route = health.clone();
    app = app.route(
        "/healthz",
        get(move || {
            let health = health_route.clone();
            async move { Json(health.snapshot()) }
        }),
    );
    if let Some(registry) = store.action_registry() {
        if let Some(management_path) = registry.management_key_path()? {
            app = coord::core::action_runner::protect_management_rpc(
                app,
                coord::core::action_runner::management_key_from_path(&management_path)?,
            );
        }
    }
    // Bind RPC before attempting any Broker setup. This listener is the
    // Scheduler's independent fault domain; Broker failures below are health
    // state transitions and never propagate out of serve().
    tracing::info!(%addr, "coord listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;

    let broker_registry = store.action_registry();
    if broker_registry
        .as_ref()
        .is_some_and(|registry| registry.external_runner.is_some())
    {
        health.broker_starting();
    }
    let broker_store = store.clone();
    let broker_health = health.clone();
    tokio::spawn(async move {
        let setup_health = broker_health.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            prepare_broker(broker_registry, broker_store, setup_health)
        })
        .await;
        let broker = match prepared {
            Ok(Ok(Some(broker))) => broker,
            Ok(Ok(None)) => {
                broker_health.broker_unavailable("not_configured");
                return;
            }
            Ok(Err((reason, error))) => {
                tracing::error!(%error, reason, "Action Broker setup rejected; Scheduler remains available");
                broker_health.broker_security_error(reason);
                broker_health.runner_security_error(reason);
                return;
            }
            Err(error) => {
                tracing::error!(%error, "Action Broker initialization worker failed; Scheduler remains available");
                broker_health.broker_security_error("initialization_worker_failed");
                broker_health.runner_security_error("initialization_worker_failed");
                return;
            }
        };
        let (address, router) = broker;
        match tokio::net::TcpListener::bind(address).await {
            Err(error) => {
                tracing::error!(%error, %address, "Action Broker listener unavailable; Scheduler remains available");
                broker_health.broker_unavailable("listener_bind_failed");
            }
            Ok(broker_listener) => {
                broker_health.broker_ready();
                if let Err(error) = axum::serve(broker_listener, router).await {
                    tracing::error!(%error, "Action Broker stopped; actions remain queued");
                    broker_health.broker_unavailable("listener_stopped");
                }
            }
        }
    });
    // Bounded independent validation queue. The stored job fence prevents a
    // cancelled/retried task or a restarted consumer accepting stale verdicts.
    let validation_store = store.clone();
    let validation_bus = bus.clone();
    tokio::spawn(async move {
        let capacity = Arc::new(tokio::sync::Semaphore::new(4));
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        loop {
            tick.tick().await;
            while let Ok(permit) = capacity.clone().try_acquire_owned() {
                let job = match validation_store.begin_validation() {
                    Ok(Some(job)) => job,
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!(error=%e,"validation queue failed");
                        break;
                    }
                };
                let store = validation_store.clone();
                let bus = validation_bus.clone();
                tokio::spawn(async move {
                    let outcome = coord::core::validator::validate(&job).await;
                    match store.finish_validation(&job, outcome) {
                        Ok(true) => {
                            bus.notify();
                        }
                        Ok(false) => {}
                        Err(e) => tracing::warn!(error=%e,"validation result failed"),
                    }
                    drop(permit);
                });
            }
        }
    });

    if let Some(registry) = actions.filter(|registry| registry.isolated_in_process) {
        tracing::warn!(
            "isolated in-process action executor enabled; never use this mode in production"
        );
        let action_store = store.clone();
        tokio::spawn(async move {
            let capacity = Arc::new(tokio::sync::Semaphore::new(ACTION_CONCURRENCY));
            let owner = format!("coord-{}", uuid::Uuid::new_v4());
            let mut tick = tokio::time::interval(ACTION_TICK);
            loop {
                tick.tick().await;
                while let Ok(permit) = capacity.clone().try_acquire_owned() {
                    let job = match action_store.begin_action(&owner) {
                        Ok(Some(job)) => job,
                        Ok(None) => {
                            drop(permit);
                            break;
                        }
                        Err(error) => {
                            tracing::warn!(error=%error,"completion action queue failed");
                            drop(permit);
                            break;
                        }
                    };
                    let store = action_store.clone();
                    let registry = registry.clone();
                    tokio::spawn(async move {
                        let result = tokio::task::spawn_blocking(move || {
                            let execution = registry.execute(&job);
                            (job, execution)
                        })
                        .await;
                        match result {
                            Ok((job, execution)) => match store.finish_action(&job, &execution) {
                                Ok(true) => tracing::info!(
                                    action_id=%job.id,
                                    event_id=%job.event_id,
                                    state=execution.state.as_str(),
                                    code=execution.code,
                                    exit_code=?execution.exit_code,
                                    output_bytes=execution.output_bytes,
                                    "completion action finished"
                                ),
                                Ok(false) => tracing::warn!(
                                    action_id=%job.id,
                                    "completion action result lost its lease fence"
                                ),
                                Err(error) => tracing::error!(
                                    action_id=%job.id,
                                    error=%error,
                                    "completion action result could not be persisted"
                                ),
                            },
                            Err(error) => tracing::error!(
                                error=%error,
                                "completion action worker panicked; lease recovery will reconcile it"
                            ),
                        }
                        drop(permit);
                    });
                }
            }
        });
    }

    // Background reclaim sweep. Lazy reclaim on `tasks/list` keeps the
    // bulletin self-healing for active readers, but the ticker covers
    // the case where nobody is reading: a stuck task should still
    // un-stick within `RECLAIM_TICK`. Each sweep notifies the bus so
    // any long-pollers wake up immediately when a reclaim happens.
    let ticker_store = store.clone();
    let ticker_bus = bus.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(RECLAIM_TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match ticker_store.reclaim_expired_leases() {
                Ok(reclaimed) if !reclaimed.is_empty() => {
                    tracing::info!(
                        count = reclaimed.len(),
                        "reclaim_ticker: returned expired claims to pending"
                    );
                    ticker_bus.notify();
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "reclaim_ticker failed"),
            }
        }
    });

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

pub async fn mcp(url: String) -> Result<()> {
    Bridge::new(url).run().await
}

pub fn print_version() {
    println!("coord {}", env!("CARGO_PKG_VERSION"));
    println!("a2a   subset (tasks/send, tasks/get, tasks/cancel)");
    println!(
        "ext   tasks/claim, tasks/extend, tasks/reclaim, tasks/complete, tasks/list (long-poll)"
    );
    println!("mcp   bridge   (use `coord mcp` for stdio)");
    println!("vault markdown (use `coord serve --vault PATH` to enable)");
    println!("kinds task | bug | feature | decision | ack | knowledge | build");
    println!("prio  low | normal | high | urgent");
    println!("lease 300s default · 3600s max · auto-reclaim every 30s");
}

fn default_db_path() -> PathBuf {
    if let Some(dirs) = directories::ProjectDirs::from("dev", "coord", "coord") {
        return dirs.data_dir().join("state.db");
    }
    PathBuf::from(".coord/state.db")
}
