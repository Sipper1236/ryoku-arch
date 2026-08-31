//! The wallpaper reveal picker, ported from ryoku's `ipc/transitions.go`.
//!
//! The 22-preset table, the no-repeat picker, and the compass origin map are the
//! daemon-side selection: a user switch resolves the configured
//! `wallpaper.transition_preset` (or a fresh no-repeat pick for the "random"
//! sentinel / an unknown value) into a [`ryogami_paper::Transition`] the renderer
//! runs on the shared GL context; init / restore / live-reload reveal with none.
//! The reveal geometry and shader live in the paper crate (`transitions::reveal`);
//! this module only chooses which preset a switch reveals with.

// Casts here are index/seed arithmetic on a small closed table; truncation and
// precision loss are intended (a 0..1 seed, a modulo index).
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_lossless
)]

use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use ryogami_paper::Transition;
use ryogami_paper::transitions::kind_code;

/// The shared wall-clock length of every reveal (the recovered 2.2s): only the
/// shape varies per preset, so the desktop feels consistent across switches.
const TRANSITION_DURATION_MS: u64 = 2200;

/// The sentinel `wallpaper.transition_preset` value: a fresh no-repeat pick per
/// switch, the shipped default.
const TRANSITION_RANDOM: &str = "random";

/// One named reveal: the reveal geometry (`kind`, mapped to a shader code), the
/// cubic-bezier timing, the sweep `angle`/`wave_amp`, the grow-origin compass
/// `pos`, and the boundary `edge_softness`. Mirrors Go's `transitionPreset`.
struct Preset {
    name: &'static str,
    kind: &'static str,
    angle: f32,
    wave_amp: f32,
    /// Origin anchor for the `grow` kind (a compass point); "" is the centre.
    pos: &'static str,
    bezier: [f32; 4],
    edge_softness: f32,
}

