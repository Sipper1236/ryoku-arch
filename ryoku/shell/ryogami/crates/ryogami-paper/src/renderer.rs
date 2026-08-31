//! In-process wallpaper renderer.
//!
//! One process, one shared EGL/GL context ([`crate::render::EglCore`]), one
//! wlr-layer-shell BACKGROUND surface per output. Static wallpapers paint via
//! `wl_shm` + `wp_viewport` (no GL held); livewalls paint via the shared GL
//! context and an ffmpeg decode loop paced by frame callbacks. The daemon drives
//! it through a [`RenderHandle`] over a command channel; the renderer itself runs
//! on its own thread because the wayland queue and EGL context are thread-bound.
//!
//! Footprint discipline (see the design spec): a settled static desktop holds no
//! decoder and no GL context — [`Renderer::idle_release`] drops them and the
//! compositor keeps displaying the last committed buffer.

use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use anyhow::{Context, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, EventQueue, Proxy, QueueHandle,
    globals::{GlobalList, registry_queue_init},
    protocol::{wl_output::WlOutput, wl_shm, wl_surface::WlSurface},
};
use wayland_protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};

use crate::fill_mode::{FillMode, apply_fill_mode};
use crate::image_paper::decode_image;
use crate::render::{EglCore, OutputBlitter, wayland_display_ptr};
use crate::video_source::VideoSource;

// ---------------------------------------------------------------------------
// Pure policy types (unit-tested without EGL/wayland)
// ---------------------------------------------------------------------------

/// Render fidelity. `Low`/`Medium` render into a smaller client buffer that
/// `wp_viewport` upscales to the output; `High` renders at native size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ResourceTier {
    Low,
    #[default]
    Medium,
    High,
}

impl ResourceTier {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    /// The longest client-buffer edge this tier renders. `None` = native (no cap).
    #[must_use]
    pub fn max_edge(self) -> Option<u32> {
        match self {
            Self::Low => Some(1280),
            Self::Medium => Some(1920),
            Self::High => None,
        }
    }

    /// Whether this tier relies on `wp_viewport` to upscale the client buffer.
    #[must_use]
    pub fn upscales(self) -> bool {
        self.max_edge().is_some()
    }

    /// The client render-buffer size for an output of `native_w`x`native_h`.
    /// Aspect ratio is preserved; a buffer already within the cap is unchanged.
    #[must_use]
    pub fn render_size(self, native_w: u32, native_h: u32) -> (u32, u32) {
        let w = native_w.max(1);
        let h = native_h.max(1);
        match self.max_edge() {
            None => (w, h),
            Some(cap) => {
                let longest = w.max(h);
                if longest <= cap {
                    return (w, h);
                }
                let scale = f64::from(cap) / f64::from(longest);
                let sw = ((f64::from(w) * scale).round() as u32).max(1);
                let sh = ((f64::from(h) * scale).round() as u32).max(1);
                (sw, sh)
            }
        }
    }
}

/// A wallpaper source, classified by extension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Static(PathBuf),
    Video(PathBuf),
}

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "tiff", "tif", "gif", "avif"];

impl Source {
    /// Classify a path as image (`Static`) or video (`Video`) by extension.
    /// An unknown or missing extension is treated as video (the decode path is
    /// the tolerant one).
    #[must_use]
    pub fn classify(path: &str) -> Self {
        let is_image = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .is_some_and(|e| IMAGE_EXTS.contains(&e.as_str()));
        if is_image {
            Self::Static(PathBuf::from(path))
        } else {
            Self::Video(PathBuf::from(path))
        }
    }

    #[must_use]
    pub fn is_video(&self) -> bool {
        matches!(self, Self::Video(_))
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Static(p) | Self::Video(p) => p,
        }
    }
}

/// What an output should display, plus how.
#[derive(Clone, Debug)]
pub struct Desired {
    pub source: Source,
    pub tier: ResourceTier,
    pub fill: FillMode,
    pub mute: bool,
    pub volume: u32,
}

