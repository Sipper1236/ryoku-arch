#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::sync::Arc;

use rusqlite::Connection;
use ryogami_proto::Event;
use tokio::sync::{Mutex, RwLock, broadcast};
use tracing::{info, warn};

use crate::config::Config;
use crate::wall::cache::CacheState;
use crate::wall::optimize::OptimizeState;
use crate::wall::watcher::SuppressSet;
use crate::wall::optimize;

#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use ryogami_proto::{Request, Response};
#[cfg(test)]
use crate::db;

mod process;
mod routing;
mod connection;
use process::*;
use routing::*;
pub use connection::run;
pub use process::ManagedProcess;

const CONFIG_RELOAD_DELAY_MS: u64 = 200;


fn should_launch_notification(config: &Config) -> bool {
    use crate::config::NotificationsBuiltIn::Never;
    !matches!(config.notifications.built_in, Never)
}

pub struct RandomRotation {
    pub handle: tokio::task::JoinHandle<()>,
    pub interval_secs: u64,
    pub types: Vec<String>,
    pub favourites_only: bool,
}

#[derive(Clone)]
pub struct SharedState {
    pub config: Arc<RwLock<Config>>,
    pub db: Arc<Mutex<Connection>>,
    pub db_shared: Arc<Mutex<Connection>>,
    pub ui: Arc<Mutex<ManagedProcess>>,
    pub host: Arc<Mutex<ManagedProcess>>,
    pub current_wallpaper: Arc<Mutex<Option<String>>>,
    pub cache_state: Arc<Mutex<CacheState>>,
    pub optimize_state: Arc<Mutex<OptimizeState>>,
    pub convert_state: Arc<Mutex<optimize::ConvertState>>,
    pub suppress_set: SuppressSet,
    pub random_rotation: Arc<Mutex<Option<RandomRotation>>>,
    pub runner: Arc<dyn crate::util::CommandRunner>,
}

pub fn broadcast_event(
    tx: &broadcast::Sender<String>,
    event: &str,
    data: serde_json::Value,
) -> Result<usize, broadcast::error::SendError<String>> {
    tx.send(make_event(event, data))
}

#[must_use]
pub fn make_event(event: &str, data: serde_json::Value) -> String {
    serde_json::to_string(&Event {
        event: event.to_string(),
        data,
    })
    .unwrap_or_default()
}

pub async fn auto_optimize_if_enabled(
    runner: Arc<dyn crate::util::CommandRunner>,
    config: &Config,
    db: Arc<Mutex<Connection>>,
    event_tx: broadcast::Sender<String>,
    optimize_state: Arc<Mutex<OptimizeState>>,
) {
    if !config.performance.auto_optimize_images {
        return;
    }
    let preset = config
        .performance
        .image_optimize_preset
        .as_deref()
        .unwrap_or("balanced");
    let resolution = config.performance.image_optimize_resolution.as_deref().unwrap_or("2k");
    info!("auto-optimizing images (preset={preset}, resolution={resolution})");
    if let Err(e) = optimize::start_optimize(runner, config, db, event_tx, optimize_state, preset, resolution).await {
        warn!("auto-optimize failed: {e}");
    }
}

#[cfg(test)]
pub(crate) struct TestHarness {
    pub state: SharedState,
    pub event_tx: broadcast::Sender<String>,
    pub subscriptions: Arc<Mutex<Vec<String>>>,
}

#[cfg(test)]
impl TestHarness {
    pub async fn dispatch(&self, method: &str, params: serde_json::Value) -> Response {
        let req = Request {
            method: method.to_string(),
            params,
            id: 1,
        };
        dispatch_request(&req, &self.event_tx, &self.subscriptions, &self.state).await
    }
}

#[cfg(test)]
pub(crate) fn test_state() -> TestHarness {
    let (event_tx, _events) = broadcast::channel::<String>(256);
    let config = Config::default();
    let state = SharedState {
        config: Arc::new(RwLock::new(config.clone())),
        db: Arc::new(Mutex::new(db::open_in_memory().expect("in-memory db"))),
        db_shared: Arc::new(Mutex::new(db::open_in_memory().expect("in-memory db_shared"))),
        ui: Arc::new(Mutex::new(ManagedProcess::new_dry("wall-ui", "SKWD_WALL_INSTALL", PathBuf::new()))),
        host: Arc::new(Mutex::new(ManagedProcess::new_dry("host", "SKWD_HOST_INSTALL", PathBuf::new()))),
        current_wallpaper: Arc::new(Mutex::new(None)),
        cache_state: Arc::new(Mutex::new(CacheState::default())),
        optimize_state: Arc::new(Mutex::new(OptimizeState::default())),
        convert_state: Arc::new(Mutex::new(optimize::ConvertState::default())),
        suppress_set: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        random_rotation: Arc::new(Mutex::new(None)),
        runner: Arc::new(crate::util::FakeRunner::new()),
    };
    TestHarness {
        state,
        event_tx,
        subscriptions: Arc::new(Mutex::new(Vec::new())),
    }
}

