//! Ryogami's in-process wallpaper renderer.
//!
//! Forked from `skwd-paper`: the wayland/EGL/GL, ffmpeg decode and cpal audio
//! logic is reused intact; only the harness is refactored from a per-output
//! child process into one library `Renderer` the daemon drives on a shared EGL
//! context. The `ryogami-paper` binary (`main.rs`) is kept as a standalone
//! renderer for debugging; the daemon no longer spawns it.

#![allow(clippy::manual_c_str_literals)]

pub mod audio;
pub mod fill_mode;
pub mod image_paper;
pub mod ipc;
pub mod render;
pub mod renderer;
pub mod transition_paper;
pub mod video_source;
pub mod watchdog;
pub mod wayland;

pub use fill_mode::FillMode;
pub use renderer::{RenderCommand, RenderHandle, Renderer, ResourceTier, Source, spawn_renderer};
