//! Wallpaper depth: the current wallpaper's subject cut out to a transparent PNG,
//! drawn in front of the desktop widgets. Generation is slow, so it runs on a
//! coalescing off-frame worker; cutouts are the user's, kept in ~/Pictures/Depth
//! and reused, never a hidden cache. Ported from ryoku's Go `ipc/depth.go`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::warn;

use crate::server::SharedState;
use crate::util::{CommandRunner, CommandSpec};
use crate::wall::VIDEO_EXTS;

const DEFAULT_MODEL: &str = "u2netp";

// --- config ------------------------------------------------------------------

#[derive(Debug, Clone)]
struct DepthSettings {
    enabled: bool,
    model: String,
    alpha_matting: bool,
}

impl Default for DepthSettings {
    fn default() -> Self {
        Self { enabled: false, model: DEFAULT_MODEL.to_string(), alpha_matting: false }
    }
}

#[derive(Deserialize)]
struct DepthConfigFile {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    model: String,
    #[serde(default, rename = "alphaMatting")]
    alpha_matting: bool,
}

/// Read ~/.config/ryoku/depth.json; only enabled/model/alphaMatting matter to the
/// daemon (feather/lift/shadow/front belong to the QML compositor).
fn depth_config() -> DepthSettings {
    let mut out = DepthSettings::default();
    let Some(dir) = ryoku_config_dir() else { return out };
    let Ok(bytes) = std::fs::read(dir.join("depth.json")) else { return out };
    let Ok(parsed) = serde_json::from_slice::<DepthConfigFile>(&bytes) else { return out };
    out.enabled = parsed.enabled;
    out.alpha_matting = parsed.alpha_matting;
    if !parsed.model.is_empty() {
        out.model = parsed.model;
    }
    out
}

/// `~/.config/ryoku` (honouring `XDG_CONFIG_HOME`), the dir Ryoku Settings write.
/// `None` only when the home dir is unknowable. Mirrors Go's `ryokuConfigDir`.
fn ryoku_config_dir() -> Option<PathBuf> {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(dir) if !dir.is_empty() => Some(PathBuf::from(dir).join("ryoku")),
        _ => std::env::var("HOME")
            .ok()
            .filter(|h| !h.is_empty())
            .map(|h| PathBuf::from(h).join(".config").join("ryoku")),
    }
}

// --- paths + engine ----------------------------------------------------------

/// `ryoku-depth`: on PATH once packaged, but a dev run reaches it under
/// RYOKU_SHELL_DIR where PATH does not.
fn depth_bin() -> String {
    if let Ok(dir) = std::env::var("RYOKU_SHELL_DIR")
        && !dir.is_empty()
    {
        let p = PathBuf::from(&dir).join("scripts").join("ryoku-depth");
        if p.is_file() {
            return p.to_string_lossy().into_owned();
        }
    }
    "ryoku-depth".to_string()
}

async fn depth_engine_available(runner: &dyn CommandRunner) -> bool {
    matches!(
        runner.run(CommandSpec::new(depth_bin()).args(["check"])).await,
        Ok(o) if o.status.success()
    )
}

fn depth_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Pictures").join("Depth")
}

fn depth_out(source: &str) -> PathBuf {
    let stem = Path::new(source).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    depth_dir().join(format!("{stem}-depth.png"))
}

fn depth_index_path() -> PathBuf {
    depth_dir().join(".index.json")
}

fn file_mod_time(p: &Path) -> i64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

// --- reuse index -------------------------------------------------------------

/// A cutout is reused only for the same source, model, and edge (matting) setting,
/// so returning to a wallpaper never shows a stale cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepthMeta {
    pub source: String,
    pub model: String,
    #[serde(rename = "alphaMatting")]
    pub alpha_matting: bool,
}

/// Reuse index at `~/Pictures/Depth/.index.json`, keyed by the cutout output path.
pub type DepthIndex = BTreeMap<String, DepthMeta>;