/// Per-connector desired state — the pure bookkeeping the renderer consults to
/// decide what to paint and whether any live decoder is still needed. `"*"` is
/// the broadcast default; a named override wins for that connector.
#[derive(Default)]
pub struct SurfaceLedger {
    default: Option<Desired>,
    per_output: HashMap<String, Desired>,
}

impl SurfaceLedger {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record desired state. `"*"` sets the broadcast default and drops every
    /// per-output override (a broadcast replaces the scene, mirroring the
    /// wallpaper topic's `show`).
    pub fn show(&mut self, connector: &str, desired: Desired) {
        if connector == "*" {
            self.default = Some(desired);
            self.per_output.clear();
        } else {
            self.per_output.insert(connector.to_string(), desired);
        }
    }

    /// Forget desired state. `"*"` clears everything.
    pub fn stop(&mut self, connector: &str) {
        if connector == "*" {
            self.default = None;
            self.per_output.clear();
        } else {
            self.per_output.remove(connector);
        }
    }

    #[must_use]
    pub fn desired_for(&self, connector: &str) -> Option<&Desired> {
        self.per_output.get(connector).or(self.default.as_ref())
    }

    /// Whether any output would show a livewall (so the GL context must stay up).
    #[must_use]
    pub fn any_video(&self) -> bool {
        self.default.as_ref().is_some_and(|d| d.source.is_video())
            || self.per_output.values().any(|d| d.source.is_video())
    }

    /// Bookkeeping for [`Renderer::idle_release`]: returns whether the GL context
    /// can be dropped (true iff no output wants a livewall).
    pub fn idle_release(&self) -> bool {
        !self.any_video()
    }
}

// ---------------------------------------------------------------------------
// Command channel + handle (the daemon-facing API)
// ---------------------------------------------------------------------------

/// A command for the render thread.
pub enum RenderCommand {
    Show {
        connector: String,
        source: Source,
        tier: ResourceTier,
        fill: FillMode,
        mute: bool,
        volume: u32,
    },
    StopOutput {
        connector: String,
    },
    StopAll,
    IdleRelease,
    SetAudio {
        connector: String,
        mute: bool,
        volume: u32,
    },
    Shutdown,
}

/// A cross-thread wakeup: writing a byte interrupts the render thread's `poll`.
struct Waker {
    fd: OwnedFd,
}

impl Waker {
    fn wake(&self) {
        let byte = [1u8];
        // Best-effort; a full non-blocking pipe already means "wake pending".
        unsafe { libc::write(self.fd.as_raw_fd(), byte.as_ptr().cast(), 1) };
    }
}

/// Send-and-Sync handle the daemon holds to drive the renderer.
#[derive(Clone)]
pub struct RenderHandle {
    tx: Sender<RenderCommand>,
    waker: Arc<Waker>,
}

impl RenderHandle {
    fn send(&self, cmd: RenderCommand) {
        if self.tx.send(cmd).is_ok() {
            self.waker.wake();
        }
    }

    pub fn show(
        &self,
        connector: &str,
        source: Source,
        tier: ResourceTier,
        fill: FillMode,
        mute: bool,
        volume: u32,
    ) {
        self.send(RenderCommand::Show {
            connector: connector.to_string(),
            source,
            tier,
            fill,
            mute,
            volume,
        });
    }

    pub fn stop_output(&self, connector: &str) {
        self.send(RenderCommand::StopOutput {
            connector: connector.to_string(),
        });
    }

    pub fn stop_all(&self) {
        self.send(RenderCommand::StopAll);
    }

    pub fn idle_release(&self) {
        self.send(RenderCommand::IdleRelease);
    }

    pub fn set_audio(&self, connector: &str, mute: bool, volume: u32) {
        self.send(RenderCommand::SetAudio {
            connector: connector.to_string(),
            mute,
            volume,
        });
    }

    pub fn shutdown(&self) {
        self.send(RenderCommand::Shutdown);
    }
}

