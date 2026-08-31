use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};
use tracing::{debug, info, warn};
use ryogami_proto::{Event, Request, Response};

use crate::db;
use crate::wall::cache::CacheState;
use crate::wall::optimize::OptimizeState;
use crate::wall::{self, apply, cache, optimize, watcher};
use notify::Watcher as _;

use super::*;

pub async fn run() -> anyhow::Result<()> {
    let sock_path = ryogami_proto::socket_path();

    if let Some(parent) = sock_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if sock_path.exists() {
        tokio::fs::remove_file(&sock_path).await?;
    }

    let listener = UnixListener::bind(&sock_path)?;
    info!("listening on {}", sock_path.display());

    let (event_tx, _) = broadcast::channel::<String>(256);

    wall::bootstrap::run(&crate::config::load().unwrap_or_default()).await;
    let config = crate::config::load().expect("failed to load config");
    if config.features.wallpapers {
        wall::clean_trash::run(&config).await;

        let cfg_clone = config.clone();
        tokio::spawn(async move {
            if let Some(prev) = wall::overview_backdrop::resolve_source(&cfg_clone).await {
                wall::overview_backdrop::refresh(&prev, &cfg_clone).await;
            }
        });
    } else {
        info!("wallpapers module disabled by config; skipping wallpaper subsystems");
    }

    let (wall_surface, topics) = WallSurface::new();
    let state = SharedState {
        config: Arc::new(RwLock::new(config.clone())),
        db: Arc::new(Mutex::new(db::open().expect("failed to open database"))),
        db_shared: Arc::new(Mutex::new(db::open().expect("failed to open shared db"))),
        ui: Arc::new(Mutex::new(ManagedProcess::new("wall-ui", "RYOGAMI_WALL_INSTALL", resolve_shell_qml()))),
        host: Arc::new(Mutex::new(ManagedProcess::new("host", "RYOGAMI_HOST_INSTALL", resolve_host_qml()))),
        current_wallpaper: Arc::new(Mutex::new(None)),
        cache_state: Arc::new(Mutex::new(CacheState::default())),
        optimize_state: Arc::new(Mutex::new(OptimizeState::default())),
        convert_state: Arc::new(Mutex::new(optimize::ConvertState::default())),
        suppress_set: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        random_rotation: Arc::new(Mutex::new(None)),
        runner: Arc::new(crate::util::RealRunner),
        topics,
        wall_surface,
        resource_tier: Arc::new(Mutex::new(config.resource_tier)),
        config_file: crate::config::config_path(),
        event_tx: event_tx.clone(),
    };
    // The renderer reads the tier through `render::current_tier`; seed it from the
    // persisted config so a restart honours the last `wallpaper resource`.
    crate::render::set_tier(config.resource_tier);
    // Publish the empty snapshot so a subscriber before the first set sees a frame.
    state.wall_surface.publish_current().await;

    {
        // The host is the skwd suite's second shell (launcher, bar, power); on
        // Ryoku those surfaces belong to ryoku-shell and the host QML is not
        // shipped, so skip cleanly instead of spawning a doomed quickshell.
        let host_qml = resolve_host_qml();
        if host_qml.exists() {
            let extra_env = build_host_env(&config).await;
            state.host.lock().await.launch_with_env(&extra_env);
        } else {
            info!("host shell absent at {}; skipping (ryoku-shell owns the desktop)", host_qml.display());
        }
    }

    let _watcher_handle: Option<notify::RecommendedWatcher> = if !config.features.wallpapers {
        None
    } else {
        match watcher::start(&config, &state.suppress_set) {
            Ok((rx, handle)) => {
                let tx = event_tx.clone();
                let ws = state.clone();
                tokio::spawn(run_watcher_loop(rx, tx, ws));
                Some(handle)
            }
            Err(e) => {
                warn!("file watcher failed to start: {e}");
                None
            }
        }
    };

    let _config_watcher: Option<notify::RecommendedWatcher> = {
        let config_path = crate::config::config_path();
        let config_dir = config_path.parent().map(std::path::Path::to_path_buf);
        let state = state.clone();
        let tx = event_tx.clone();

        let (cfg_tx, mut cfg_rx) = mpsc::unbounded_channel::<()>();
        let cfg_file = config_path.clone();

        let watcher = config_dir.and_then(|dir| {
            let mut w = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let dominated = matches!(event.kind, notify::EventKind::Modify(_) | notify::EventKind::Create(_));
                    if dominated && event.paths.iter().any(|p| p == &cfg_file) {
                        let _ = cfg_tx.send(());
                    }
                }
            })
            .ok()?;
            w.watch(&dir, notify::RecursiveMode::NonRecursive).ok()?;
            info!("[config] watching {}", dir.display());
            Some(w)
        });

        tokio::spawn(async move {
            while cfg_rx.recv().await.is_some() {
                tokio::time::sleep(std::time::Duration::from_millis(CONFIG_RELOAD_DELAY_MS)).await;
                while cfg_rx.try_recv().is_ok() {}

                match crate::config::load() {
                    Ok(new_cfg) => {
                        info!("[config] reloaded from {}", config_path.display());
                        let (prev_engine, prev_niri) = {
                            let c = state.config.read().await;
                            (c.paper.engine, c.niri.clone())
                        };
                        let new_engine = new_cfg.paper.engine;
                        let backdrop_changed = prev_niri.overview_backdrop != new_cfg.niri.overview_backdrop
                            || prev_niri.overview_backdrop_blur_enabled != new_cfg.niri.overview_backdrop_blur_enabled
                            || prev_niri.overview_backdrop_blur != new_cfg.niri.overview_backdrop_blur
                            || prev_niri.backdrop != new_cfg.niri.backdrop
                            || prev_niri.backdrop_follow_wallpaper != new_cfg.niri.backdrop_follow_wallpaper
                            || prev_niri.backdrop_auto_theme != new_cfg.niri.backdrop_auto_theme
                            || prev_niri.backdrop_theme != new_cfg.niri.backdrop_theme
                            || prev_niri.backdrop_dim != new_cfg.niri.backdrop_dim;
                        *state.config.write().await = new_cfg;
                        let _ = broadcast_event(&tx, "ryogami.wall.config_changed", serde_json::json!({}));
                        {
                            let tier = state.config.read().await.resource_tier;
                            *state.resource_tier.lock().await = tier;
                            crate::render::set_tier(tier);
                        }

                        let wallpapers_on = state.config.read().await.features.wallpapers;
                        if wallpapers_on && prev_engine != new_engine {
                            info!(
                                "[config] paper.engine changed: {:?} -> {:?}, re-applying static wallpapers",
                                prev_engine, new_engine
                            );
                            let cfg_snapshot = state.config.read().await.clone();
                            tokio::spawn(async move {
                                if let Err(e) =
                                    crate::wall::apply::reapply_statics_for_engine_change(&cfg_snapshot).await
                                {
                                    warn!("[config] engine-change re-apply failed: {e}");
                                }
                            });
                        }

                        if wallpapers_on && backdrop_changed {
                            info!("[config] niri overview backdrop settings changed, re-rendering");
                            let cfg = state.config.read().await.clone();
                            tokio::spawn(async move {
                                if cfg.niri.overview_backdrop {
                                    if let Some(src) = crate::wall::overview_backdrop::resolve_source(&cfg).await {
                                        crate::wall::overview_backdrop::refresh(&src, &cfg).await;
                                    }
                                } else {
                                    crate::wall::overview_backdrop::refresh("", &cfg).await;
                                }
                            });
                        }
                    }
                    Err(e) => {
                        warn!("[config] reload failed: {e}");
                    }
                }
            }
        });

        watcher
    };

    loop {
        let (stream, _addr) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, state).await {
                debug!("client error: {e}");
            }
        });
    }
}

