#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use rusqlite::params;
use ryogami_proto::{Request, Response};

use crate::wall::{self, optimize};

use super::*;

/// Cover is the schema default fit (ryoku contract 08 sec 8). Task 6 wires
/// `wallpaper.content_fit` from ryogami.json; until then every set uses Cover.
const DEFAULT_FIT: &str = "Cover";

/// Dispatch one wire command line and return its single-line reply. `subscribe
/// <topic>` is handled at the connection layer (topic streaming). A leading `{`
/// is an internal JSON-RPC request (wall/optimize/effects/state), kept for those
/// subsystems that are still method-keyed.
pub(super) async fn dispatch_command(line: &str, state: &SharedState) -> String {
    let cmd = line.trim();
    if cmd.starts_with('{') {
        return match serde_json::from_str::<Request>(cmd) {
            Ok(req) => reply_of(dispatch_request(&req, state).await),
            Err(e) => format!("err parse: {e}"),
        };
    }
    match cmd.split_whitespace().next().unwrap_or("") {
        "wallpaper" => wallpaper_command(cmd, state).await,
        "depth" => depth_command(cmd, state).await,
        "" => "err empty command".into(),
        other => format!("err unknown command: {other}"),
    }
}

fn reply_of(resp: Response) -> String {
    match resp.error {
        Some(e) => format!("err {}", e.message),
        None => "ok".into(),
    }
}

// --- wallpaper verbs ---------------------------------------------------------
//
// Grammar mirrors ryoku's Go dispatch: `wallpaper <mode> [--screen <name>] [arg]`.
// A path may contain spaces, so it is recovered verbatim after the flag is
// stripped; a connector name never contains spaces.

async fn wallpaper_command(line: &str, state: &SharedState) -> String {
    if !state.config.read().await.features.wallpapers {
        return "err wallpaper: wallpapers module is disabled".into();
    }
    let rest_full = line.strip_prefix("wallpaper").unwrap_or("").trim();
    let (rest, screen) = extract_screen(rest_full);
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    match tokens.first().copied().unwrap_or("") {
        "set" => {
            let path = rest.strip_prefix("set").unwrap_or("").trim();
            if path.is_empty() {
                return "err wallpaper: set requires a path".into();
            }
            wallpaper_set(state, path, &screen).await
        }
        "random" => wallpaper_random(state, &tokens).await,
        "restore" => wallpaper_restore(state).await,
        "audio" => wallpaper_audio(state, &tokens, &screen).await,
        "resource" => wallpaper_resource(state, &tokens).await,
        "" => "err wallpaper: missing mode".into(),
        other => format!("err wallpaper: unknown mode: {other}"),
    }
}

async fn wallpaper_set(state: &SharedState, path: &str, screen: &str) -> String {
    if screen.is_empty() {
        state.wall_surface.show(path, DEFAULT_FIT, None).await;
    } else {
        state.wall_surface.show_output(screen, path, DEFAULT_FIT, None).await;
    }
    *state.current_wallpaper.lock().await = Some(basename(path));
    state.depth.schedule(false);
    spawn_static_render(state, path, screen).await;
    "ok".into()
}

/// Drive the (still out-of-process) renderer detached: the topic frame is
/// already published, so a headless / display-less run still delivers it. Task 3
/// folds the renderer in-process.
async fn spawn_static_render(state: &SharedState, path: &str, screen: &str) {
    if is_headless() {
        return;
    }
    let cfg = state.config.read().await.clone();
    let path = path.to_string();
    let outs: Vec<String> = if screen.is_empty() { vec![] } else { vec![screen.to_string()] };
    tokio::spawn(async move {
        if let Err(e) = wall::apply::apply_static(&path, &outs, &[], &[], &cfg).await {
            tracing::warn!("apply_static failed: {e}");
        }
    });
}

async fn wallpaper_random(state: &SharedState, tokens: &[&str]) -> String {
    match tokens.get(1).copied() {
        Some("start") => {
            let mut params = serde_json::Map::new();
            if let Some(iv) = flag_value(tokens, "--interval").and_then(|s| s.parse::<u64>().ok()) {
                params.insert("interval".into(), serde_json::Value::from(iv));
            }
            let req = Request { method: "wall.random_start".into(), params: params.into(), id: 0 };
            reply_of(dispatch_request(&req, state).await)
        }
        Some("stop") => {
            let req = Request { method: "wall.random_stop".into(), params: serde_json::Value::Null, id: 0 };
            reply_of(dispatch_request(&req, state).await)
        }
        _ => random_once(state).await,
    }
}

/// One-shot random pick (ryoku's Super+Shift+W): apply a random still and publish.
async fn random_once(state: &SharedState) -> String {
    let last = state.current_wallpaper.lock().await.clone();
    let pick = {
        let conn = state.db.lock().await;
        crate::db::random_pick(&conn, last.as_deref(), &["static"], false).ok().flatten()
    };
    let Some((_key, _ty, name, _video, _we)) = pick else {
        return "err wallpaper: no wallpapers available".into();
    };
    let cfg = state.config.read().await.clone();
    let path = cfg.wallpaper_dir().join(&name).display().to_string();
    wallpaper_set(state, &path, "").await
}