fn make_wake_pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as RawFd; 2];
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Spawn the render thread and return a handle. The thread connects to the
/// compositor; if that fails (no display), it logs and exits and the handle's
/// commands become no-ops. NEVER call this in a headless/test context — that is
/// the daemon's responsibility (see `crate::render::handle`).
#[must_use]
pub fn spawn_renderer() -> RenderHandle {
    let (tx, rx) = mpsc::channel::<RenderCommand>();
    let (wake_read, wake_write) = make_wake_pipe().expect("wake pipe");
    std::thread::Builder::new()
        .name("ryogami-render".into())
        .spawn(move || match Renderer::new(wake_read) {
            Ok(renderer) => renderer.run(rx),
            Err(e) => tracing::error!(error = %e, "renderer init failed; wallpaper rendering disabled"),
        })
        .expect("spawn render thread");
    RenderHandle {
        tx,
        waker: Arc::new(Waker { fd: wake_write }),
    }
}

// ---------------------------------------------------------------------------
// The renderer + its wayland application state
// ---------------------------------------------------------------------------

/// Live content bound to one output surface.
enum Content {
    /// No buffer attached yet (or torn down); the compositor shows the last
    /// committed buffer, if any.
    Empty,
    /// A settled static wallpaper: an `wl_shm` buffer kept resident so the
    /// compositor can hold it. No GL.
    // The payload is a resident keepalive (Drop-only); nothing reads it.
    #[allow(dead_code)]
    Static(StaticBuf),
    /// A livewall: the shared context renders each frame into the source's FBO
    /// and blits it to this output's window surface.
    Video(VideoContent),
}

/// Keeps the `wl_shm` pool + slot alive so the compositor can hold the committed
/// static buffer with no client GL. Nothing reads these fields; they exist only
/// to defer their `Drop`.
#[allow(dead_code)]
struct StaticBuf {
    _pool: SlotPool,
    _keep: smithay_client_toolkit::shm::slot::Buffer,
}

struct VideoContent {
    source: VideoSource,
    blitter: OutputBlitter,
}

struct OutputSurface {
    output: WlOutput,
    name: String,
    surface: WlSurface,
    layer: ZwlrLayerSurfaceV1,
    viewport: WpViewport,
    width: u32,
    height: u32,
    configured: bool,
    realized_wh: Option<(u32, u32)>,
    content: Content,
    frame_pending: bool,
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    compositor_state: CompositorState,
    shm: Shm,
    layer_shell: ZwlrLayerShellV1,
    viewporter: WpViewporter,
    qh: QueueHandle<App>,
    egl: Option<EglCore>,
    outputs: Vec<OutputSurface>,
    ledger: SurfaceLedger,
}

/// The in-process wallpaper renderer. Constructed and driven on one thread.
pub struct Renderer {
    #[allow(dead_code)]
    conn: Connection,
    event_queue: EventQueue<App>,
    app: App,
    wake_fd: OwnedFd,
}

impl Renderer {
    /// Connect to the compositor and set up the shared state. Does not create any
    /// surface until [`Renderer::show_output`] is called.
    pub fn new(wake_fd: OwnedFd) -> Result<Self> {
        let conn = Connection::connect_to_env().context("wayland connect")?;
        let (globals, mut event_queue): (GlobalList, EventQueue<App>) =
            registry_queue_init(&conn).context("registry_queue_init")?;
        let qh = event_queue.handle();

        let registry_state = RegistryState::new(&globals);
        let output_state = OutputState::new(&globals, &qh);
        let compositor_state =
            CompositorState::bind(&globals, &qh).context("compositor not available")?;
        let layer_shell: ZwlrLayerShellV1 = globals
            .bind(&qh, 1..=4, ())
            .context("zwlr_layer_shell_v1 not available")?;
        let viewporter: WpViewporter = globals
            .bind(&qh, 1..=1, ())
            .context("wp_viewporter not available")?;
        let shm = Shm::bind(&globals, &qh).context("wl_shm not available")?;

        let mut app = App {
            registry_state,
            output_state,
            compositor_state,
            shm,
            layer_shell,
            viewporter,
            qh: qh.clone(),
            egl: None,
            outputs: Vec::new(),
            ledger: SurfaceLedger::new(),
        };
        event_queue.roundtrip(&mut app)?;

        Ok(Self {
            conn,
            event_queue,
            app,
            wake_fd,
        })
    }

    // --- the interface named in the task (thread-local) --------------------

