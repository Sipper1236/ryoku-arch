#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use rusqlite::params;
use ryogami_proto::{Request, Response};

use crate::wall::{self, optimize};

use super::*;

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
        "next" => wallpaper_next(state, &screen).await,
        "repaint" => wallpaper_repaint(state).await,
        "live-reload" => wallpaper_live_reload(state).await,
        "restore" => wallpaper_restore(state).await,
        "audio" => wallpaper_audio(state, &tokens, &screen).await,
        "resource" => wallpaper_resource(state, &tokens).await,
        "" => "err wallpaper: missing mode".into(),
        other => format!("err wallpaper: unknown mode: {other}"),
    }
}

async fn wallpaper_set(state: &SharedState, path: &str, screen: &str) -> String {
    let fit = crate::config::content_fit();
    if screen.is_empty() {
        state.wall_surface.show(path, &fit, None).await;
    } else {
        state.wall_surface.show_output(screen, path, &fit, None).await;
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

/// Advance to the next wallpaper (ryoku's Super+W `next`). Ryogami has no shuffled
/// bag, so this is the file after the current one in the wallpaper dir (wrapping),
/// then applied like a set. Mirrors the Go `next` branch (popBag -> applyPick).
async fn wallpaper_next(state: &SharedState, screen: &str) -> String {
    let cfg = state.config.read().await.clone();
    let files = list_wallpaper_files(&cfg);
    if files.is_empty() {
        return "err wallpaper: no wallpapers available".into();
    }
    let current = state.current_wallpaper.lock().await.clone();
    let pick = next_after(&files, current.as_deref());
    apply_pick(state, &pick, screen).await
}

/// Re-apply the current wallpaper in place (ryoku's `repaint`, used after a theme /
/// filter change): re-emit the frame with a bumped revision so the re-rendered
/// source is reloaded, then re-run the render for the current still(s). No reveal.
/// Mirrors the Go `repaint` branch (scheduleTheme + wall.republish).
async fn wallpaper_repaint(state: &SharedState) -> String {
    state.wall_surface.republish().await;
    let snap = state.wall_surface.snapshot().await;
    if !snap.default.live && !snap.default.path.is_empty() {
        spawn_static_render(state, &snap.default.path, "").await;
    }
    for (name, entry) in &snap.outputs {
        if !entry.live && !entry.path.is_empty() {
            spawn_static_render(state, &entry.path, name).await;
        }
    }
    "ok".into()
}

/// Restart the live (video) players with fresh settings (ryoku's `live-reload`).
/// A still current is a no-op. Mirrors the Go `live-reload` branch.
async fn wallpaper_live_reload(state: &SharedState) -> String {
    let cfg = state.config.read().await.clone();
    match wall::apply::live_reload(&cfg).await {
        Ok(_) => "ok".into(),
        Err(e) => format!("err wallpaper: {e}"),
    }
}

/// Apply one resolved pick: a video (re)starts its live player, a still publishes
/// the frame and renders. Mirrors ryoku's Go applyPick.
async fn apply_pick(state: &SharedState, path: &str, screen: &str) -> String {
    if !is_video(path) {
        return wallpaper_set(state, path, screen).await;
    }
    let cfg = state.config.read().await.clone();
    let outs: Vec<String> = if screen.is_empty() { vec![] } else { vec![screen.to_string()] };
    let empty_audio: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    let empty_vol: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    if let Err(e) = wall::apply::apply_video(path, &outs, &[], &[], &empty_audio, &empty_vol, &cfg).await {
        return format!("err wallpaper: {e}");
    }
    *state.current_wallpaper.lock().await = Some(basename(path));
    state.depth.schedule(false);
    "ok".into()
}

/// Every image/video under the wallpaper (and video) dir, sorted for a stable
/// `next`. Mirrors ryoku's Go listPics (wallDir + liveDir walk).
fn list_wallpaper_files(config: &crate::config::Config) -> Vec<String> {
    let wall_dir = config.wallpaper_dir();
    let video_dir = config.video_dir();
    let mut roots = vec![wall_dir.clone()];
    if video_dir != wall_dir {
        roots.push(video_dir);
    }
    let mut files: Vec<String> = Vec::new();
    for root in roots {
        for entry in walkdir::WalkDir::new(&root).into_iter().filter_map(std::result::Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if wall::IMAGE_EXTS.contains(&ext.as_str()) || wall::VIDEO_EXTS.contains(&ext.as_str()) {
                files.push(path.display().to_string());
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

/// The file after `current` (matched by basename) in `files`, wrapping to the
/// first; the first when the current is absent. `files` must be non-empty.
fn next_after(files: &[String], current: Option<&str>) -> String {
    let cur = current.unwrap_or("");
    match files.iter().position(|f| basename(f) == cur) {
        Some(i) => files[(i + 1) % files.len()].clone(),
        None => files[0].clone(),
    }
}

fn is_video(path: &str) -> bool {
    let lower = path.to_lowercase();
    wall::VIDEO_EXTS.iter().any(|e| lower.ends_with(&format!(".{e}")))
}

async fn wallpaper_restore(state: &SharedState) -> String {
    let cfg = state.config.read().await.clone();
    match wall::apply::restore(&cfg).await {
        Ok(name) if !name.is_empty() => {
            let p = cfg.wallpaper_dir().join(&name);
            if p.is_file() {
                state.wall_surface.show(&p.display().to_string(), &crate::config::content_fit(), None).await;
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
    state.config.write().await.resource_tier = tier;
    crate::render::set_tier(tier);
    if let Err(e) = crate::config::persist_resource_tier(&state.config_file, tier) {
        tracing::warn!("failed to persist resource tier: {e}");
    }
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
        assert_eq!(h.state.config.read().await.resource_tier, ResourceTier::High, "in-memory config updated");
        // The tier is persisted to the (temp) ryogami.json so a restart honours it.
        let persisted: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&h.state.config_file).unwrap()).unwrap();
        assert_eq!(persisted["resource_tier"], "high");
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

    #[test]
    fn next_after_advances_wraps_and_defaults() {
        let files = vec![
            "/w/a.png".to_string(),
            "/w/b.png".to_string(),
            "/w/c.png".to_string(),
        ];
        // The pick is always a different file than the current one.
        assert_eq!(next_after(&files, Some("a.png")), "/w/b.png");
        assert_eq!(next_after(&files, Some("b.png")), "/w/c.png");
        // Last wraps back to the first.
        assert_eq!(next_after(&files, Some("c.png")), "/w/a.png");
        // Absent / unknown current starts at the first.
        assert_eq!(next_after(&files, None), "/w/a.png");
        assert_eq!(next_after(&files, Some("gone.png")), "/w/a.png");
    }

    #[test]
    fn list_wallpaper_files_collects_media_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let wall = dir.path().join("walls");
        std::fs::create_dir_all(&wall).unwrap();
        std::fs::write(wall.join("b.png"), b"i").unwrap();
        std::fs::write(wall.join("a.jpg"), b"i").unwrap();
        std::fs::write(wall.join("clip.mp4"), b"v").unwrap();
        std::fs::write(wall.join("notes.txt"), b"x").unwrap(); // non-media, skipped
        let mut cfg = crate::config::Config::default();
        cfg.paths.wallpaper = Some(wall.display().to_string());

        let files = list_wallpaper_files(&cfg);
        let names: Vec<String> = files.iter().map(|f| basename(f)).collect();
        assert_eq!(names, vec!["a.jpg", "b.png", "clip.mp4"], "media only, sorted");
    }

    #[tokio::test]
    async fn next_errors_when_no_wallpapers() {
        let h = test_state();
        let empty = tempfile::tempdir().unwrap();
        h.state.config.write().await.paths.wallpaper = Some(empty.path().to_string_lossy().to_string());
        h.state.config.write().await.paths.video_wallpaper = Some(empty.path().to_string_lossy().to_string());
        assert!(dispatch_command("wallpaper next", &h.state).await.starts_with("err"));
    }

    #[tokio::test]
    async fn repaint_bumps_revision_and_replies_ok() {
        // repaint must re-emit the frame; the bumped revision is the re-emit's
        // proof (a byte-identical frame would be suppressed by the topic).
        let h = test_state();
        let before = h.state.wall_surface.snapshot().await.default.revision;
        assert_eq!(dispatch_command("wallpaper repaint", &h.state).await, "ok");
        let after = h.state.wall_surface.snapshot().await.default.revision;
        assert!(after > before, "repaint bumped the frame revision ({before} -> {after})");
    }

    #[tokio::test]
    async fn live_reload_noops_on_static() {
        let h = test_state();
        let cache = tempfile::tempdir().unwrap();
        h.state.config.write().await.paths.cache = Some(cache.path().to_string_lossy().to_string());
        std::fs::write(cache.path().join("last-wallpaper.json"), r#"{"type":"static","path":"/w/x.png"}"#).unwrap();
        assert_eq!(dispatch_command("wallpaper live-reload", &h.state).await, "ok");
    }
}
