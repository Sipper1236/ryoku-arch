//! Ryoku's reveal transition ported onto the paper GL engine.
//!
//! `reveal.frag` (the in-shell backdrop's shader) is one fragment stage keyed by
//! `u_kind`: kinds 0..6 sweep a scalar reveal mask (fade / wipe / wave / center /
//! grow / any / outer), 7..15 warp the sample coordinates (pixelate / dissolve /
//! ripple / shatter / glitch / crt / stripes / melt / peel). The uniforms and
//! textures mirror the QML `ShaderEffect`; the daemon fills them from a picked
//! transition (see the daemon's `wall::transitions`).
//!
//! Orientation: the vertex stage flips `v_uv` to top-left origin (Qt's
//! convention, which `reveal.frag` was authored for), so image textures are
//! uploaded native (row 0 = top) and sampled with `v_uv` directly. skwd's
//! catalog shaders share the same flipped vertex stage.

use anyhow::{Result, anyhow};
use std::ffi::CString;

/// The 16 reveal geometries in code order — matches `reveal.frag`'s `kind` and
/// `Backdrop.qml`'s `kindCode`.
pub const REVEAL_KINDS: [&str; 16] = [
    "fade", "wipe", "wave", "center", "grow", "any", "outer", "pixelate", "dissolve", "ripple",
    "shatter", "glitch", "crt", "stripes", "melt", "peel",
];

/// Map a reveal kind name to its shader code; an unknown name is `fade` (0),
/// mirroring `Backdrop.qml`'s `kindCode`.
#[must_use]
pub fn kind_code(name: &str) -> i32 {
    REVEAL_KINDS
        .iter()
        .position(|&k| k == name)
        .map_or(0, |i| i as i32)
}

/// A resolved reveal, everything the shader needs for one switch. `shader` names
/// a skwd catalog transition instead of the reveal stage when set; the reveal
/// `kind`/params are then unused.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transition {
    pub kind: i32,
    pub angle: f32,
    pub wave_amp: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub edge_softness: f32,
    pub seed: f32,
    pub duration_ms: u64,
    pub bezier: [f32; 4],
    pub shader: Option<String>,
}

