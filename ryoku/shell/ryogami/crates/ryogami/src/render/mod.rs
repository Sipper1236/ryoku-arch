//! The daemon's owner of the in-process wallpaper renderer.
//!
//! The renderer (`ryogami_paper::Renderer`) runs on its own thread because the
//! wayland queue and EGL context are thread-bound; this module holds the
//! process-wide [`RenderHandle`] and translates daemon types (the wire
//! `ResourceTier`, `config::FillMode`) into the renderer's. The thread is
//! spawned lazily on first use and never in a headless/test context.
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU8, Ordering};

use ryogami_paper::{FillMode as PaperFill, RenderHandle, ResourceTier as PaperTier, spawn_renderer};

use crate::config::FillMode as ConfigFill;
use crate::server::ResourceTier;

/// The render fidelity the renderer applies, mirroring `SharedState.resource_tier`
/// (set by `wallpaper resource`). Kept here so the wall apply path can read it
/// without threading `SharedState` through every call. Task 6 persists it.
static TIER: AtomicU8 = AtomicU8::new(TIER_MEDIUM);
const TIER_LOW: u8 = 0;
const TIER_MEDIUM: u8 = 1;
const TIER_HIGH: u8 = 2;

/// No display, or an explicit headless run: the renderer must not connect to a
/// compositor (it would hijack the live wallpaper). Callers fall back to
/// publishing the topic only.
#[must_use]
pub fn is_headless() -> bool {
    std::env::var_os("RYOGAMI_HEADLESS").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_none()
}

/// The process-wide render handle, or `None` when headless. The render thread is
/// spawned the first time this is read.
static HANDLE: LazyLock<Option<RenderHandle>> = LazyLock::new(|| {
    if is_headless() {
        tracing::info!("headless: in-process renderer disabled (topic still published)");
        None
    } else {
        tracing::info!("starting in-process wallpaper renderer");
        Some(spawn_renderer())
    }
});

pub fn handle() -> Option<&'static RenderHandle> {
    HANDLE.as_ref()
}

/// Record the resource tier chosen by `wallpaper resource`.
pub fn set_tier(tier: ResourceTier) {
    let v = match tier {
        ResourceTier::Low => TIER_LOW,
        ResourceTier::Medium => TIER_MEDIUM,
        ResourceTier::High => TIER_HIGH,
    };
    TIER.store(v, Ordering::Relaxed);
}

/// The renderer's current resource tier.
#[must_use]
pub fn current_tier() -> PaperTier {
    match TIER.load(Ordering::Relaxed) {
        TIER_LOW => PaperTier::Low,
        TIER_HIGH => PaperTier::High,
        _ => PaperTier::Medium,
    }
}

/// Map the config fill mode to the renderer's.
#[must_use]
pub fn paper_fill(fill: ConfigFill) -> PaperFill {
    match fill {
        ConfigFill::Fill => PaperFill::Fill,
        ConfigFill::Fit => PaperFill::Fit,
        ConfigFill::Stretch => PaperFill::Stretch,
        ConfigFill::Center => PaperFill::Center,
        ConfigFill::Tile => PaperFill::Tile,
    }
}