/// The 22-preset table in Go's order: 13 recovered from the wallpaper daemon
/// (one crossfade, three sweeps, five circle reveals, four M3 expressive ports)
/// plus nine coordinate-warping ports of the upstream ii set. `edge_softness`
/// keeps Go's `(120 - step) / 300` mapping (step = crispness).
static PRESETS: [Preset; 22] = [
    // crossfade
    Preset { name: "silk_fade", kind: "fade", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.65, 0.0, 0.35, 1.0], edge_softness: 0.0 },
    // directional sweeps (wipe / wave)
    Preset { name: "diagonal_silk", kind: "wipe", angle: 30.0, wave_amp: 0.0, pos: "",
        bezier: [0.16, 1.0, 0.3, 1.0], edge_softness: (120.0 - 110.0) / 300.0 },
    Preset { name: "dream_curtain", kind: "wipe", angle: 90.0, wave_amp: 0.0, pos: "",
        bezier: [0.83, 0.0, 0.17, 1.0], edge_softness: (120.0 - 35.0) / 300.0 },
    Preset { name: "liquid_ribbon", kind: "wave", angle: 45.0, wave_amp: 35.0 / 500.0, pos: "",
        bezier: [0.76, 0.0, 0.24, 1.0], edge_softness: (120.0 - 90.0) / 300.0 },
    // circle reveals (center / grow / outer / any)
    Preset { name: "iris_open", kind: "center", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.22, 1.0, 0.36, 1.0], edge_softness: (120.0 - 100.0) / 300.0 },
    Preset { name: "corner_bloom", kind: "grow", angle: 0.0, wave_amp: 0.0, pos: "bottom-left",
        bezier: [0.16, 1.0, 0.3, 1.0], edge_softness: (120.0 - 90.0) / 300.0 },
    Preset { name: "spotlight_rise", kind: "grow", angle: 0.0, wave_amp: 0.0, pos: "bottom",
        bezier: [0.0, 0.55, 0.45, 1.0], edge_softness: (120.0 - 90.0) / 300.0 },
    Preset { name: "wander_iris", kind: "any", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.25, 1.0, 0.5, 1.0], edge_softness: (120.0 - 100.0) / 300.0 },
    Preset { name: "vignette_close", kind: "outer", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.65, 0.0, 0.35, 1.0], edge_softness: (120.0 - 90.0) / 300.0 },
    // Material 3 expressive-motion ports
    Preset { name: "celeste_veil", kind: "fade", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.34, 0.88, 0.34, 1.0], edge_softness: 0.0 },
    Preset { name: "comet_streak", kind: "wipe", angle: 135.0, wave_amp: 0.0, pos: "",
        bezier: [0.05, 0.7, 0.1, 1.0], edge_softness: (120.0 - 100.0) / 300.0 },
    Preset { name: "aurora_ripple", kind: "wave", angle: 120.0, wave_amp: 30.0 / 500.0, pos: "",
        bezier: [0.31, 0.94, 0.34, 1.0], edge_softness: (120.0 - 80.0) / 300.0 },
    Preset { name: "starfall_bloom", kind: "grow", angle: 0.0, wave_amp: 0.0, pos: "top",
        bezier: [0.2, 0.0, 0.0, 1.0], edge_softness: (120.0 - 100.0) / 300.0 },
    // coordinate-warping ports of the upstream ii set
    Preset { name: "mosaic_swell", kind: "pixelate", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.65, 0.0, 0.35, 1.0], edge_softness: 0.0 },
    Preset { name: "ember_burn", kind: "dissolve", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.76, 0.0, 0.24, 1.0], edge_softness: (120.0 - 60.0) / 300.0 },
    Preset { name: "pond_wake", kind: "ripple", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.22, 1.0, 0.36, 1.0], edge_softness: (120.0 - 85.0) / 300.0 },
    Preset { name: "glass_scatter", kind: "shatter", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.05, 0.7, 0.1, 1.0], edge_softness: 0.0 },
    Preset { name: "signal_tear", kind: "glitch", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.65, 0.0, 0.35, 1.0], edge_softness: 0.0 },
    Preset { name: "cathode_wink", kind: "crt", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.83, 0.0, 0.17, 1.0], edge_softness: 0.0 },
    Preset { name: "shutter_sweep", kind: "stripes", angle: 24.0, wave_amp: 0.0, pos: "",
        bezier: [0.25, 1.0, 0.5, 1.0], edge_softness: (120.0 - 90.0) / 300.0 },
    Preset { name: "wax_descent", kind: "melt", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.83, 0.0, 0.17, 1.0], edge_softness: 0.0 },
    Preset { name: "page_turn", kind: "peel", angle: 0.0, wave_amp: 0.0, pos: "",
        bezier: [0.16, 1.0, 0.3, 1.0], edge_softness: (120.0 - 100.0) / 300.0 },
];

/// A self-contained splitmix64 stream, so this crate needs no `rand` for the
/// per-switch index / origin / seed draws.
struct Rng {
    state: u64,
}