/// Cubic-bezier easing: the value (eased progress) at wall-clock fraction `x`.
/// Control points are P0=(0,0), P1=(b0,b1), P2=(b2,b3), P3=(1,1); solve for the
/// parameter `s` with X(s)=x (Newton, bisection fallback), return Y(s). This is
/// the timing QML's `Easing.Bezier` applies to the reveal's `progress`.
#[must_use]
pub fn cubic_bezier_ease(b: [f32; 4], x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let (x1, y1, x2, y2) = (b[0], b[1], b[2], b[3]);
    let bez = |a: f32, c: f32, t: f32| {
        let mt = 1.0 - t;
        3.0 * mt * mt * t * a + 3.0 * mt * t * t * c + t * t * t
    };
    let dbez = |a: f32, c: f32, t: f32| {
        let mt = 1.0 - t;
        3.0 * mt * mt * a + 6.0 * mt * t * (c - a) + 3.0 * t * t * (1.0 - c)
    };
    let mut t = x;
    for _ in 0..8 {
        let dx = bez(x1, x2, t) - x;
        if dx.abs() < 1e-5 {
            return bez(y1, y2, t);
        }
        let d = dbez(x1, x2, t);
        if d.abs() < 1e-6 {
            break;
        }
        t -= dx / d;
        t = t.clamp(0.0, 1.0);
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    let mut mid = x;
    for _ in 0..24 {
        mid = 0.5 * (lo + hi);
        let vx = bez(x1, x2, mid);
        if (vx - x).abs() < 1e-5 {
            break;
        }
        if vx < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(y1, y2, mid)
}

/// A compiled transition program plus the `u_progress` location driven each
/// frame. Built once per switch; [`Self::delete`] frees it after.
pub struct RevealProgram {
    pub program: u32,
    pub loc_progress: i32,
}

impl RevealProgram {
    /// Compile the reveal stage and set its static (per-switch) uniforms. A
    /// context must be current.
    pub fn reveal(t: &Transition, res: (f32, f32)) -> Result<Self> {
        let program = link_program(REVEAL_VERT, REVEAL_FRAG)?;
        let loc_progress = unsafe {
            gl::UseProgram(program);
            set_sampler(program, "u_tex_old\0", 0);
            set_sampler(program, "u_tex_new\0", 1);
            uniform_i(program, "u_kind\0", t.kind);
            uniform_f(program, "u_angle\0", t.angle);
            uniform_f(program, "u_wave_amp\0", t.wave_amp);
            uniform_2f(program, "u_origin\0", t.origin_x, t.origin_y);
            uniform_f(program, "u_edge_softness\0", t.edge_softness);
            uniform_f(program, "u_seed\0", t.seed);
            uniform_2f(program, "u_res\0", res.0, res.1);
            gl::GetUniformLocation(program, b"u_progress\0".as_ptr().cast())
        };
        Ok(Self {
            program,
            loc_progress,
        })
    }

    /// Compile a skwd catalog shader (single `u_progress`, two textures). Used
    /// when a transition names a catalog entry rather than a reveal kind.
    pub fn catalog(frag_src: &str) -> Result<Self> {
        let program = link_program(REVEAL_VERT, frag_src)?;
        let loc_progress = unsafe {
            gl::UseProgram(program);
            set_sampler(program, "u_tex_old\0", 0);
            set_sampler(program, "u_tex_new\0", 1);
            // The one thumbnail-driven catalog shader (mosaic-tumble) degrades to
            // a plain rotate/scatter when no thumbs are bound.
            uniform_i(program, "u_thumb_count\0", 0);
            gl::GetUniformLocation(program, b"u_progress\0".as_ptr().cast())
        };
        Ok(Self {
            program,
            loc_progress,
        })
    }

    pub fn set_progress(&self, p: f32) {
        unsafe {
            gl::UseProgram(self.program);
            if self.loc_progress >= 0 {
                gl::Uniform1f(self.loc_progress, p);
            }
        }
    }

    pub fn delete(&self) {
        unsafe {
            if self.program != 0 {
                gl::DeleteProgram(self.program);
            }
        }
    }
}

unsafe fn set_sampler(program: u32, name: &str, unit: i32) {
    unsafe {
        let loc = gl::GetUniformLocation(program, name.as_ptr().cast());
        if loc >= 0 {
            gl::Uniform1i(loc, unit);
        }
    }
}

unsafe fn uniform_i(program: u32, name: &str, v: i32) {
    unsafe {
        let loc = gl::GetUniformLocation(program, name.as_ptr().cast());
        if loc >= 0 {
            gl::Uniform1i(loc, v);
        }
    }
}

unsafe fn uniform_f(program: u32, name: &str, v: f32) {
    unsafe {
        let loc = gl::GetUniformLocation(program, name.as_ptr().cast());
        if loc >= 0 {
            gl::Uniform1f(loc, v);
        }
    }
}

unsafe fn uniform_2f(program: u32, name: &str, a: f32, b: f32) {
    unsafe {
        let loc = gl::GetUniformLocation(program, name.as_ptr().cast());
        if loc >= 0 {
            gl::Uniform2f(loc, a, b);
        }
    }
}

fn link_program(vert_src: &str, frag_src: &str) -> Result<u32> {
    unsafe {
        let v = compile_shader(gl::VERTEX_SHADER, vert_src)?;
        let f = compile_shader(gl::FRAGMENT_SHADER, frag_src)?;
        let p = gl::CreateProgram();
        gl::AttachShader(p, v);
        gl::AttachShader(p, f);
        gl::LinkProgram(p);
        let mut ok: i32 = 0;
        gl::GetProgramiv(p, gl::LINK_STATUS, &mut ok);
        gl::DeleteShader(v);
        gl::DeleteShader(f);
        if ok == 0 {
            return Err(anyhow!("reveal program link failed"));
        }
        Ok(p)
    }
}

unsafe fn compile_shader(kind: u32, src: &str) -> Result<u32> {
    unsafe {
        let s = gl::CreateShader(kind);
        let c = CString::new(src).map_err(|_| anyhow!("shader src NUL"))?;
        let ptr = c.as_ptr();
        gl::ShaderSource(s, 1, &ptr, std::ptr::null());
        gl::CompileShader(s);
        let mut ok: i32 = 0;
        gl::GetShaderiv(s, gl::COMPILE_STATUS, &mut ok);
        if ok == 0 {
            gl::DeleteShader(s);
            return Err(anyhow!("reveal shader compile failed (kind={kind})"));
        }
        Ok(s)
    }
}

/// Flips `v_uv` to top-left origin so the ported `reveal.frag` (Qt convention)
/// samples native-orientation textures upright.
const REVEAL_VERT: &str = "#version 330 core
layout(location=0) in vec2 a_pos;
layout(location=1) in vec2 a_tex;
out vec2 v_uv;
void main() {
    gl_Position = vec4(a_pos, 0.0, 1.0);
    v_uv = vec2(a_tex.x, 1.0 - a_tex.y);
}
";

/// The 16-kind reveal, ported from `reveal.frag` (`#version 440` Qt) to
/// `#version 330 core` with individual uniforms; kind branches are verbatim.
pub const REVEAL_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;

uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
uniform float u_angle;
uniform float u_wave_amp;
uniform vec2 u_origin;
uniform float u_edge_softness;
uniform float u_seed;
uniform int u_kind;
uniform vec2 u_res;

const float TAU = 6.28318530718;
const float PI = 3.14159265359;

float rand(vec2 c) {
    return fract(sin(dot(c, vec2(12.9898, 78.233))) * 43758.5453);
}

float vnoise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    float a = rand(i), b = rand(i + vec2(1.0, 0.0));
    float c = rand(i + vec2(0.0, 1.0)), d = rand(i + vec2(1.0, 1.0));
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(a, b, u.x) + (c - a) * u.y * (1.0 - u.x) + (d - b) * u.x * u.y;
}

vec2 hash2(vec2 q) {
    q = vec2(dot(q, vec2(127.1, 311.7)), dot(q, vec2(269.5, 183.3)));
    return fract(sin(q) * 43758.5453);
}

vec4 revealExpressive(vec2 uv) {
    float p = u_progress;

    if (u_kind == 7) {
        float amp = 1.0 - abs(2.0 * p - 1.0);
        vec2 suv = uv;
        if (amp > 0.001) {
            float block = 0.05 * amp;
            suv = floor(uv / block) * block + block * 0.5;
        }
        float a = smoothstep(0.4, 0.6, p);
        return mix(texture(u_tex_old, suv), texture(u_tex_new, suv), a);
    }

    if (u_kind == 8) {
        vec2 nuv = uv + u_seed;
        float n = vnoise(nuv * 6.0) * 0.7 + vnoise(nuv * 15.0) * 0.3;
        float reveal = step(n, p);
        float glow = 1.0 - smoothstep(0.0, 0.05, abs(n - p));
        vec4 colour = mix(texture(u_tex_old, uv), texture(u_tex_new, uv), reveal);
        colour.rgb += vec3(1.0, 0.82, 0.45) * glow * 1.3;
        return colour;
    }

    if (u_kind == 9) {
        vec2 origin = u_origin;
        float d = distance(uv, origin);
        float front = p * 0.9;
        float wave = sin((d - front) * 35.0) * exp(-abs(d - front) * 10.0);
        vec2 dir = normalize(uv - origin + 1e-4);
        vec2 suv = uv + dir * wave * 0.04;
        float reveal = step(d, front);
        vec4 colour = mix(texture(u_tex_old, suv), texture(u_tex_new, suv), reveal);
        colour.rgb += max(wave, 0.0) * 0.25;
        return colour;
    }

    if (u_kind == 10) {
        const float cells = 10.0;
        vec2 g = uv * cells;
        vec2 id = floor(g), cuv = fract(g);
        vec2 r = hash2(id + u_seed);
        float delay = r.x * 0.6;
        float lp = clamp((p - delay) / (1.0 - delay), 0.0, 1.0);
        float rot = lp * (r.y - 0.5) * 3.0;
        vec2 c = cuv - 0.5;
        c = mat2(cos(rot), -sin(rot), sin(rot), cos(rot)) * c * (1.0 - lp * 0.35);
        c += normalize(r - 0.5) * lp * 0.6;
        vec2 suv = (id + c + 0.5) / cells;
        float oldA = 1.0 - smoothstep(0.0, 1.0, lp);
        return mix(texture(u_tex_new, uv), texture(u_tex_old, suv), oldA);
    }

    if (u_kind == 11) {
        float amp = sin(p * PI) * 0.08;
        float band = floor(uv.y * 24.0) / 24.0;
        float dx = (rand(vec2(band, u_seed)) - 0.5) * amp;
        vec2 g = vec2(uv.x + dx, uv.y);
        float shift = amp * 0.5;
        float r = mix(texture(u_tex_old, g + vec2(shift, 0.0)).r, texture(u_tex_new, g + vec2(shift, 0.0)).r, p);
        float gc = mix(texture(u_tex_old, g).g, texture(u_tex_new, g).g, p);
        float b = mix(texture(u_tex_old, g - vec2(shift, 0.0)).b, texture(u_tex_new, g - vec2(shift, 0.0)).b, p);
        float n = rand(uv * (u_seed + 1.0)) * amp * 2.0;
        return vec4(r + n, gc + n, b + n, 1.0);
    }

    if (u_kind == 12) {
        float dy = abs(uv.y - 0.5);
        vec3 colour;
        if (p < 0.5) {
            float h = clamp(p * 2.0, 0.0, 1.0);
            float band = mix(0.5, 0.004, h);
            if (dy < band) {
                vec2 s = vec2(uv.x, 0.5 + (uv.y - 0.5) / max(band / 0.5, 0.001));
                colour = texture(u_tex_old, s).rgb + vec3(0.8, 0.9, 1.0) * smoothstep(band, band * 0.7, dy) * h;
            } else {
                colour = vec3(0.0);
            }
        } else {
            float h = clamp(p * 2.0 - 1.0, 0.0, 1.0);
            float band = mix(0.004, 0.5, h);
            if (dy < band) {
                vec2 s = vec2(uv.x, 0.5 + (uv.y - 0.5) / max(band / 0.5, 0.001));
                colour = texture(u_tex_new, s).rgb + vec3(0.8, 0.9, 1.0) * smoothstep(band, band * 0.7, dy) * (1.0 - h);
            } else {
                colour = vec3(0.0);
            }
        }
        return vec4(colour, 1.0);
    }

    if (u_kind == 13) {
        const float stripes = 12.0;
        float a = radians(u_angle);
        float along = uv.x * cos(a) + uv.y * sin(a);
        float perp = -uv.x * sin(a) + uv.y * cos(a);
        int idx = int(floor(along * stripes));
        bool odd = mod(float(idx), 2.0) != 0.0;
        float delay = clamp(along, 0.0, 1.0) * 0.1;
        float sp = clamp((p - delay) / (1.0 - 0.1), 0.0, 1.0);
        float lo = min(min(0.0, -sin(a)), min(cos(a), cos(a) - sin(a)));
        float hi = max(max(0.0, -sin(a)), max(cos(a), cos(a) - sin(a)));
        float soft = 0.02;
        float edge = odd ? (hi + soft) - sp * (hi - lo + soft * 2.0)
                         : (lo - soft) + sp * (hi - lo + soft * 2.0);
        float mask = odd ? smoothstep(edge - soft, edge + soft, perp)
                         : 1.0 - smoothstep(edge - soft, edge + soft, perp);
        vec4 colour = mix(texture(u_tex_old, uv), texture(u_tex_new, uv), mask);
        colour.rgb *= 1.0 - 0.2 * (1.0 - abs(sp - 0.5) * 2.0) * (1.0 - smoothstep(0.0, soft * 2.5, abs(perp - edge)));
        return colour;
    }

    if (u_kind == 14) {
        float col = vnoise(vec2(uv.x * 24.0, u_seed)) * 0.6 + vnoise(vec2(uv.x * 96.0, u_seed)) * 0.15;
        float accel = 1.0 + smoothstep(0.5, 1.0, p) * 0.5;
        float push = clamp(col * 0.5 + (p * 2.0 - 1.0) * accel * 2.0, 0.0, 2.0);
        vec2 s = vec2(uv.x, uv.y - push);
        return s.y >= 0.0 ? texture(u_tex_old, s) : texture(u_tex_new, uv);
    }

    if (u_kind == 15) {
        float front = (uv.x + uv.y) * 0.5;
        return front < p ? texture(u_tex_new, uv)
                         : texture(u_tex_old, uv - vec2(p * 0.2));
    }

    return texture(u_tex_new, uv);
}

void main() {
    vec2 uv = v_uv;

    if (u_kind >= 7) {
        frag = revealExpressive(uv);
        return;
    }

    vec4 oldC = texture(u_tex_old, uv);
    vec4 newC = texture(u_tex_new, uv);

    float a;
    if (u_kind == 0) {
        a = u_progress;
    } else {
        float pos;
        if (u_kind == 1 || u_kind == 2) {
            float rad = radians(u_angle);
            vec2 dir = vec2(cos(rad), sin(rad));
            float lo = min(0.0, dir.x) + min(0.0, dir.y);
            float hi = max(0.0, dir.x) + max(0.0, dir.y);
            pos = (dot(uv, dir) - lo) / max(hi - lo, 1e-4);
            if (u_kind == 2) {
                vec2 perp = vec2(-dir.y, dir.x);
                pos += u_wave_amp * sin(dot(uv, perp) * TAU * 2.5);
            }
        } else {
            float asp = u_res.x / max(u_res.y, 1.0);
            vec2 o = vec2(u_origin.x * asp, u_origin.y);
            vec2 pp = vec2(uv.x * asp, uv.y);
            float d = distance(pp, o);
            float m = max(max(distance(o, vec2(0.0, 0.0)), distance(o, vec2(asp, 0.0))),
                          max(distance(o, vec2(0.0, 1.0)), distance(o, vec2(asp, 1.0))));
            float radial = d / max(m, 1e-4);
            pos = (u_kind == 6) ? (1.0 - radial) : radial;
        }
        float soft = max(u_edge_softness, 0.002);
        float margin = soft + u_wave_amp;
        float f = u_progress * (1.0 + 2.0 * margin) - margin;
        a = smoothstep(pos - soft, pos + soft, f);
    }

    frag = mix(oldC, newC, clamp(a, 0.0, 1.0));
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_code_matches_backdrop_mapping() {
        assert_eq!(kind_code("fade"), 0);
        assert_eq!(kind_code("outer"), 6);
        assert_eq!(kind_code("pixelate"), 7);
        assert_eq!(kind_code("peel"), 15);
        // Unknown falls back to fade, like Backdrop.qml's kindCode default.
        assert_eq!(kind_code("nope"), 0);
    }

    #[test]
    fn reveal_frag_declares_every_kind_uniform() {
        for u in [
            "u_tex_old",
            "u_tex_new",
            "u_progress",
            "u_angle",
            "u_wave_amp",
            "u_origin",
            "u_edge_softness",
            "u_seed",
            "u_kind",
            "u_res",
        ] {
            assert!(REVEAL_FRAG.contains(u), "reveal.frag missing uniform {u}");
        }
        // Kinds routed by an explicit branch: 0/1/2 (scalar mask) and 7..15
        // (expressive). Kinds 3..6 (center / grow / any / outer) share the radial
        // else-branch, so they carry no per-kind `u_kind == N`; `outer` (6)
        // inverts the radial mask there.
        for k in [0, 1, 2, 7, 8, 9, 10, 11, 12, 13, 14, 15] {
            assert!(
                REVEAL_FRAG.contains(&format!("u_kind == {k}")),
                "reveal.frag missing kind {k}"
            );
        }
        assert!(REVEAL_FRAG.contains("float radial"), "reveal.frag missing radial path");
        assert!(REVEAL_FRAG.contains("u_kind == 6"), "reveal.frag missing outer (6)");
    }

    #[test]
    fn cubic_bezier_endpoints_and_monotonicity() {
        let b = [0.65, 0.0, 0.35, 1.0]; // easeInOutCubic
        assert!((cubic_bezier_ease(b, 0.0) - 0.0).abs() < 1e-4);
        assert!((cubic_bezier_ease(b, 1.0) - 1.0).abs() < 1e-4);
        // Symmetric curve: midpoint eases to ~0.5.
        assert!((cubic_bezier_ease(b, 0.5) - 0.5).abs() < 1e-3);
        // Monotonic non-decreasing across the sweep.
        let mut prev = -1.0;
        for i in 0..=20 {
            let v = cubic_bezier_ease(b, i as f32 / 20.0);
            assert!(v + 1e-4 >= prev, "bezier not monotonic at {i}");
            prev = v;
        }
    }
}