    /// Show `source` on `connector` (`"*"` = all outputs) at `tier`, with default
    /// fill and muted audio. Richer control (fill/audio) goes through the command
    /// channel; see [`RenderHandle::show`].
    pub fn show_output(&mut self, connector: &str, source: Source, tier: ResourceTier) {
        self.apply_show(connector, source, tier, FillMode::default(), true, 80);
    }

    /// Tear down a connector's surface + live decoder/GL entirely (used when
    /// another engine takes over the output).
    pub fn stop_output(&mut self, connector: &str) {
        self.app.stop_output(connector);
        let _ = self.event_queue.flush();
    }

    /// Drop the decoder + video GL resources once the desktop is a settled
    /// static; the compositor keeps the last committed buffer. See the module
    /// docs for exactly what stays resident.
    pub fn idle_release(&mut self) {
        self.app.idle_release();
    }

    // --- the command path (from the daemon) --------------------------------

    fn apply_show(
        &mut self,
        connector: &str,
        source: Source,
        tier: ResourceTier,
        fill: FillMode,
        mute: bool,
        volume: u32,
    ) {
        self.app.show(
            connector,
            Desired {
                source,
                tier,
                fill,
                mute,
                volume,
            },
        );
        // A new surface commits at size 0x0 and needs a configure round-trip
        // before it can be painted.
        let _ = self.event_queue.roundtrip(&mut self.app);
    }

    fn handle_command(&mut self, cmd: RenderCommand) {
        match cmd {
            RenderCommand::Show {
                connector,
                source,
                tier,
                fill,
                mute,
                volume,
            } => self.apply_show(&connector, source, tier, fill, mute, volume),
            RenderCommand::StopOutput { connector } => self.stop_output(&connector),
            RenderCommand::StopAll => {
                self.app.stop_all();
                let _ = self.event_queue.flush();
            }
            RenderCommand::IdleRelease => self.idle_release(),
            RenderCommand::SetAudio {
                connector,
                mute,
                volume,
            } => self.app.set_audio(&connector, mute, volume),
            RenderCommand::Shutdown => {}
        }
    }

    /// The render-thread event loop: drain commands, dispatch wayland events
    /// (frame callbacks drive livewall frames), then block on the wayland socket
    /// and the wake pipe until either has work.
    pub fn run(mut self, rx: Receiver<RenderCommand>) {
        loop {
            loop {
                match rx.try_recv() {
                    Ok(RenderCommand::Shutdown) | Err(TryRecvError::Disconnected) => {
                        self.app.stop_all();
                        let _ = self.event_queue.flush();
                        return;
                    }
                    Ok(cmd) => self.handle_command(cmd),
                    Err(TryRecvError::Empty) => break,
                }
            }

            let _ = self.event_queue.flush();
            if let Err(e) = self.event_queue.dispatch_pending(&mut self.app) {
                tracing::error!(error = %e, "wayland dispatch failed; renderer exiting");
                return;
            }

            match self.event_queue.prepare_read() {
                None => continue,
                Some(guard) => {
                    let wl_fd = self.event_queue.as_fd().as_raw_fd();
                    poll_two(wl_fd, self.wake_fd.as_raw_fd());
                    drain_fd(self.wake_fd.as_raw_fd());
                    let _ = guard.read();
                }
            }
        }
    }
}

fn poll_two(wl_fd: RawFd, wake_fd: RawFd) {
    let mut fds = [
        libc::pollfd {
            fd: wl_fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wake_fd,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    // Infinite timeout: livewall frame callbacks keep the socket busy; a settled
    // static only wakes on a command (the pipe) or a compositor event.
    unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
}

fn drain_fd(fd: RawFd) {
    let mut buf = [0u8; 64];
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            break;
        }
    }
}

impl App {
    // --- scene management --------------------------------------------------

    fn show(&mut self, connector: &str, desired: Desired) {
        self.ledger.show(connector, desired);
        if connector == "*" {
            let outs: Vec<WlOutput> = self.output_state.outputs().collect();
            for out in outs {
                self.ensure_surface(&out);
            }
            for idx in 0..self.outputs.len() {
                self.apply_desired_to(idx);
            }
        } else if let Some(out) = self.find_output_by_name(connector) {
            let idx = self.ensure_surface(&out);
            if let Some(idx) = idx {
                self.apply_desired_to(idx);
            }
        }
    }