#[cfg(test)]
mod harness_tests {
    use super::*;

    #[tokio::test]
    async fn paper_ready_ok_with_and_without_pid() {
        let h = test_state();
        assert!(h.dispatch("paper.ready", serde_json::json!({})).await.error.is_none());
        assert!(
            h.dispatch("paper.ready", serde_json::json!({ "pid": 999_999 }))
                .await
                .error
                .is_none()
        );
    }

    #[tokio::test]
    async fn subscribe_registers_trimmed_prefixes() {
        let h = test_state();
        let resp = h
            .dispatch("subscribe", serde_json::json!({ "events": ["skwd.wall.*", "state.changed"] }))
            .await;
        assert!(resp.error.is_none());
        let subs = h.subscriptions.lock().await;
        assert!(subs.contains(&"skwd.wall.".to_string()));
        assert!(subs.contains(&"state.changed".to_string()));
    }

    #[tokio::test]
    async fn status_reports_version_and_null_wallpaper() {
        let h = test_state();
        let r = h.dispatch("status", serde_json::json!({})).await.result.unwrap();
        assert_eq!(r["version"], env!("CARGO_PKG_VERSION"));
        assert!(r["current_wallpaper"].is_null());
    }

    #[tokio::test]
    async fn theme_colors_returns_object() {
        let h = test_state();
        let r = h.dispatch("theme.colors", serde_json::json!({})).await.result.unwrap();
        assert!(r["colors"].is_object());
    }

    #[tokio::test]
    async fn state_set_get_delete_roundtrip() {
        let h = test_state();
        assert_eq!(
            h.dispatch("state.get", serde_json::json!({})).await.error.unwrap().code,
            -32602
        );
        h.dispatch("state.set", serde_json::json!({ "key": "k", "value": "v" })).await;
        let got = h.dispatch("state.get", serde_json::json!({ "key": "k" })).await.result.unwrap();
        assert_eq!(got["value"], "v");
        h.dispatch("state.set", serde_json::json!({ "key": "k" })).await;
        let got2 = h.dispatch("state.get", serde_json::json!({ "key": "k" })).await.result.unwrap();
        assert!(got2["value"].is_null());
    }

    #[tokio::test]
    async fn wallpapers_flag_gates_the_whole_wallpaper_surface() {
        let h = test_state();
        {
            let mut c = h.state.config.write().await;
            c.features.wallpapers = false;
        }
        for method in [
            "wall.cache_status",
            "effects.list",
            "optimize.status",
            "video_convert.status",
        ] {
            let e = h.dispatch(method, serde_json::json!({})).await.error.unwrap();
            assert_eq!(e.code, -32601, "{method}");
            assert!(
                e.message.contains("wallpapers module is disabled"),
                "{method}: {}",
                e.message
            );
        }
    }

    #[tokio::test]
    async fn unknown_top_level_method_errors() {
        let h = test_state();
        assert!(h.dispatch("nope.nope", serde_json::json!({})).await.error.is_some());
    }

    #[tokio::test]
    async fn wall_list_reflects_db_state() {
        let h = test_state();
        let r = h.dispatch("wall.list", serde_json::json!({})).await.result.unwrap();
        assert_eq!(r["count"], 0);

        {
            let db = h.state.db.lock().await;
            crate::db::upsert_cache_entry(&db, "static:x", "static", "x", "t", "ts", "", "", 1, 0, 0, 0)
                .unwrap();
        }
        let r2 = h.dispatch("wall.list", serde_json::json!({})).await.result.unwrap();
        assert_eq!(r2["count"], 1);
        assert_eq!(r2["wallpapers"][0]["key"], "static:x");
    }

    #[tokio::test]
    async fn wall_set_favourite_through_dispatch_updates_db() {
        let h = test_state();
        {
            let db = h.state.db.lock().await;
            crate::db::upsert_cache_entry(&db, "static:f", "static", "f", "t", "ts", "", "", 1, 0, 0, 0)
                .unwrap();
        }
        let resp = h
            .dispatch("wall.set_favourite", serde_json::json!({ "key": "static:f", "favourite": true }))
            .await;
        assert!(resp.error.is_none(), "{:?}", resp.error);
        let favs = h
            .dispatch("wall.list", serde_json::json!({ "favourites": true }))
            .await
            .result
            .unwrap();
        assert_eq!(favs["count"], 1);
    }
}