impl Rng {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn seeded() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64);
        Self::new(nanos ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform index in `0..n` (n must be non-zero).
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// A float in `[0, 1)` (24-bit mantissa), matching Go's `rand.Float64` use.
    fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The no-repeat picker state: the last index and the shared RNG. Mirrors the
/// daemon's `lastTransition` (Go guarded it under the wallpaper lock; here a
/// `Mutex` guards this struct).
struct Picker {
    last: Option<usize>,
    rng: Rng,
}

impl Picker {
    fn new() -> Self {
        Self { last: None, rng: Rng::seeded() }
    }

    /// A random index in `0..n`, never the immediately previous one. Mirrors Go's
    /// `pickTransition`: re-roll into the remaining `n-1` when the first draw
    /// repeats.
    fn next_index(&mut self, n: usize) -> usize {
        let mut i = self.rng.below(n);
        if n > 1 && Some(i) == self.last {
            i = (i + 1 + self.rng.below(n - 1)) % n;
        }
        self.last = Some(i);
        i
    }
}

static PICKER: LazyLock<Mutex<Picker>> = LazyLock::new(|| Mutex::new(Picker::new()));

/// The reveal a wallpaper op animates with. Restore / init / live-reload (the
/// carried or relaunched frame) reveal with none — they must not animate over a
/// fresh surface. A user switch resolves the configured preset, or picks a fresh
/// no-repeat one for the "random" sentinel / an unknown value. Mirrors Go's
/// `transitionFor`.
#[must_use]
pub fn transition_for(restoring: bool) -> Option<Transition> {
    if restoring {
        return None;
    }
    let mut picker = PICKER.lock().expect("transition picker mutex");
    if let Some(p) = lookup_preset(&configured_preset()) {
        return Some(resolve_transition(p, &mut picker.rng));
    }
    let i = picker.next_index(PRESETS.len());
    Some(resolve_transition(&PRESETS[i], &mut picker.rng))
}

/// Bind a preset to a concrete origin, a fresh per-switch seed, and the shared
/// duration; `kind` becomes the shader code the reveal stage keys on.
fn resolve_transition(p: &Preset, rng: &mut Rng) -> Transition {
    let (origin_x, origin_y) = origin_for_preset(p, rng);
    Transition {
        kind: kind_code(p.kind),
        angle: p.angle,
        wave_amp: p.wave_amp,
        origin_x,
        origin_y,
        edge_softness: p.edge_softness,
        seed: rng.unit(),
        duration_ms: TRANSITION_DURATION_MS,
        bezier: p.bezier,
        shader: None,
    }
}

/// Resolve the reveal origin in surface coordinates (0,0 top-left .. 1,1
/// bottom-right). `any` blooms from a fresh random point; `grow` reads its
/// compass `pos`; everything else blooms from (or, for `outer`, seals toward)
/// the centre. Mirrors Go's `originForPreset`.
fn origin_for_preset(p: &Preset, rng: &mut Rng) -> (f32, f32) {
    if p.kind == "any" {
        return (rng.unit(), rng.unit());
    }
    match p.pos {
        "top" => (0.5, 0.0),
        "bottom" => (0.5, 1.0),
        "left" => (0.0, 0.5),
        "right" => (1.0, 0.5),
        "top-left" => (0.0, 0.0),
        "top-right" => (1.0, 0.0),
        "bottom-left" => (0.0, 1.0),
        "bottom-right" => (1.0, 1.0),
        _ => (0.5, 0.5),
    }
}

/// Resolve a stored preference to a preset. The random sentinel, the empty
/// string (no key), and any absent name return `None`, so `transition_for` falls
/// back to the no-repeat picker.
fn lookup_preset(name: &str) -> Option<&'static Preset> {
    if name.is_empty() || name == TRANSITION_RANDOM {
        return None;
    }
    PRESETS.iter().find(|p| p.name == name)
}