    fn stop_output(&mut self, connector: &str) {
        self.ledger.stop(connector);
        if connector == "*" {
            self.stop_all();
            return;
        }
        if let Some(idx) = self.outputs.iter().position(|s| s.name == connector) {
            self.destroy_surface(idx);
        }
    }

    fn stop_all(&mut self) {
        self.ledger.stop("*");
        while !self.outputs.is_empty() {
            self.destroy_surface(self.outputs.len() - 1);
        }
        self.drop_egl_if_idle();
    }

    fn set_audio(&mut self, connector: &str, mute: bool, volume: u32) {
        for out in &mut self.outputs {
            if connector != "*" && out.name != connector {
                continue;
            }
            if let Content::Video(v) = &mut out.content {
                v.source.set_mute(mute);
                v.source.set_volume(volume);
            }
        }
    }

    /// Drop every livewall decoder + video GL resource and, when no output still
    /// wants a livewall, the shared context too. Static `wl_shm` buffers and the
    /// wl surfaces stay resident; the compositor keeps showing them.
    fn idle_release(&mut self) {
        if !self.ledger.idle_release() {
            return;
        }
        self.release_all_video();
        self.drop_egl_if_idle();
    }

    fn release_all_video(&mut self) {
        if let Some(egl) = self.egl.as_ref() {
            let _ = egl.make_current_offscreen();
        }
        for out in &mut self.outputs {
            if let Content::Video(v) = &mut out.content {
                unsafe {
                    gl::DeleteFramebuffers(1, &v.source.fbo);
                    gl::DeleteTextures(1, &v.source.fbo_texture);
                }
                out.content = Content::Empty;
                out.frame_pending = false;
                out.realized_wh = None;
            }
        }
    }

    fn drop_egl_if_idle(&mut self) {
        let has_video = self
            .outputs
            .iter()
            .any(|s| matches!(s.content, Content::Video(_)));
        if !has_video {
            self.egl = None;
        }
    }

    fn find_output_by_name(&self, name: &str) -> Option<WlOutput> {
        self.output_state.outputs().find(|o| {
            self.output_state
                .info(o)
                .and_then(|i| i.name)
                .as_deref()
                == Some(name)
        })
    }

    /// Create the layer surface for `output` if absent; return its index.
    fn ensure_surface(&mut self, output: &WlOutput) -> Option<usize> {
        let info = self.output_state.info(output)?;
        let name = info.name.clone().unwrap_or_default();
        if let Some(idx) = self.outputs.iter().position(|s| s.name == name) {
            return Some(idx);
        }

        let surface = self.compositor_state.create_surface(&self.qh);
        let layer = self.layer_shell.get_layer_surface(
            &surface,
            Some(output),
            Layer::Background,
            "ryogami".to_string(),
            &self.qh,
            (),
        );
        layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(0, 0);
        let viewport = self.viewporter.get_viewport(&surface, &self.qh, ());
        surface.commit();

        tracing::info!(output = %name, "created layer surface");
        self.outputs.push(OutputSurface {
            output: output.clone(),
            name,
            surface,
            layer,
            viewport,
            width: 0,
            height: 0,
            configured: false,
            realized_wh: None,
            content: Content::Empty,
            frame_pending: false,
        });
        Some(self.outputs.len() - 1)
    }

    fn destroy_surface(&mut self, idx: usize) {
        if idx >= self.outputs.len() {
            return;
        }
        // Free the video GL first, while a context is current.
        if let Content::Video(v) = &self.outputs[idx].content
            && let Some(egl) = self.egl.as_ref()
        {
            let _ = egl.make_current_offscreen();
            unsafe {
                gl::DeleteFramebuffers(1, &v.source.fbo);
                gl::DeleteTextures(1, &v.source.fbo_texture);
            }
        }
        let out = self.outputs.remove(idx);
        out.viewport.destroy();
        out.layer.destroy();
        out.surface.destroy();
        // `out` drops here: any remaining `Content` GL/SHM is freed (the video
        // FBO/texture were already deleted above while a context was current).
    }