fn load_depth_index() -> DepthIndex {
    std::fs::read(depth_index_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_depth_index(idx: &DepthIndex) {
    if let Ok(b) = serde_json::to_vec_pretty(idx) {
        let _ = std::fs::write(depth_index_path(), b);
    }
}

/// A saved cutout at `out` is reusable when its index entry matches the request
/// and the file is at least as new as its source.
#[must_use]
pub fn depth_reusable(idx: &DepthIndex, source: &str, model: &str, matting: bool, out: &Path) -> bool {
    let key = out.to_string_lossy();
    let Some(m) = idx.get(key.as_ref()) else { return false };
    if m.source != source || m.model != model || m.alpha_matting != matting {
        return false;
    }
    let ot = file_mod_time(out);
    ot > 0 && ot >= file_mod_time(Path::new(source))
}

// --- worker handle -----------------------------------------------------------

/// Coalescing off-frame depth worker handle, stored on `SharedState`. Mirrors the
/// Go daemon's depthSig/depthForce/depthBusy: a burst of switches collapses into
/// one regeneration, and `busy` drives the Settings progress bar.
#[derive(Clone)]
pub struct DepthHandle {
    tx: mpsc::Sender<()>,
    force: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
}

impl DepthHandle {
    /// Build the handle and the receiver the worker task drains.
    #[must_use]
    pub fn new() -> (DepthHandle, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel(1);
        let handle = DepthHandle {
            tx,
            force: Arc::new(AtomicBool::new(false)),
            busy: Arc::new(AtomicBool::new(false)),
        };
        (handle, rx)
    }

    /// Coalesce a switch into one regeneration. `force` marks a pending forced
    /// regenerate (enable / model change / explicit refresh).
    pub fn schedule(&self, force: bool) {
        if force {
            self.force.store(true, Ordering::SeqCst);
        }
        let _ = self.tx.try_send(());
    }

    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }
}

// --- worker ------------------------------------------------------------------

/// Drain the coalescing signal; each wake regenerates (or clears) depth once.
pub async fn depth_worker(state: SharedState, mut rx: mpsc::Receiver<()>) {
    while rx.recv().await.is_some() {
        let force = state.depth.force.swap(false, Ordering::SeqCst);
        apply_depth(&state, &depth_config(), force).await;
    }
}

/// Depth disabled or the engine absent: drop every cutout. Otherwise regenerate.
/// `ryoku-depth check` failing (no rembg on this box) is the graceful no-op path.
async fn apply_depth(state: &SharedState, cfg: &DepthSettings, force: bool) {
    if !cfg.enabled || !depth_engine_available(&*state.runner).await {
        state.wall_surface.clear_depth().await;
        return;
    }
    generate_depth(state, &cfg.model, cfg.alpha_matting, force).await;
}

/// Reuse each on-screen wallpaper's saved cutout or regenerate it, then publish.
/// The slow helper runs off the surface lock.
async fn generate_depth(state: &SharedState, model: &str, matting: bool, force: bool) {
    let targets = depth_targets(state).await;
    if targets.is_empty() {
        return;
    }
    state.depth.busy.store(true, Ordering::SeqCst);
    if let Err(e) = std::fs::create_dir_all(depth_dir()) {
        warn!("depth: {e}");
        state.depth.busy.store(false, Ordering::SeqCst);
        return;
    }
    let mut idx = load_depth_index();
    let mut changed = false;
    for (slot, source) in targets {
        let out = depth_out(&source);
        let out_str = out.to_string_lossy().into_owned();
        if force || !depth_reusable(&idx, &source, model, matting, &out) {
            let mut args =
                vec!["cutout".to_string(), source.clone(), out_str.clone(), "--model".to_string(), model.to_string()];
            if matting {
                args.push("--alpha-matting".to_string());
            }
            match state.runner.run(CommandSpec::new(depth_bin()).args(args)).await {
                Ok(o) if o.status.success() => {}
                _ => {
                    warn!("depth: cutout failed for {source}");
                    continue;
                }
            }
            idx.insert(
                out_str.clone(),
                DepthMeta { source: source.clone(), model: model.to_string(), alpha_matting: matting },
            );
            changed = true;
        }
        let rev = file_mod_time(&out);
        state.wall_surface.set_depth(&slot, &source, &out_str, rev).await;
    }
    if changed {
        save_depth_index(&idx);
    }
    state.depth.busy.store(false, Ordering::SeqCst);
}

/// Pair each slot ("" = default) with its ORIGINAL wallpaper path — a still that
/// exists on disk. The cutout is named after the wallpaper and reused on return;
/// videos are skipped, so a live wallpaper carries empty depth.
async fn depth_targets(state: &SharedState) -> Vec<(String, String)> {
    let frame = state.wall_surface.snapshot().await;
    let mut out = Vec::new();
    if is_still_file(&frame.default.path) {
        out.push((String::new(), frame.default.path.clone()));
    }
    for (name, e) in &frame.outputs {
        if is_still_file(&e.path) {
            out.push((name.clone(), e.path.clone()));
        }
    }
    out
}

/// The current default cutout on disk, for the Settings preview; empty when none.
pub async fn default_cutout(state: &SharedState) -> String {
    let def = state.wall_surface.snapshot().await.default.path;
    if def.is_empty() || is_video(&def) {
        return String::new();
    }
    let out = depth_out(&def);
    if out.is_file() { out.to_string_lossy().into_owned() } else { String::new() }
}

fn is_still_file(p: &str) -> bool {
    !p.is_empty() && !is_video(p) && Path::new(p).is_file()
}

fn is_video(p: &str) -> bool {
    let lower = p.to_lowercase();
    VIDEO_EXTS.iter().any(|e| lower.ends_with(&format!(".{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::test_state;
    use crate::util::FakeRunner;

    #[test]
    fn reuse_matches_source_model_matting_and_invalidates_on_change() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("wall.png");
        std::fs::write(&source, b"src").unwrap();
        let out = dir.path().join("wall-depth.png");
        std::fs::write(&out, b"cut").unwrap();
        let src = source.to_string_lossy().into_owned();

        let mut idx = DepthIndex::new();
        idx.insert(
            out.to_string_lossy().into_owned(),
            DepthMeta { source: src.clone(), model: "u2netp".into(), alpha_matting: false },
        );

        // A matching source+model+matting reuses the saved cutout.
        assert!(depth_reusable(&idx, &src, "u2netp", false, &out));
        // A model change invalidates it (a returning wallpaper never shows a stale cut).
        assert!(!depth_reusable(&idx, &src, "isnet", false, &out));
        // A matting (edge) change invalidates it.
        assert!(!depth_reusable(&idx, &src, "u2netp", true, &out));
        // An unindexed output is not reusable.
        assert!(!depth_reusable(&idx, &src, "u2netp", false, &dir.path().join("none-depth.png")));
    }

    #[test]
    fn reuse_rejects_a_cutout_older_than_its_source() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("wall-depth.png");
        std::fs::write(&out, b"cut").unwrap();
        // Source written after (thus newer than) the cutout: reuse would be stale.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let source = dir.path().join("wall.png");
        std::fs::write(&source, b"src").unwrap();
        let src = source.to_string_lossy().into_owned();

        let mut idx = DepthIndex::new();
        idx.insert(
            out.to_string_lossy().into_owned(),
            DepthMeta { source: src.clone(), model: "u2netp".into(), alpha_matting: false },
        );
        assert!(!depth_reusable(&idx, &src, "u2netp", false, &out));
    }

    /// `ryoku-depth check` failing (no engine on this box) must no-op gracefully:
    /// clear depth, never shell out `cutout`.
    #[tokio::test]
    async fn engine_unavailable_clears_depth_without_cutout() {
        let fake = Arc::new(FakeRunner::new());
        fake.on("ryoku-depth", &["check"], b"", 1);
        let mut state = test_state().state;
        state.runner = fake.clone();
        state.wall_surface.show("/tmp/does-not-matter.png", "Cover", None).await;

        apply_depth(&state, &DepthSettings { enabled: true, model: "u2netp".into(), alpha_matting: false }, false).await;

        assert!(
            fake.calls.lock().unwrap().iter().all(|c| !c.args.iter().any(|a| a == "cutout")),
            "engine-unavailable path must not run cutout"
        );
        let frame = state.wall_surface.snapshot().await;
        assert_eq!(frame.default.depth, "");
        assert_eq!(frame.default.depth_rev, 0);
    }

    #[tokio::test]
    async fn disabled_config_clears_depth_without_engine_check() {
        let fake = Arc::new(FakeRunner::new());
        let mut state = test_state().state;
        state.runner = fake.clone();

        apply_depth(&state, &DepthSettings { enabled: false, ..Default::default() }, false).await;

        assert_eq!(fake.call_count(), 0, "disabled depth must not probe the engine");
    }
}
