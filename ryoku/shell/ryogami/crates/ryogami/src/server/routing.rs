#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::sync::Arc;

use rusqlite::params;
use ryogami_proto::{Request, Response};
use tokio::sync::{Mutex, broadcast};

use crate::wall::{self, optimize};

use super::*;

pub(super) async fn dispatch_request(
    req: &Request,
    event_tx: &broadcast::Sender<String>,
    subscriptions: &Arc<Mutex<Vec<String>>>,
    state: &SharedState,
) -> Response {
    if req.method == "paper.ready" {
        if let Some(pid) = req.params.get("pid").and_then(serde_json::Value::as_u64) {
            wall::apply::signal_paper_ready(pid as u32).await;
        }
        return Response::ok(req.id, serde_json::json!({"ok": true}));
    }
    if req.method.starts_with("wall.") {
        if !state.config.read().await.features.wallpapers {
            return Response::err(req.id, -32601, "wallpapers module is disabled");
        }
        return wall::dispatch(req, event_tx, state).await;
    }
    if req.method.starts_with("optimize.") || req.method.starts_with("video_convert.") {
        if !state.config.read().await.features.wallpapers {
            return Response::err(req.id, -32601, "wallpapers module is disabled");
        }
        return optimize::dispatch(req, event_tx, state).await;
    }
    if req.method.starts_with("effects.") {
        if !state.config.read().await.features.wallpapers {
            return Response::err(req.id, -32601, "wallpapers module is disabled");
        }
        return wall::effects::dispatch(req, event_tx, state).await;
    }
    match req.method.as_str() {
        "subscribe" => {
            if let Some(events) = req.params.get("events").and_then(|v| v.as_array()) {
                let mut subs = subscriptions.lock().await;
                for e in events {
                    if let Some(s) = e.as_str() {
                        let prefix = s.trim_end_matches('*').to_string();
                        if !subs.contains(&prefix) {
                            subs.push(prefix);
                        }
                    }
                }
            }
            Response::ok(req.id, serde_json::json!({"subscribed": true}))
        }

        "status" => {
            let wp = state.current_wallpaper.lock().await;
            Response::ok(
                req.id,
                serde_json::json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "current_wallpaper": *wp,
                }),
            )
        }

        "theme.colors" => Response::ok(req.id, serde_json::json!({"colors": {}})),

        "state.get" => {
            let key = req.str_param("key", "");
            if key.is_empty() {
                return Response::err(req.id, -32602, "missing key".to_string());
            }
            let db = state.db.lock().await;
            let val: Option<String> = db
                .query_row(
                    "SELECT val FROM state WHERE key=?1",
                    params![key],
                    |r| r.get(0),
                )
                .ok();
            Response::ok(req.id, serde_json::json!({ "value": val }))
        }

        "state.set" => {
            let key = req.str_param("key", "");
            let val = req.opt_str("value");
            if key.is_empty() {
                return Response::err(req.id, -32602, "missing key".to_string());
            }
            let db = state.db.lock().await;
            match val {
                Some(v) => {
                    let _ = db.execute(
                        "INSERT OR REPLACE INTO state(key, val) VALUES(?1, ?2)",
                        params![key, v],
                    );
                }
                None => {
                    let _ = db.execute("DELETE FROM state WHERE key=?1", params![key]);
                }
            }
            Response::ok(req.id, serde_json::json!({ "ok": true }))
        }

        _ => Response::err(req.id, -32601, format!("unknown method: {}", req.method)),
    }
}