pub(super) fn file_removed_payload(name: &str, file_type: &watcher::FileType) -> serde_json::Value {
    let wp_type = if *file_type == watcher::FileType::Static { "static" } else { "video" };
    serde_json::json!({ "name": name, "type": wp_type })
}

pub(super) fn folder_removed_payload(prefix: &str, names: &[String]) -> serde_json::Value {
    serde_json::json!({ "prefix": prefix, "names": names })
}

pub(super) fn we_added_payload(we_id: &str, we_dir: &std::path::Path) -> serde_json::Value {
    serde_json::json!({ "we_id": we_id, "we_dir": we_dir.display().to_string() })
}

pub(super) fn we_removed_payload(we_id: &str) -> serde_json::Value {
    serde_json::json!({ "we_id": we_id })
}

async fn auto_recolor_new(
    state: &SharedState,
    tx: &broadcast::Sender<String>,
    config: &crate::config::Config,
    src_name: &str,
    src_path: &std::path::Path,
) {
    let theme = config.effects.auto_recolor_theme().to_string();
    let params = serde_json::json!({ "theme": theme });
    let sfx = wall::effects::native::suffix("theme", &params);
    let Ok(out) = wall::effects::native::library_path(src_path, &sfx) else {
        return;
    };
    let wall_dir = config.wallpaper_dir();
    let out_name = out.strip_prefix(&wall_dir).map_or_else(
        |_| out.file_name().map_or_else(String::new, |n| n.to_string_lossy().to_string()),
        |p| p.to_string_lossy().to_string(),
    );

    {
        let mut set = state.suppress_set.lock().unwrap();
        set.insert(out_name.clone());
    }
    let render_res = wall::effects::native::render("theme", src_path, &params, &out).await;
    {
        let mut set = state.suppress_set.lock().unwrap();
        set.remove(&out_name);
    }

    match render_res {
        Ok(()) => {
            info!("[server] auto-recolored {src_name} -> {out_name} ({theme})");
            cache::process_single(config, state.db_shared.clone(), tx, &out_name, &out, "static").await;
        }
        Err(e) => warn!("[server] auto-recolor failed for {src_name}: {e}"),
    }
}