async fn wallpaper_restore(state: &SharedState) -> String {
    let cfg = state.config.read().await.clone();
    match wall::apply::restore(&cfg).await {
        Ok(name) if !name.is_empty() => {
            let p = cfg.wallpaper_dir().join(&name);
            if p.is_file() {
                state.wall_surface.show(&p.display().to_string(), DEFAULT_FIT, None).await;
            }
            *state.current_wallpaper.lock().await = Some(name);
            state.depth.schedule(false);
            "ok".into()
        }
        Ok(_) => "ok".into(),
        Err(e) => format!("err wallpaper: {e}"),
    }
}

async fn wallpaper_audio(state: &SharedState, tokens: &[&str], screen: &str) -> String {
    let (mute, volume) = match tokens.get(1).copied() {
        Some("mute") => (Some(true), None),
        Some("unmute") => (Some(false), None),
        Some("volume") => match tokens.get(2).and_then(|s| s.parse::<u32>().ok()) {
            Some(v) => (None, Some(v.min(100))),
            None => return "err wallpaper: audio volume requires a number".into(),
        },
        _ => return "err wallpaper: audio expects mute|unmute|volume <n>".into(),
    };
    let outputs = if screen.is_empty() { None } else { Some(vec![screen.to_string()]) };
    let cfg = state.config.read().await.clone();
    wall::apply::set_audio_for(&cfg, mute, volume, outputs).await;
    "ok".into()
}

async fn wallpaper_resource(state: &SharedState, tokens: &[&str]) -> String {
    let Some(tier) = tokens.get(1).and_then(|s| ResourceTier::parse(s)) else {
        return "err wallpaper: resource expects low|medium|high".into();
    };
    *state.resource_tier.lock().await = tier;
    crate::render::set_tier(tier);
    "ok".into()
}

async fn depth_command(line: &str, state: &SharedState) -> String {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    match tokens.get(1).copied() {
        Some("refresh") => {
            // A forced regenerate: enable / model change / explicit user refresh
            // all want a fresh cut even when a reusable one exists.
            state.depth.schedule(true);
            "ok".into()
        }
        Some("status") => serde_json::json!({
            "busy": state.depth.is_busy(),
            "path": wall::depth::default_cutout(state).await,
        })
        .to_string(),
        _ => "err depth expects refresh|status".into(),
    }
}

fn extract_screen(rest: &str) -> (String, String) {
    let Some(i) = rest.find("--screen ") else {
        return (rest.to_string(), String::new());
    };
    let after = &rest[i + "--screen ".len()..];
    let end = after.find(' ').unwrap_or(after.len());
    let screen = after[..end].trim().to_string();
    let mut r = rest[..i].trim_end().to_string();
    let tail = after[end..].trim_start();
    if !tail.is_empty() {
        if !r.is_empty() {
            r.push(' ');
        }
        r.push_str(tail);
    }
    (r.trim().to_string(), screen)
}

fn flag_value<'a>(tokens: &'a [&'a str], flag: &str) -> Option<&'a str> {
    tokens.iter().position(|t| *t == flag).and_then(|i| tokens.get(i + 1).copied())
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

// --- internal RPC router -----------------------------------------------------
//
// The wall/optimize/effects/state subsystems are still keyed by method name; the
// line verbs above call into this router.

pub(super) async fn dispatch_request(req: &Request, state: &SharedState) -> Response {
    if req.method.starts_with("wall.") {
        if !state.config.read().await.features.wallpapers {
            return Response::err(req.id, -32601, "wallpapers module is disabled");
        }
        return wall::dispatch(req, &state.event_tx, state).await;
    }
    if req.method.starts_with("optimize.") || req.method.starts_with("video_convert.") {
        if !state.config.read().await.features.wallpapers {
            return Response::err(req.id, -32601, "wallpapers module is disabled");
        }
        return optimize::dispatch(req, &state.event_tx, state).await;
    }
    if req.method.starts_with("effects.") {
        if !state.config.read().await.features.wallpapers {
            return Response::err(req.id, -32601, "wallpapers module is disabled");
        }
        return wall::effects::dispatch(req, &state.event_tx, state).await;
    }
    match req.method.as_str() {
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
                .query_row("SELECT val FROM state WHERE key=?1", params![key], |r| r.get(0))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::test_state;

    #[tokio::test]
    async fn resource_sets_tier() {
        let h = test_state();
        assert_eq!(dispatch_command("wallpaper resource high", &h.state).await, "ok");
        assert_eq!(*h.state.resource_tier.lock().await, ResourceTier::High);
        assert!(dispatch_command("wallpaper resource bogus", &h.state).await.starts_with("err"));
    }

    #[tokio::test]
    async fn depth_status_reports_busy_and_path() {
        // The contract QuickSettingsDepth.qml parses: {busy, path}.
        let h = test_state();
        let out = dispatch_command("depth status", &h.state).await;
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["busy"], false);
        assert_eq!(v["path"], "");
    }

    #[tokio::test]
    async fn depth_refresh_is_accepted() {
        let h = test_state();
        assert_eq!(dispatch_command("depth refresh", &h.state).await, "ok");
    }

    #[tokio::test]
    async fn unknown_command_errors() {
        let h = test_state();
        assert!(dispatch_command("bogus verb", &h.state).await.starts_with("err"));
    }

    #[test]
    fn extract_screen_recovers_path_and_connector() {
        let (rest, screen) = extract_screen("set /a b.png --screen DP-2");
        assert_eq!(rest, "set /a b.png");
        assert_eq!(screen, "DP-2");
        let (rest2, screen2) = extract_screen("set /a.png");
        assert_eq!(rest2, "set /a.png");
        assert_eq!(screen2, "");
    }
}