    /// Realize the ledger's desired content on output `idx` (if configured).
    fn apply_desired_to(&mut self, idx: usize) {
        let name = self.outputs[idx].name.clone();
        let Some(desired) = self.ledger.desired_for(&name).cloned() else {
            return;
        };
        let (sw, sh) = (self.outputs[idx].width, self.outputs[idx].height);
        if !self.outputs[idx].configured || sw == 0 || sh == 0 {
            return;
        }
        if self.outputs[idx].realized_wh == Some((sw, sh))
            && !matches!(self.outputs[idx].content, Content::Empty)
        {
            return;
        }
        match &desired.source {
            Source::Static(path) => self.realize_static(idx, path, desired.fill, desired.tier),
            Source::Video(path) => {
                self.realize_video(idx, path, desired.tier, desired.mute, desired.volume);
            }
        }
    }

    fn realize_static(&mut self, idx: usize, path: &Path, fill: FillMode, tier: ResourceTier) {
        // Switching away from a livewall: free its GL first.
        self.release_video_at(idx);

        let (sw, sh) = (self.outputs[idx].width, self.outputs[idx].height);
        let (rw, rh) = tier.render_size(sw, sh);
        let path_str = path.to_string_lossy();
        let (iw, ih, pixels) = match decode_image(&path_str) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(error = %e, path = %path_str, "static decode failed");
                return;
            }
        };
        let (bw, bh, buf_pixels) = apply_fill_mode(iw, ih, pixels, rw, rh, fill);
        let stride = (bw as i32) * 4;
        let pool_size = (stride as usize) * (bh as usize);
        let mut pool = match SlotPool::new(pool_size.max(4), &self.shm) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "SlotPool::new failed");
                return;
            }
        };
        let (keep, canvas) =
            match pool.create_buffer(bw as i32, bh as i32, stride, wl_shm::Format::Abgr8888) {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!(error = %e, "create_buffer failed");
                    return;
                }
            };
        let n = buf_pixels.len().min(canvas.len());
        canvas[..n].copy_from_slice(&buf_pixels[..n]);
        let buffer = keep.wl_buffer().clone();

        let out = &mut self.outputs[idx];
        out.viewport.set_source(0.0, 0.0, f64::from(bw), f64::from(bh));
        out.viewport.set_destination(sw as i32, sh as i32);
        out.surface.attach(Some(&buffer), 0, 0);
        out.surface.damage_buffer(0, 0, bw as i32, bh as i32);
        out.surface.commit();
        out.content = Content::Static(StaticBuf {
            _pool: pool,
            _keep: keep,
        });
        out.realized_wh = Some((sw, sh));
        out.frame_pending = false;
        tracing::info!(output = %out.name, w = bw, h = bh, "static committed");
    }

    fn realize_video(
        &mut self,
        idx: usize,
        path: &Path,
        tier: ResourceTier,
        mute: bool,
        volume: u32,
    ) {
        self.release_video_at(idx);

        let (sw, sh) = (self.outputs[idx].width, self.outputs[idx].height);
        let (rw, rh) = tier.render_size(sw, sh);

        if self.egl.is_none() {
            let dp = match wayland_display_ptr(&self.outputs[idx].surface) {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(error = %e, "wayland display ptr");
                    return;
                }
            };
            match EglCore::new(dp) {
                Ok(core) => self.egl = Some(core),
                Err(e) => {
                    tracing::error!(error = %e, "EGL core init failed");
                    return;
                }
            }
        }
        let egl = self.egl.as_ref().unwrap();
        if !egl.make_current_offscreen() {
            tracing::error!("make_current for video init failed");
            return;
        }
        let path_str = path.to_string_lossy();
        let mut source = match VideoSource::new(&path_str, rw, rh, mute) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, path = %path_str, "video source init failed");
                return;
            }
        };
        source.set_volume(volume);
        source.prime_first_frame(2000);
        source.set_pause(false);
        let blitter = match egl.make_blitter(&self.outputs[idx].surface, rw, rh) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(error = %e, "blitter create failed");
                return;
            }
        };
        egl.blit_texture_to(&blitter, source.fbo_texture);

        let out = &mut self.outputs[idx];
        out.viewport.set_source(0.0, 0.0, f64::from(rw), f64::from(rh));
        out.viewport.set_destination(sw as i32, sh as i32);
        out.content = Content::Video(VideoContent { source, blitter });
        out.realized_wh = Some((sw, sh));
        tracing::info!(output = %out.name, w = rw, h = rh, "livewall started");
        self.schedule_frame(idx);
    }

    /// Free a single output's livewall GL, leaving `Content::Empty`.
    fn release_video_at(&mut self, idx: usize) {
        if !matches!(self.outputs[idx].content, Content::Video(_)) {
            return;
        }
        if let Some(egl) = self.egl.as_ref() {
            let _ = egl.make_current_offscreen();
        }
        if let Content::Video(v) = &self.outputs[idx].content {
            unsafe {
                gl::DeleteFramebuffers(1, &v.source.fbo);
                gl::DeleteTextures(1, &v.source.fbo_texture);
            }
        }
        self.outputs[idx].content = Content::Empty;
        self.outputs[idx].frame_pending = false;
        self.outputs[idx].realized_wh = None;
    }

    fn schedule_frame(&mut self, idx: usize) {
        let out = &mut self.outputs[idx];
        if !matches!(out.content, Content::Video(_)) || out.frame_pending {
            return;
        }
        out.frame_pending = true;
        out.surface.frame(&self.qh, out.surface.clone());
        out.surface.commit();
    }

    fn render_video_frame(&mut self, idx: usize) {
        let Some(egl) = self.egl.as_ref() else {
            return;
        };
        let out = &mut self.outputs[idx];
        if let Content::Video(v) = &mut out.content {
            if !egl.make_current_offscreen() {
                return;
            }
            let _ = v.source.render_to_fbo();
            unsafe { gl::Finish() };
            egl.blit_texture_to(&v.blitter, v.source.fbo_texture);
        }
    }
}

