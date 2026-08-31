//! Wall apply's bridge to the in-process renderer.
//!
//! Ryogami renders the BACKGROUND surface itself (`crate::render` owns a shared
//! EGL/GL context on its own thread); this module translates the wall apply path
//! into renderer commands. There are no `ryogami-paper` child processes and no
//! `paper.ready` handshake any more: a static apply shows the frame then releases
//! GL to idle, a video apply starts a livewall, and switching engines tears the
//! surfaces down.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex as StdMutex, OnceLock};

use ryogami_paper::Source;

use crate::config::{self, Config};
use crate::render;

/// Serializes wall applies so overlapping `set`s do not race on the renderer.
struct PaperManager {
    apply_lock: tokio::sync::Mutex<()>,
    preheat: StdMutex<HashSet<String>>,
}

fn manager() -> &'static PaperManager {
    static M: OnceLock<PaperManager> = OnceLock::new();
    M.get_or_init(|| PaperManager {
        apply_lock: tokio::sync::Mutex::new(()),
        preheat: StdMutex::new(HashSet::new()),
    })
}

pub(super) fn apply_lock() -> &'static tokio::sync::Mutex<()> {
    &manager().apply_lock
}

/// Resolve the optimized (webp/transcoded) variant the renderer should decode.
fn render_source(path: &str) -> PathBuf {
    PathBuf::from(crate::wall::optimized::optimized_or(path))
}

fn normalized(outputs: &[String]) -> Vec<String> {
    if outputs.is_empty() {
        vec!["*".to_string()]
    } else {
        outputs.to_vec()
    }
}

/// Show a static wallpaper on each target output, then release GL to idle: a
/// settled static desktop holds no decoder or GL context, only the committed
/// buffer.
pub(super) async fn show_static(outputs: &[String], path: &str, fill: config::FillMode) {
    let Some(handle) = render::handle() else {
        return;
    };
    let tier = render::current_tier();
    let pfill = render::paper_fill(fill);
    let src = render_source(path);
    for out in normalized(outputs) {
        handle.show(&out, Source::Static(src.clone()), tier, pfill, true, 80);
    }
    handle.idle_release();
}

/// Show a livewall on each target output. `entries` is `(connector, mute, volume)`.
pub(super) async fn show_video(entries: &[(String, bool, u32)], path: &str, fill: config::FillMode) {
    let Some(handle) = render::handle() else {
        return;
    };
    let tier = render::current_tier();
    let pfill = render::paper_fill(fill);
    let src = render_source(path);
    for (out, mute, volume) in entries {
        handle.show(out, Source::Video(src.clone()), tier, pfill, *mute, *volume);
    }
}

/// Tear down every output surface (used when another engine — external, KDE,
/// awww, WE — takes over rendering).
pub(super) async fn stop_all() {
    if let Some(handle) = render::handle() {
        handle.stop_all();
    }
}

/// Tear down the given output surfaces (`"*"` = all).
pub(super) async fn stop_outputs(targets: &[String]) {
    let Some(handle) = render::handle() else {
        return;
    };
    if targets.iter().any(|t| t == "*") {
        handle.stop_all();
    } else {
        for t in targets {
            handle.stop_output(t);
        }
    }
}

/// Lifecycle hooks kept for the wall overview toggle. Prewarming a GL persist was
/// an out-of-process optimization; the in-process renderer needs no warmup, so
/// these are intentionally empty.
pub async fn on_wall_show(_config: &Config) {}

pub async fn on_wall_hide() {}

fn preheat_inflight() -> &'static StdMutex<HashSet<String>> {
    &manager().preheat
}

/// Warm the OS page cache for a wallpaper file so the next apply decodes fast.
pub fn preheat(path: &str) {
    if path.is_empty() {
        return;
    }
    let key = path.to_string();
    {
        let mut set = preheat_inflight().lock().unwrap();
        if !set.insert(key.clone()) {
            return;
        }
    }
    tokio::spawn(async move {
        let start = std::time::Instant::now();
        let bytes = tokio::fs::read(&key).await.map_or(0, |b| b.len());
        let dur_ms = start.elapsed().as_millis() as u64;
        tracing::debug!(path = %key, bytes, dur_ms, "wall.preheat done");
        preheat_inflight().lock().unwrap().remove(&key);
    });
}