async fn run_watcher_loop(
    mut rx: mpsc::UnboundedReceiver<watcher::FsEvent>,
    tx: broadcast::Sender<String>,
    state: SharedState,
) {
    enum WatcherPhase {
        Scanning,
        Ready,
    }
    let mut phase = WatcherPhase::Scanning;

    loop {
        let Some(evt) = rx.recv().await else { break };

        match &evt {
            watcher::FsEvent::FileAdded { name, path, file_type } => {
                if matches!(phase, WatcherPhase::Scanning) {
                    continue;
                }
                info!("[server] watcher FileAdded name={name} path={}", path.display());
                let wp_type = if *file_type == watcher::FileType::Static {
                    "static"
                } else {
                    "video"
                };
                let config = state.config.read().await.clone();
                let suppress = state.suppress_set.clone();
                let db = state.db_shared.clone();

                let (final_name, final_path): (String, std::path::PathBuf) =
                if wp_type == "static" && config.performance.auto_optimize_images && optimize::should_optimize(name) {
                    let stem = std::path::Path::new(name)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(name);
                    let new_name = format!("{stem}.webp");

                    {
                        let mut set = suppress.lock().unwrap();
                        set.insert(name.clone());
                        set.insert(new_name.clone());
                    }

                    let result = match optimize::optimize_single_inline(&*state.runner, &config, &db, path, name).await {
                        Ok((fname, fpath)) => {
                            info!("[server] optimized {name} -> {fname}");
                            cache::process_single(&config, db.clone(), &tx, &fname, &fpath, wp_type).await;
                            (fname, fpath)
                        }
                        Err(e) => {
                            warn!("[server] optimize failed for {name}: {e}, caching original");
                            cache::process_single(&config, db.clone(), &tx, name, path, wp_type).await;
                            (name.clone(), path.clone())
                        }
                    };

                    {
                        let mut set = suppress.lock().unwrap();
                        set.remove(name);
                        set.remove(&new_name);
                    }
                    result
                } else {
                    cache::process_single(&config, db.clone(), &tx, name, path, wp_type).await;
                    (name.clone(), path.clone())
                };

                if wp_type == "static"
                    && config.effects.auto_recolor
                    && !final_name.contains("effects/")
                {
                    auto_recolor_new(&state, &tx, &config, &final_name, &final_path).await;
                }
            }
            watcher::FsEvent::FileRemoved { name, file_type } => {
                let wp_type = if *file_type == watcher::FileType::Static { "static" } else { "video" };
                let config = state.config.read().await.clone();
                {
                    let db = state.db_shared.clone();
                    let conn = db.lock().await;
                    let _ = db::delete_by_name(&conn, name);
                    let src_path = if *file_type == watcher::FileType::Static {
                        config.wallpaper_dir().join(name)
                    } else {
                        config.video_dir().join(name)
                    };
                    let _ = db::delete_optimize_by_src(&conn, &src_path.display().to_string());
                }
                {
                    let cache_dir = config.cache_dir().join("wallpaper");
                    let thumb_name = name.replace('/', "--") + ".webp";
                    if wp_type == "static" {
                        let _ = std::fs::remove_file(cache_dir.join("thumbs").join(&thumb_name));
                        let _ = std::fs::remove_file(cache_dir.join("thumbs-sm").join(&thumb_name));
                    } else {
                        let _ = std::fs::remove_file(cache_dir.join("video-thumbs").join(&thumb_name));
                        let _ = std::fs::remove_file(cache_dir.join("thumbs-sm").join(format!("vid-{thumb_name}")));
                    }
                }
                let _ = broadcast_event(&tx, "ryogami.wall.file_removed", file_removed_payload(name, file_type));
            }
            watcher::FsEvent::FolderRemoved { prefix } => {
                let db = state.db_shared.clone();
                let deleted = db::delete_by_name_prefix(&*db.lock().await, prefix).unwrap_or_default();
                if !deleted.is_empty() {
                    let config = state.config.read().await.clone();
                    let cache_dir = config.cache_dir().join("wallpaper");
                    for name in &deleted {
                        let thumb_name = name.replace('/', "--") + ".webp";
                        for sub in &["thumbs", "video-thumbs"] {
                            let _ = std::fs::remove_file(cache_dir.join(sub).join(&thumb_name));
                        }
                        let _ = std::fs::remove_file(cache_dir.join("thumbs-sm").join(&thumb_name));
                        let _ = std::fs::remove_file(cache_dir.join("thumbs-sm").join(format!("vid-{thumb_name}")));
                    }
                    let _ = broadcast_event(&tx, "ryogami.wall.folder_removed", folder_removed_payload(prefix, &deleted));
                }
            }
            watcher::FsEvent::WeAdded { we_id, we_dir } => {
                if matches!(phase, WatcherPhase::Scanning) {
                    continue;
                }
                info!("[server] watcher WeAdded we_id={we_id} dir={}", we_dir.display());
                let _ = broadcast_event(&tx, "ryogami.wall.we_added", we_added_payload(we_id, we_dir));
                let config = state.config.read().await.clone();
                let db = state.db_shared.clone();
                cache::process_we_single(&config, db, &tx, we_id, we_dir).await;
            }
            watcher::FsEvent::WeRemoved { we_id } => {
                let _ = broadcast_event(&tx, "ryogami.wall.we_removed", we_removed_payload(we_id));
            }
            watcher::FsEvent::ScanDone => {
                phase = WatcherPhase::Ready;
                let _ = broadcast_event(&tx, "ryogami.wall.scan_done", serde_json::json!({}));
                info!("initial directory scan complete, starting cache rebuild");

                let config = state.config.read().await.clone();
                let db = state.db_shared.clone();
                cache::rebuild(&config, db.clone(), state.cache_state.clone(), tx.clone()).await;
                auto_optimize_if_enabled(state.runner.clone(), &config, db, tx.clone(), state.optimize_state.clone()).await;

                if config.restore_on_startup {
                    match apply::restore(&config).await {
                        Ok(name) => {
                            info!("auto-restored wallpaper: {name}");
                            // Ryoku renders from the wallpaper topic; without
                            // this publish a daemon restart leaves the in-shell
                            // surface on the empty retained frame (mirrors the
                            // `wallpaper restore` verb).
                            let p = config.wallpaper_dir().join(&name);
                            if p.is_file() {
                                state
                                    .wall_surface
                                    .show(&p.display().to_string(), &crate::config::content_fit(), None)
                                    .await;
                            }
                        }
                        Err(e) => info!("no wallpaper to restore: {e}"),
                    }
                } else {
                    info!("startup restore disabled by config");
                }
            }
        }
    }
}