// ---------------------------------------------------------------------------
// wayland handlers
// ---------------------------------------------------------------------------

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: i32) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: wayland_client::protocol::wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &WlSurface, _: u32) {
        let Some(idx) = self.outputs.iter().position(|s| &s.surface == surface) else {
            return;
        };
        self.outputs[idx].frame_pending = false;
        self.render_video_frame(idx);
        self.schedule_frame(idx);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: &WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: &WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: WlOutput) {
        // Adopt a hotplugged output only if the ledger already wants it painted.
        let name = self
            .output_state
            .info(&output)
            .and_then(|i| i.name)
            .unwrap_or_default();
        if self.ledger.desired_for(&name).is_some()
            && let Some(idx) = self.ensure_surface(&output)
        {
            self.apply_desired_to(idx);
        }
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: WlOutput) {
        if let Some(idx) = self.outputs.iter().position(|s| s.output == output) {
            self.destroy_surface(idx);
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_registry!(App);

impl wayland_client::Dispatch<ZwlrLayerSurfaceV1, ()> for App {
    fn event(
        state: &mut Self,
        layer: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Event;
        match event {
            Event::Configure {
                serial,
                width,
                height,
            } => {
                layer.ack_configure(serial);
                let Some(idx) = state.outputs.iter().position(|s| &s.layer == layer) else {
                    return;
                };
                if width > 0 && height > 0 {
                    state.outputs[idx].width = width;
                    state.outputs[idx].height = height;
                }
                state.outputs[idx].configured = true;
                if state.outputs[idx].width == 0 || state.outputs[idx].height == 0 {
                    return;
                }
                state.apply_desired_to(idx);
            }
            Event::Closed => {
                if let Some(idx) = state.outputs.iter().position(|s| &s.layer == layer) {
                    state.destroy_surface(idx);
                }
            }
            _ => {}
        }
    }
}

impl wayland_client::Dispatch<ZwlrLayerShellV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwlrLayerShellV1,
        _: <ZwlrLayerShellV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl wayland_client::Dispatch<WpViewport, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpViewport,
        _: <WpViewport as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl wayland_client::Dispatch<WpViewporter, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: <WpViewporter as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_parse_roundtrips() {
        assert_eq!(ResourceTier::parse("low"), Some(ResourceTier::Low));
        assert_eq!(ResourceTier::parse("medium"), Some(ResourceTier::Medium));
        assert_eq!(ResourceTier::parse("high"), Some(ResourceTier::High));
        assert_eq!(ResourceTier::parse("bogus"), None);
        assert_eq!(ResourceTier::default(), ResourceTier::Medium);
    }

    #[test]
    fn tier_render_size_caps_longest_edge_preserving_aspect() {
        // High never scales.
        assert_eq!(ResourceTier::High.render_size(3840, 2160), (3840, 2160));
        assert!(!ResourceTier::High.upscales());

        // Medium caps the longest edge at 1920.
        assert_eq!(ResourceTier::Medium.render_size(3840, 2160), (1920, 1080));
        assert!(ResourceTier::Medium.upscales());

        // Low caps the longest edge at 1280.
        assert_eq!(ResourceTier::Low.render_size(3840, 2160), (1280, 720));
        // Portrait: the longest edge is the height.
        assert_eq!(ResourceTier::Low.render_size(1440, 2560), (720, 1280));

        // Already within the cap: unchanged.
        assert_eq!(ResourceTier::Low.render_size(800, 600), (800, 600));
        assert_eq!(ResourceTier::Medium.render_size(1600, 900), (1600, 900));
    }

    #[test]
    fn source_classify_by_extension() {
        for p in ["a.jpg", "B.PNG", "c.webp", "/x/y.AVIF", "d.gif", "e.tiff"] {
            assert!(
                matches!(Source::classify(p), Source::Static(_)),
                "{p} should be static"
            );
        }
        for p in ["a.mp4", "b.webm", "c.mkv", "noext", "d.txt"] {
            assert!(
                matches!(Source::classify(p), Source::Video(_)),
                "{p} should be video"
            );
        }
    }

    fn static_desired() -> Desired {
        Desired {
            source: Source::Static(PathBuf::from("/w.png")),
            tier: ResourceTier::Medium,
            fill: FillMode::Fill,
            mute: true,
            volume: 80,
        }
    }

    fn video_desired() -> Desired {
        Desired {
            source: Source::Video(PathBuf::from("/v.mp4")),
            tier: ResourceTier::Medium,
            fill: FillMode::Fill,
            mute: true,
            volume: 80,
        }
    }

    #[test]
    fn ledger_broadcast_and_override() {
        let mut l = SurfaceLedger::new();
        l.show("*", static_desired());
        assert!(matches!(
            l.desired_for("DP-1").map(|d| &d.source),
            Some(Source::Static(_))
        ));
        assert!(!l.any_video());

        // A per-output override wins for that connector; others keep the default.
        l.show("DP-2", video_desired());
        assert!(l.desired_for("DP-2").unwrap().source.is_video());
        assert!(!l.desired_for("DP-1").unwrap().source.is_video());
        assert!(l.any_video());
    }

    #[test]
    fn ledger_broadcast_clears_overrides() {
        let mut l = SurfaceLedger::new();
        l.show("DP-2", video_desired());
        assert!(l.any_video());
        // A fresh broadcast replaces the whole scene.
        l.show("*", static_desired());
        assert!(!l.any_video());
        assert!(l.desired_for("DP-2").is_some());
    }

    #[test]
    fn ledger_stop_and_idle_release() {
        let mut l = SurfaceLedger::new();
        l.show("*", static_desired());
        l.show("DP-2", video_desired());
        assert!(l.any_video());
        // Idle-release must not drop GL while a livewall is desired.
        assert!(!l.idle_release());

        l.stop("DP-2");
        assert!(!l.any_video());
        // DP-2 falls back to the broadcast default.
        assert!(matches!(
            l.desired_for("DP-2").map(|d| &d.source),
            Some(Source::Static(_))
        ));
        // Now a settled static desktop: GL may be released.
        assert!(l.idle_release());

        l.stop("*");
        assert!(l.desired_for("DP-1").is_none());
    }
}