/// Read `wallpaper.transition_preset` from ryoku's `shell.json` per apply (so a
/// changed preference takes effect on the next switch with no live plumbing). A
/// missing dir/file/key or unreadable value is the random sentinel.
fn configured_preset() -> String {
    let Some(dir) = ryoku_config_dir() else {
        return TRANSITION_RANDOM.to_string();
    };
    let Ok(bytes) = std::fs::read(dir.join("shell.json")) else {
        return TRANSITION_RANDOM.to_string();
    };
    let parsed: ShellJson = serde_json::from_slice(&bytes).unwrap_or_default();
    if parsed.wallpaper.transition_preset.is_empty() {
        TRANSITION_RANDOM.to_string()
    } else {
        parsed.wallpaper.transition_preset
    }
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

#[derive(Deserialize, Default)]
struct ShellJson {
    #[serde(default)]
    wallpaper: ShellWallpaper,
}

#[derive(Deserialize, Default)]
struct ShellWallpaper {
    #[serde(default)]
    transition_preset: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 22-preset table matches `ipc/transitions.go`'s names + kinds in order.
    #[test]
    fn preset_table_matches_go() {
        const EXPECTED: [(&str, &str); 22] = [
            ("silk_fade", "fade"),
            ("diagonal_silk", "wipe"),
            ("dream_curtain", "wipe"),
            ("liquid_ribbon", "wave"),
            ("iris_open", "center"),
            ("corner_bloom", "grow"),
            ("spotlight_rise", "grow"),
            ("wander_iris", "any"),
            ("vignette_close", "outer"),
            ("celeste_veil", "fade"),
            ("comet_streak", "wipe"),
            ("aurora_ripple", "wave"),
            ("starfall_bloom", "grow"),
            ("mosaic_swell", "pixelate"),
            ("ember_burn", "dissolve"),
            ("pond_wake", "ripple"),
            ("glass_scatter", "shatter"),
            ("signal_tear", "glitch"),
            ("cathode_wink", "crt"),
            ("shutter_sweep", "stripes"),
            ("wax_descent", "melt"),
            ("page_turn", "peel"),
        ];
        assert_eq!(PRESETS.len(), EXPECTED.len());
        for (p, (name, kind)) in PRESETS.iter().zip(EXPECTED) {
            assert_eq!(p.name, name);
            assert_eq!(p.kind, kind);
            // Every kind must map into the shader's 16-kind set (0..16).
            let code = kind_code(p.kind);
            assert!((0..16).contains(&code), "{name}: kind {kind} out of range");
        }
    }

    /// The picker never returns the same index twice in a row, across many picks
    /// and independent of the seed.
    #[test]
    fn picker_never_repeats() {
        let n = PRESETS.len();
        for seed in [1u64, 42, 777, 0xDEAD_BEEF, u64::MAX] {
            let mut picker = Picker { last: None, rng: Rng::new(seed) };
            let mut prev = None;
            for _ in 0..100_000 {
                let i = picker.next_index(n);
                assert!(i < n);
                if let Some(p) = prev {
                    assert_ne!(i, p, "picker repeated index {i}");
                }
                prev = Some(i);
            }
        }
        // And the live time-seeded picker holds the same property.
        let mut picker = Picker::new();
        let mut prev = None;
        for _ in 0..100_000 {
            let i = picker.next_index(n);
            if let Some(p) = prev {
                assert_ne!(i, p);
            }
            prev = Some(i);
        }
    }

    /// The compass origin map resolves as in Go's `originForPreset`.
    #[test]
    fn origin_compass_mapping() {
        fn at(kind: &'static str, pos: &'static str) -> (f32, f32) {
            let p = Preset {
                name: "",
                kind,
                angle: 0.0,
                wave_amp: 0.0,
                pos,
                bezier: [0.0; 4],
                edge_softness: 0.0,
            };
            origin_for_preset(&p, &mut Rng::new(1))
        }
        assert_eq!(at("grow", "top"), (0.5, 0.0));
        assert_eq!(at("grow", "bottom"), (0.5, 1.0));
        assert_eq!(at("grow", "left"), (0.0, 0.5));
        assert_eq!(at("grow", "right"), (1.0, 0.5));
        assert_eq!(at("grow", "top-left"), (0.0, 0.0));
        assert_eq!(at("grow", "top-right"), (1.0, 0.0));
        assert_eq!(at("grow", "bottom-left"), (0.0, 1.0));
        assert_eq!(at("grow", "bottom-right"), (1.0, 1.0));
        // center / outer / fade (no pos) all seal from the centre.
        assert_eq!(at("center", ""), (0.5, 0.5));
        assert_eq!(at("outer", ""), (0.5, 0.5));
        assert_eq!(at("fade", ""), (0.5, 0.5));

        // The table's grow presets carry their compass anchor.
        let lookup = |name| PRESETS.iter().find(|p| p.name == name).unwrap();
        let mut rng = Rng::new(1);
        assert_eq!(origin_for_preset(lookup("corner_bloom"), &mut rng), (0.0, 1.0));
        assert_eq!(origin_for_preset(lookup("spotlight_rise"), &mut rng), (0.5, 1.0));
        assert_eq!(origin_for_preset(lookup("starfall_bloom"), &mut rng), (0.5, 0.0));

        // `any` blooms from a random on-screen point.
        let (x, y) = at("any", "");
        assert!((0.0..1.0).contains(&x) && (0.0..1.0).contains(&y));
    }

    /// A resolved reveal carries the shared duration and no skwd catalog shader.
    #[test]
    fn resolve_fills_transition() {
        let mut rng = Rng::new(7);
        let t = resolve_transition(&PRESETS[0], &mut rng);
        assert_eq!(t.duration_ms, TRANSITION_DURATION_MS);
        assert_eq!(t.kind, kind_code("fade"));
        assert!(t.shader.is_none());
        assert!((0.0..1.0).contains(&t.seed));
    }
}