async fn handle_client(stream: tokio::net::UnixStream, state: SharedState) -> anyhow::Result<()> {
    let (reader, writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let mut first = String::new();
    if reader.read_line(&mut first).await? == 0 {
        return Ok(());
    }
    let cmd = first.trim();
    if cmd.is_empty() {
        return Ok(());
    }

    // A long-lived topic subscription streams one topic (the shell's wallpaper
    // frame); everything else enters the request loop, which serves line verbs,
    // JSON-RPC requests, and, after a JSON `subscribe`, pushed events on the
    // same connection: the contract the wall-ui client expects.
    if let Some(name) = cmd.strip_prefix("subscribe ") {
        return serve_subscription(reader, writer, &state, name.trim()).await;
    }
    serve_requests(reader, writer, &state, first).await
}

/// Serve one client connection: each line is a verb or JSON request answered in
/// order. A JSON `subscribe` request additionally streams broadcast events whose
/// name matches one of the requested prefixes, interleaved between replies, so
/// the wall-ui holds one socket for calls and change notifications alike. A
/// plain one-shot client (the CLI, the Go shell daemon) sends its line, reads
/// the reply, and closes; EOF ends the loop either way.
async fn serve_requests(
    mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    mut writer: tokio::net::unix::OwnedWriteHalf,
    state: &SharedState,
    mut line: String,
) -> anyhow::Result<()> {
    let mut events: Option<broadcast::Receiver<String>> = None;
    let mut prefixes: Vec<String> = Vec::new();
    loop {
        let cmd = line.trim();
        if !cmd.is_empty() {
            let reply = if cmd.starts_with('{') {
                match subscribe_request(cmd) {
                    Some(req) => {
                        prefixes = subscribe_prefixes(&req);
                        events = Some(state.event_tx.subscribe());
                        serde_json::to_string(&Response::ok(
                            req.id,
                            serde_json::json!({ "subscribed": prefixes }),
                        ))
                        .unwrap_or_default()
                    }
                    None => dispatch_json(cmd, state).await,
                }
            } else {
                dispatch_command(cmd, state).await
            };
            writer.write_all(reply.as_bytes()).await?;
            writer.write_all(b"\n").await?;
        }
        line.clear();
        match &mut events {
            None => {
                if reader.read_line(&mut line).await? == 0 {
                    return Ok(());
                }
            }
            Some(rx) => loop {
                tokio::select! {
                    read = reader.read_line(&mut line) => match read {
                        Ok(0) => return Ok(()),
                        Ok(_) => break,
                        Err(e) => return Err(e.into()),
                    },
                    recv = rx.recv() => match recv {
                        Ok(ev) => {
                            if event_matches(&ev, &prefixes) {
                                writer.write_all(ev.as_bytes()).await?;
                                writer.write_all(b"\n").await?;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => return Ok(()),
                    },
                }
            },
        }
    }
}

fn subscribe_request(cmd: &str) -> Option<Request> {
    let req = serde_json::from_str::<Request>(cmd).ok()?;
    (req.method == "subscribe").then_some(req)
}

/// The prefixes a `subscribe` request wants; an absent or empty list means every
/// ryogami event (what the wall-ui sends).
fn subscribe_prefixes(req: &Request) -> Vec<String> {
    let listed: Vec<String> = req
        .params
        .get("prefixes")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if listed.is_empty() { vec!["ryogami.".to_string()] } else { listed }
}

fn event_matches(ev: &str, prefixes: &[String]) -> bool {
    match serde_json::from_str::<Event>(ev) {
        Ok(e) => prefixes.iter().any(|p| e.event.starts_with(p.as_str())),
        Err(_) => false,
    }
}

/// Stream one topic to a subscriber until it disconnects or half-closes. The
/// retained frame is sent first, then a fresh frame on every change; further
/// client input (or EOF) ends the stream (mirrors ryoku's serveSubscription).
async fn serve_subscription(
    mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    mut writer: tokio::net::unix::OwnedWriteHalf,
    state: &SharedState,
    name: &str,
) -> anyhow::Result<()> {
    let Some(topic) = state.topics.get(name) else {
        writer.write_all(format!("err unknown topic: {name}\n").as_bytes()).await?;
        return Ok(());
    };
    let (last, mut rx) = topic.subscribe().await;
    if let Some(frame) = last {
        writer.write_all(frame.as_bytes()).await?;
        writer.write_all(b"\n").await?;
    }
    let mut scratch = Vec::new();
    loop {
        tokio::select! {
            recv = rx.recv() => match recv {
                Ok(frame) => {
                    writer.write_all(frame.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            read = reader.read_until(b'\n', &mut scratch) => match read {
                Ok(0) | Err(_) => break,
                Ok(_) => scratch.clear(),
            },
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn file_removed_payload_contract() {
        let s = file_removed_payload("a/b.webp", &watcher::FileType::Static);
        assert_eq!(s["name"], "a/b.webp");
        assert_eq!(s["type"], "static");

        let v = file_removed_payload("clip.mp4", &watcher::FileType::Video);
        assert_eq!(v["type"], "video");
    }

    #[test]
    fn folder_removed_payload_contract() {
        let p = folder_removed_payload("pack", &["pack/a.webp".to_string(), "pack/b.webp".to_string()]);
        assert_eq!(p["prefix"], "pack");
        assert_eq!(p["names"].as_array().unwrap().len(), 2);
        assert_eq!(p["names"][0], "pack/a.webp");
    }

    #[test]
    fn we_added_payload_contract() {
        let p = we_added_payload("123", &PathBuf::from("/we/123"));
        assert_eq!(p["we_id"], "123");
        assert_eq!(p["we_dir"], "/we/123");
    }

    #[test]
    fn we_removed_payload_contract() {
        let p = we_removed_payload("123");
        assert_eq!(p["we_id"], "123");
    }
}
