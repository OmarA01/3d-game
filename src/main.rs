// CRYSTAL RUSH — a neon first-person maze raid.
//
// Collect every crystal in the maze. Drones patrol the corridors and hunt
// you with real pathfinding, turrets snipe you with dodgeable plasma bolts.
// Outrun them, dash through gaps, or gun everything down. Each level the
// maze grows and the machines get faster.
//
// Rendering: custom GLSL material with 12 dynamic point lights, per-pixel
// fog, emissive wall trim and floor grid; additive billboard glows; all
// geometry built as meshes with real normals. Audio is synthesized at
// startup (no asset files).

use macroquad::audio::{load_sound_from_bytes, play_sound, PlaySoundParams, Sound};
use macroquad::miniquad::{
    BlendFactor, BlendState, Comparison, Equation, PipelineParams, UniformDesc, UniformType,
};
use macroquad::prelude::*;
use macroquad::rand::{gen_range, srand};
use std::collections::VecDeque;

// ---------------------------------------------------------------- constants

const CELL: f32 = 2.0;
const WALL_H: f32 = 2.4;
const EYE_H: f32 = 0.85;
const PLAYER_R: f32 = 0.32;
const DRONE_R: f32 = 0.45;

const WALK_SPEED: f32 = 4.5;
const SPRINT_SPEED: f32 = 6.6;
const DASH_SPEED: f32 = 15.0;
const DASH_TIME: f32 = 0.16;
const DASH_CD: f32 = 1.4;

const SHOT_CD: f32 = 0.22;
const SHOT_RANGE: f32 = 35.0;
const DRONE_HP: i32 = 3;
const TURRET_HP: i32 = 4;
const DRONE_RESPAWN: f32 = 7.0;

const FOG_MAX: f32 = 26.0;
const BASE_FOV: f32 = 62.0;

const COL_BG: Color = Color::new(0.020, 0.012, 0.060, 1.0);
const COL_FOG: Color = Color::new(0.040, 0.026, 0.090, 1.0);
const COL_FLOOR: Color = Color::new(0.30, 0.28, 0.42, 1.0);
const COL_WALL: Color = Color::new(0.36, 0.30, 0.58, 1.0);
const COL_WALL_TOP: Color = Color::new(0.46, 0.38, 0.70, 1.0);
const COL_CRYSTAL: Color = Color::new(0.10, 0.95, 1.00, 1.0);
const COL_UI: Color = Color::new(0.45, 0.95, 1.00, 1.0);
const COL_OVERDRIVE: Color = Color::new(1.00, 0.30, 0.90, 1.0);

// ------------------------------------------------------------------ helpers

fn clerp(a: Color, b: Color, t: f32) -> Color {
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, a)
}

fn cmul(c: Color, m: f32) -> Color {
    Color::new(c.r * m, c.g * m, c.b * m, c.a)
}

fn shuffle<T>(v: &mut [T]) {
    for i in (1..v.len()).rev() {
        let j = gen_range(0, i + 1);
        v.swap(i, j);
    }
}

fn hash01(i: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E3779B9) ^ 0x85EBCA6B;
    x ^= x >> 16;
    x = x.wrapping_mul(0x45D9F3B);
    x ^= x >> 16;
    (x & 0xFFFFFF) as f32 / 16777216.0
}

fn center_text(text: &str, y: f32, size: f32, color: Color) {
    let d = measure_text(text, None, size as u16, 1.0);
    let x = screen_width() / 2.0 - d.width / 2.0;
    draw_text(text, x + 2.0, y + 2.0, size, Color::new(0.0, 0.0, 0.0, color.a * 0.7));
    draw_text(text, x, y, size, color);
}

fn wrap_angle(a: f32) -> f32 {
    let mut a = a;
    while a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    while a < -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

// ---------------------------------------------------------------- sound gen

const SR: usize = 22050;

fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let data_len = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(SR as u32).to_le_bytes());
    out.extend_from_slice(&((SR * 2) as u32).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn render_samples(dur: f32, f: impl Fn(f32) -> f32) -> Vec<f32> {
    (0..(dur * SR as f32) as usize)
        .map(|i| f(i as f32 / SR as f32))
        .collect()
}

fn white(t: f32) -> f32 {
    hash01((t * SR as f32) as u32) * 2.0 - 1.0
}

fn sine(f: f32, t: f32) -> f32 {
    (std::f32::consts::TAU * f * t).sin()
}

fn sfx_shoot() -> Vec<u8> {
    wav_bytes(&render_samples(0.13, |t| {
        let k = t / 0.13;
        let freq = 950.0 - 4800.0 * t;
        let freq = freq.max(160.0);
        let ph = std::f32::consts::TAU * (950.0 * t - 2400.0 * t * t);
        let sq = if ph.sin() > 0.0 { 1.0 } else { -1.0 };
        let env = (1.0 - k).powi(2);
        (sq * 0.30 + sine(freq * 0.5, t) * 0.35 + white(t) * 0.06) * env
    }))
}

fn sfx_pickup() -> Vec<u8> {
    wav_bytes(&render_samples(0.22, |t| {
        let f = if t < 0.08 { 659.26 } else { 987.77 };
        let lt = if t < 0.08 { t } else { t - 0.08 };
        let env = (-lt * 14.0).exp() * (1.0 - (t / 0.22).powi(4));
        (sine(f, t) * 0.5 + sine(f * 2.0, t) * 0.15 + sine(f * 3.0, t) * 0.06) * env
    }))
}

fn sfx_health() -> Vec<u8> {
    wav_bytes(&render_samples(0.30, |t| {
        let f = if t < 0.12 { 440.0 } else { 554.37 };
        let lt = if t < 0.12 { t } else { t - 0.12 };
        let env = (-lt * 9.0).exp() * (1.0 - (t / 0.30).powi(4));
        (sine(f, t) * 0.45 + sine(f * 2.0, t) * 0.12) * env
    }))
}

fn sfx_kill() -> Vec<u8> {
    wav_bytes(&render_samples(0.55, |t| {
        let env = (-t * 6.5).exp();
        let thump = sine(75.0 - 40.0 * t, t) * (-t * 8.0).exp() * 0.9;
        let crackle = white(t) * env * 0.55;
        (thump + crackle).tanh() * 0.9
    }))
}

fn sfx_hurt() -> Vec<u8> {
    wav_bytes(&render_samples(0.30, |t| {
        let env = (-t * 9.0).exp();
        (sine(110.0 - 90.0 * t, t) * 0.6 + white(t) * 0.25) * env
    }))
}

fn sfx_dash() -> Vec<u8> {
    wav_bytes(&render_samples(0.26, |t| {
        let k = t / 0.26;
        let hump = (std::f32::consts::PI * k).sin();
        white(t) * hump * 0.28 * (0.4 + 0.6 * sine(250.0 + 1400.0 * k, t).abs())
    }))
}

fn sfx_turret() -> Vec<u8> {
    wav_bytes(&render_samples(0.16, |t| {
        let k = t / 0.16;
        let f = 320.0 + 260.0 * (1.0 - k);
        let saw = 2.0 * ((f * t).fract()) - 1.0;
        saw * 0.30 * (1.0 - k)
    }))
}

fn sfx_clear() -> Vec<u8> {
    let notes = [523.25_f32, 659.25, 783.99, 1046.5];
    wav_bytes(&render_samples(0.75, |t| {
        let mut s = 0.0;
        for (i, f) in notes.iter().enumerate() {
            let start = i as f32 * 0.11;
            if t >= start {
                let lt = t - start;
                s += (sine(*f, t) * 0.4 + sine(f * 2.0, t) * 0.1) * (-lt * 7.0).exp();
            }
        }
        s * (1.0 - (t / 0.75).powi(6))
    }))
}

fn sfx_death() -> Vec<u8> {
    wav_bytes(&render_samples(1.0, |t| {
        let f = 330.0 * (0.18_f32).powf(t);
        let vib = 1.0 + 0.02 * sine(6.0, t);
        let ph = std::f32::consts::TAU * f * vib * t;
        let sq = if ph.sin() > 0.0 { 1.0 } else { -1.0 };
        (sq * 0.22 + sine(f * 0.5, t) * 0.3) * (1.0 - t).max(0.0)
    }))
}

fn sfx_step() -> Vec<u8> {
    wav_bytes(&render_samples(0.05, |t| {
        white(t) * (-t * 90.0).exp() * 0.5 * (0.5 + 0.5 * sine(140.0, t))
    }))
}

/// 9.6 s synth loop: Am — F — C — G, pad + bass + arpeggio + hats.
fn music_loop() -> Vec<u8> {
    let beat = 0.6_f32; // 100 bpm
    let bars: [( [f32; 3], f32 ); 4] = [
        ([110.00, 130.81, 164.81], 55.00), // Am
        ([87.31, 110.00, 130.81], 43.65),  // F
        ([130.81, 164.81, 196.00], 65.41), // C
        ([98.00, 123.47, 146.83], 49.00),  // G
    ];
    let arp_oct = [2.0_f32, 4.0, 3.0, 4.0, 2.0, 4.0, 3.0, 6.0];
    let total = beat * 16.0;
    let n = (total * SR as f32) as usize;
    let mut buf = vec![0.0_f32; n];
    for (i, s) in buf.iter_mut().enumerate() {
        let t = i as f32 / SR as f32;
        let bar = ((t / (beat * 4.0)) as usize) % 4;
        let bar_t = t % (beat * 4.0);
        let (chord, root) = bars[bar];
        // Pad: slow swell per bar, detuned pair per note.
        let env_pad = (bar_t / 0.5).min(1.0) * (1.0 - ((bar_t - beat * 4.0 + 0.45) / 0.45).max(0.0));
        let mut pad = 0.0;
        for f in chord {
            pad += sine(f, t) + sine(f * 1.004, t) * 0.8;
        }
        pad *= 0.040 * env_pad;
        // Bass pluck on each beat.
        let beat_t = t % beat;
        let bass = sine(root, t) * (-beat_t * 5.0).exp() * 0.20;
        // Arpeggio on eighths.
        let eighth = beat / 2.0;
        let step8 = ((t / eighth) as usize) % 8;
        let at = t % eighth;
        let base = chord[step8 % 3];
        let af = base * arp_oct[step8];
        let arp = (sine(af, t) + sine(af * 2.0, t) * 0.25) * (-at * 11.0).exp() * 0.055;
        // Hats on offbeats.
        let off_t = (t + beat / 2.0) % beat;
        let hat = white(t) * (-off_t * 120.0).exp() * 0.030;
        *s = (pad + bass + arp + hat).tanh();
    }
    wav_bytes(&buf)
}

struct Sounds {
    shoot: Sound,
    pickup: Sound,
    health: Sound,
    kill: Sound,
    hurt: Sound,
    dash: Sound,
    turret: Sound,
    clear: Sound,
    death: Sound,
    step: Sound,
    music: Sound,
}

async fn load_sounds() -> Option<Sounds> {
    Some(Sounds {
        shoot: load_sound_from_bytes(&sfx_shoot()).await.ok()?,
        pickup: load_sound_from_bytes(&sfx_pickup()).await.ok()?,
        health: load_sound_from_bytes(&sfx_health()).await.ok()?,
        kill: load_sound_from_bytes(&sfx_kill()).await.ok()?,
        hurt: load_sound_from_bytes(&sfx_hurt()).await.ok()?,
        dash: load_sound_from_bytes(&sfx_dash()).await.ok()?,
        turret: load_sound_from_bytes(&sfx_turret()).await.ok()?,
        clear: load_sound_from_bytes(&sfx_clear()).await.ok()?,
        death: load_sound_from_bytes(&sfx_death()).await.ok()?,
        step: load_sound_from_bytes(&sfx_step()).await.ok()?,
        music: load_sound_from_bytes(&music_loop()).await.ok()?,
    })
}

fn play(snd: &Option<Sounds>, pick: impl Fn(&Sounds) -> &Sound, volume: f32) {
    if let Some(s) = snd {
        play_sound(
            pick(s),
            PlaySoundParams { looped: false, volume: volume.clamp(0.0, 1.0) },
        );
    }
}

// ------------------------------------------------------------- mesh builder

struct MeshBuilder {
    v: Vec<Vertex>,
    i: Vec<u16>,
}

impl MeshBuilder {
    fn new() -> Self {
        MeshBuilder { v: Vec::new(), i: Vec::new() }
    }

    fn vert(&mut self, p: Vec3, n: Vec3, c: Color) -> u16 {
        let idx = self.v.len() as u16;
        self.v.push(Vertex {
            position: p,
            uv: vec2(0.0, 0.0),
            color: [
                (c.r * 255.0) as u8,
                (c.g * 255.0) as u8,
                (c.b * 255.0) as u8,
                (c.a * 255.0) as u8,
            ],
            normal: vec4(n.x, n.y, n.z, 0.0),
        });
        idx
    }

    fn quad(&mut self, p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, n: Vec3, c: Color) {
        let a = self.vert(p0, n, c);
        let b = self.vert(p1, n, c);
        let cc = self.vert(p2, n, c);
        let d = self.vert(p3, n, c);
        self.i.extend_from_slice(&[a, b, cc, a, cc, d]);
    }

    /// Box from corner `o` spanned by edges e1,e2,e3 (right-handed-ish).
    fn box_at(&mut self, o: Vec3, e1: Vec3, e2: Vec3, e3: Vec3, c: Color) {
        let n1 = e1.normalize_or_zero();
        let n2 = e2.normalize_or_zero();
        let n3 = e3.normalize_or_zero();
        // -e3 and +e3 faces
        self.quad(o, o + e1, o + e1 + e2, o + e2, -n3, c);
        self.quad(o + e3, o + e2 + e3, o + e1 + e2 + e3, o + e1 + e3, n3, c);
        // -e2 / +e2
        self.quad(o, o + e3, o + e1 + e3, o + e1, -n2, c);
        self.quad(o + e2, o + e1 + e2, o + e1 + e2 + e3, o + e2 + e3, n2, c);
        // -e1 / +e1
        self.quad(o, o + e2, o + e2 + e3, o + e3, -n1, c);
        self.quad(o + e1, o + e1 + e3, o + e1 + e2 + e3, o + e1 + e2, n1, c);
    }

    fn box_center(&mut self, center: Vec3, e1: Vec3, e2: Vec3, e3: Vec3, c: Color) {
        self.box_at(center - (e1 + e2 + e3) * 0.5, e1, e2, e3, c);
    }

    fn sphere(&mut self, center: Vec3, r: f32, rings: usize, slices: usize, c: Color) {
        let base = self.v.len() as u16;
        for ri in 0..=rings {
            let phi = std::f32::consts::PI * ri as f32 / rings as f32;
            for si in 0..=slices {
                let theta = std::f32::consts::TAU * si as f32 / slices as f32;
                let n = vec3(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin());
                self.vert(center + n * r, n, c);
            }
        }
        let w = (slices + 1) as u16;
        for ri in 0..rings as u16 {
            for si in 0..slices as u16 {
                let a = base + ri * w + si;
                let b = a + w;
                self.i.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    }

    fn build(self) -> Mesh {
        Mesh { vertices: self.v, indices: self.i, texture: None }
    }
}

// ------------------------------------------------------------------ shaders

const WORLD_VERT: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
varying lowp vec4 vcolor;
varying highp vec3 vpos;
varying highp vec3 vnorm;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    vec4 wp = Model * vec4(position, 1.0);
    vpos = wp.xyz;
    vnorm = normal.xyz;
    vcolor = color0 / 255.0;
    gl_Position = Projection * wp;
}"#;

const WORLD_FRAG: &str = r#"#version 100
precision highp float;
varying lowp vec4 vcolor;
varying highp vec3 vpos;
varying highp vec3 vnorm;
uniform vec4 LightPos[12];
uniform vec4 LightCol[12];
uniform vec3 CamPos;
uniform vec4 FogInfo;
uniform float GameTime;
void main() {
    vec3 N = normalize(vnorm);
    vec3 acc = vec3(0.055, 0.05, 0.105);
    for (int i = 0; i < 12; i++) {
        vec3 L = LightPos[i].xyz - vpos;
        float dist = length(L);
        float att = clamp(1.0 - dist / max(LightPos[i].w, 0.001), 0.0, 1.0);
        att *= att;
        float ndl = max(dot(N, L / max(dist, 0.001)), 0.0) * 0.75 + 0.25;
        acc += LightCol[i].rgb * (LightCol[i].w * att * ndl);
    }
    vec3 col = vcolor.rgb * acc;

    float side = 1.0 - abs(N.y);
    float trim = smoothstep(2.22, 2.36, vpos.y) * side;
    col += vec3(0.50, 0.22, 1.0) * trim * (0.55 + 0.18 * sin(GameTime * 2.0 + vpos.x * 0.7 + vpos.z * 0.9));

    float up = step(0.7, N.y) * (1.0 - step(0.1, vpos.y));
    vec2 f = fract(vpos.xz / 2.0);
    vec2 dd = min(f, 1.0 - f) * 2.0;
    float line = 1.0 - smoothstep(0.0, 0.07, min(dd.x, dd.y));
    col += vec3(0.0, 0.50, 0.60) * line * up * (0.30 + 0.10 * sin(GameTime * 1.7 + vpos.x * 0.4 + vpos.z * 0.3));

    float fd = distance(CamPos, vpos);
    float fog = clamp(1.0 - exp(-fd * fd * FogInfo.w), 0.0, 1.0);
    col = mix(col, FogInfo.rgb, fog);
    gl_FragColor = vec4(col, vcolor.a);
}"#;

const GLOW_VERT: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying lowp vec4 vcolor;
varying lowp vec2 uv;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    vcolor = color0 / 255.0;
    uv = texcoord;
    gl_Position = Projection * Model * vec4(position, 1.0);
}"#;

const GLOW_FRAG: &str = r#"#version 100
precision mediump float;
varying lowp vec4 vcolor;
varying lowp vec2 uv;
uniform sampler2D Texture;
void main() {
    vec4 t = texture2D(Texture, uv);
    gl_FragColor = vec4(vcolor.rgb * t.a * vcolor.a, 1.0);
}"#;

struct Renderer {
    world_mat: Option<Material>,
    glow_mat: Option<Material>,
    glow_tex: Texture2D,
    vignette_tex: Texture2D,
    stars: Vec<(f32, f32, f32, f32)>, // azimuth, elevation, size, phase
}

impl Renderer {
    fn new() -> Renderer {
        let world_mat = load_material(
            ShaderSource::Glsl { vertex: WORLD_VERT, fragment: WORLD_FRAG },
            MaterialParams {
                pipeline_params: PipelineParams {
                    depth_write: true,
                    depth_test: Comparison::LessOrEqual,
                    ..Default::default()
                },
                uniforms: vec![
                    UniformDesc::new("LightPos", UniformType::Float4).array(12),
                    UniformDesc::new("LightCol", UniformType::Float4).array(12),
                    UniformDesc::new("CamPos", UniformType::Float3),
                    UniformDesc::new("FogInfo", UniformType::Float4),
                    UniformDesc::new("GameTime", UniformType::Float1),
                ],
                textures: vec![],
            },
        )
        .ok();

        let glow_mat = load_material(
            ShaderSource::Glsl { vertex: GLOW_VERT, fragment: GLOW_FRAG },
            MaterialParams {
                pipeline_params: PipelineParams {
                    depth_write: false,
                    depth_test: Comparison::LessOrEqual,
                    color_blend: Some(BlendState::new(
                        Equation::Add,
                        BlendFactor::One,
                        BlendFactor::One,
                    )),
                    ..Default::default()
                },
                uniforms: vec![],
                textures: vec![],
            },
        )
        .ok();

        // Radial gradient sprite for glows.
        let s = 64usize;
        let mut img = Image::gen_image_color(s as u16, s as u16, Color::new(1.0, 1.0, 1.0, 0.0));
        for y in 0..s {
            for x in 0..s {
                let dx = (x as f32 / (s - 1) as f32) * 2.0 - 1.0;
                let dy = (y as f32 / (s - 1) as f32) * 2.0 - 1.0;
                let r = (dx * dx + dy * dy).sqrt().min(1.0);
                let a = (1.0 - r).powf(2.2);
                img.set_pixel(x as u32, y as u32, Color::new(1.0, 1.0, 1.0, a));
            }
        }
        let glow_tex = Texture2D::from_image(&img);
        glow_tex.set_filter(FilterMode::Linear);

        // Vignette overlay.
        let vs = 128usize;
        let mut vimg = Image::gen_image_color(vs as u16, vs as u16, Color::new(0.0, 0.0, 0.0, 0.0));
        for y in 0..vs {
            for x in 0..vs {
                let dx = (x as f32 / (vs - 1) as f32) * 2.0 - 1.0;
                let dy = (y as f32 / (vs - 1) as f32) * 2.0 - 1.0;
                let r = (dx * dx + dy * dy).sqrt();
                let a = ((r - 0.55) / 0.55).clamp(0.0, 1.0).powf(1.8) * 0.55;
                vimg.set_pixel(x as u32, y as u32, Color::new(0.0, 0.0, 0.05, a));
            }
        }
        let vignette_tex = Texture2D::from_image(&vimg);
        vignette_tex.set_filter(FilterMode::Linear);

        let mut stars = Vec::new();
        for i in 0..150u32 {
            stars.push((
                hash01(i * 7 + 1) * std::f32::consts::TAU,
                0.04 + hash01(i * 13 + 5) * 1.1,
                1.0 + hash01(i * 29 + 11) * 1.8,
                hash01(i * 37 + 3) * 6.28,
            ));
        }

        Renderer { world_mat, glow_mat, glow_tex, vignette_tex, stars }
    }
}

// --------------------------------------------------------------------- maze

struct Maze {
    n: usize,
    walls: Vec<bool>,
}

impl Maze {
    fn is_wall(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.n as i32 || y >= self.n as i32 {
            return true;
        }
        self.walls[y as usize * self.n + x as usize]
    }

    fn set(&mut self, x: i32, y: i32, w: bool) {
        if x >= 0 && y >= 0 && x < self.n as i32 && y < self.n as i32 {
            self.walls[y as usize * self.n + x as usize] = w;
        }
    }

    fn half(&self) -> f32 {
        self.n as f32 * CELL * 0.5
    }

    fn cell_center(&self, x: i32, y: i32) -> Vec2 {
        vec2(
            x as f32 * CELL + CELL * 0.5 - self.half(),
            y as f32 * CELL + CELL * 0.5 - self.half(),
        )
    }

    fn world_to_cell(&self, p: Vec2) -> (i32, i32) {
        (
            ((p.x + self.half()) / CELL).floor() as i32,
            ((p.y + self.half()) / CELL).floor() as i32,
        )
    }

    fn open_cells(&self) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for y in 0..self.n as i32 {
            for x in 0..self.n as i32 {
                if !self.is_wall(x, y) {
                    out.push((x, y));
                }
            }
        }
        out
    }

    fn generate(n: usize) -> Maze {
        let mut m = Maze { n, walls: vec![true; n * n] };
        let mut stack: Vec<(i32, i32)> = vec![(1, 1)];
        m.set(1, 1, false);
        while let Some(&(cx, cy)) = stack.last() {
            let mut dirs = [(2, 0), (-2, 0), (0, 2), (0, -2)];
            shuffle(&mut dirs);
            let mut moved = false;
            for (dx, dy) in dirs {
                let (nx, ny) = (cx + dx, cy + dy);
                if nx > 0 && ny > 0 && nx < n as i32 - 1 && ny < n as i32 - 1 && m.is_wall(nx, ny) {
                    m.set(cx + dx / 2, cy + dy / 2, false);
                    m.set(nx, ny, false);
                    stack.push((nx, ny));
                    moved = true;
                    break;
                }
            }
            if !moved {
                stack.pop();
            }
        }
        for _ in 0..(n * n / 6) {
            let x = gen_range(1, n as i32 - 1);
            let y = gen_range(1, n as i32 - 1);
            if m.is_wall(x, y) {
                let open = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .filter(|(dx, dy)| !m.is_wall(x + dx, y + dy))
                    .count();
                if open >= 2 {
                    m.set(x, y, false);
                }
            }
        }
        m
    }

    fn resolve(&self, mut p: Vec2, r: f32) -> Vec2 {
        for _ in 0..2 {
            let (cx, cy) = self.world_to_cell(p);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (gx, gy) = (cx + dx, cy + dy);
                    if !self.is_wall(gx, gy) {
                        continue;
                    }
                    let c = self.cell_center(gx, gy);
                    let closest = vec2(
                        p.x.clamp(c.x - CELL / 2.0, c.x + CELL / 2.0),
                        p.y.clamp(c.y - CELL / 2.0, c.y + CELL / 2.0),
                    );
                    let d = p - closest;
                    let dist = d.length();
                    if dist < r {
                        if dist > 1e-4 {
                            p = closest + d / dist * r;
                        } else {
                            p.y = c.y + CELL / 2.0 + r;
                        }
                    }
                }
            }
        }
        p
    }

    fn los(&self, a: Vec2, b: Vec2) -> bool {
        let d = b - a;
        let len = d.length();
        if len < 0.001 {
            return true;
        }
        let steps = (len / 0.2).ceil() as i32;
        for i in 1..steps {
            let p = a + d * (i as f32 / steps as f32);
            let (cx, cy) = self.world_to_cell(p);
            if self.is_wall(cx, cy) {
                return false;
            }
        }
        true
    }

    /// BFS path between cells; returns the cell sequence excluding `from`.
    fn bfs(&self, from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
        if from == to {
            return Vec::new();
        }
        let n = self.n as i32;
        let idx = |c: (i32, i32)| (c.1 * n + c.0) as usize;
        let mut parent: Vec<i32> = vec![-1; (n * n) as usize];
        let mut q = VecDeque::new();
        parent[idx(from)] = idx(from) as i32;
        q.push_back(from);
        while let Some(c) = q.pop_front() {
            if c == to {
                let mut path = vec![c];
                let mut cur = c;
                loop {
                    let p = parent[idx(cur)] as usize;
                    let pc = ((p as i32 % n), (p as i32 / n));
                    if pc == from {
                        break;
                    }
                    path.push(pc);
                    cur = pc;
                }
                path.reverse();
                return path;
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let nc = (c.0 + dx, c.1 + dy);
                if !self.is_wall(nc.0, nc.1) && parent[idx(nc)] < 0 {
                    parent[idx(nc)] = idx(c) as i32;
                    q.push_back(nc);
                }
            }
        }
        Vec::new()
    }
}

// ----------------------------------------------------------------- entities

struct Crystal {
    pos: Vec2,
    phase: f32,
    taken: bool,
}

#[derive(PartialEq, Clone, Copy)]
enum DroneState {
    Patrol,
    Chase,
    Investigate,
}

struct Drone {
    pos: Vec2,
    dir: Vec2,
    state: DroneState,
    path: Vec<(i32, i32)>,
    path_i: usize,
    repath_t: f32,
    last_seen: Vec2,
    lost_t: f32,
    investigate_t: f32,
    stuck_t: f32,
    phase: f32,
    hp: i32,
    hit_flash: f32,
}

impl Drone {
    fn new(pos: Vec2) -> Drone {
        Drone {
            pos,
            dir: vec2(1.0, 0.0),
            state: DroneState::Patrol,
            path: Vec::new(),
            path_i: 0,
            repath_t: 0.0,
            last_seen: pos,
            lost_t: 0.0,
            investigate_t: 0.0,
            stuck_t: 0.0,
            phase: gen_range(0.0, 6.28),
            hp: DRONE_HP,
            hit_flash: 0.0,
        }
    }
}

struct Turret {
    pos: Vec2,
    aim: Vec2,
    fire_cd: f32,
    hp: i32,
    alive: bool,
    hit_flash: f32,
}

struct Projectile {
    pos: Vec2,
    vel: Vec2,
    ttl: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum PickupKind {
    Health,
    Overdrive,
}

struct Pickup {
    pos: Vec2,
    kind: PickupKind,
    phase: f32,
    taken: bool,
}

struct Particle {
    pos: Vec3,
    vel: Vec3,
    life: f32,
    max: f32,
    size: f32,
    color: Color,
    grav: f32,
}

struct Popup {
    text: String,
    t: f32,
}

struct WorldPopup {
    pos: Vec3,
    text: String,
    t: f32,
}

struct Tracer {
    from: Vec3,
    to: Vec3,
    ttl: f32,
}

struct Explosion {
    pos: Vec3,
    t: f32,
    big: bool,
}

#[derive(Default, Clone, Copy)]
struct RunStats {
    crystals: u32,
    kills: u32,
    turrets: u32,
}

struct LightSrc {
    pos: Vec3,
    color: Vec3,
    radius: f32,
    intensity: f32,
}

// --------------------------------------------------------------------- game

struct Game {
    level: u32,
    score: i64,
    hp: f32,
    maze: Maze,
    wall_chunks: Vec<(Vec2, f32, Mesh)>, // center, radius, mesh
    floor_mesh: Mesh,
    ppos: Vec2,
    vel: Vec2,
    yaw: f32,
    pitch: f32,
    dash_t: f32,
    dash_cd: f32,
    dash_dir: Vec2,
    shot_cd: f32,
    recoil: f32,
    muzzle_flash: f32,
    invuln: f32,
    dmg_flash: f32,
    pick_flash: f32,
    hitmark_t: f32,
    bob_t: f32,
    prev_step_phase: f32,
    move_frac: f32,
    roll: f32,
    fov: f32,
    shake: f32,
    combo: f32,
    combo_t: f32,
    overdrive_t: f32,
    last_hit_dir: Option<(f32, f32)>, // world angle, ttl
    time_in_level: f32,
    intro_t: f32,
    total_crystals: usize,
    crystals: Vec<Crystal>,
    drones: Vec<Drone>,
    turrets: Vec<Turret>,
    projectiles: Vec<Projectile>,
    pickups: Vec<Pickup>,
    respawns: Vec<f32>,
    particles: Vec<Particle>,
    popups: Vec<Popup>,
    world_popups: Vec<WorldPopup>,
    tracers: Vec<Tracer>,
    explosions: Vec<Explosion>,
    last_bonus: (i64, i64),
    stats: RunStats,
    cam_matrix: Mat4,
    pending_hitstop: f32,
}

fn build_world_meshes(maze: &Maze) -> (Vec<(Vec2, f32, Mesh)>, Mesh) {
    let n = maze.n as i32;
    let chunk_cells = 8i32;
    let chunks_per_side = (n + chunk_cells - 1) / chunk_cells;
    let mut chunks = Vec::new();

    for cy in 0..chunks_per_side {
        for cx in 0..chunks_per_side {
            let mut mb = MeshBuilder::new();
            for y in (cy * chunk_cells)..((cy + 1) * chunk_cells).min(n) {
                for x in (cx * chunk_cells)..((cx + 1) * chunk_cells).min(n) {
                    if !maze.is_wall(x, y) {
                        continue;
                    }
                    let c = maze.cell_center(x, y);
                    let h = WALL_H;
                    let v = hash01((x * 31 + y * 977) as u32) * 0.10 - 0.05;
                    let side = cmul(COL_WALL, 1.0 + v);
                    let top = cmul(COL_WALL_TOP, 1.0 + v * 0.5);
                    let (x0, x1) = (c.x - CELL / 2.0, c.x + CELL / 2.0);
                    let (z0, z1) = (c.y - CELL / 2.0, c.y + CELL / 2.0);
                    // Only faces adjacent to open space.
                    if !maze.is_wall(x, y + 1) {
                        mb.quad(
                            vec3(x0, 0.0, z1), vec3(x1, 0.0, z1),
                            vec3(x1, h, z1), vec3(x0, h, z1),
                            vec3(0.0, 0.0, 1.0), side,
                        );
                    }
                    if !maze.is_wall(x, y - 1) {
                        mb.quad(
                            vec3(x1, 0.0, z0), vec3(x0, 0.0, z0),
                            vec3(x0, h, z0), vec3(x1, h, z0),
                            vec3(0.0, 0.0, -1.0), side,
                        );
                    }
                    if !maze.is_wall(x + 1, y) {
                        mb.quad(
                            vec3(x1, 0.0, z1), vec3(x1, 0.0, z0),
                            vec3(x1, h, z0), vec3(x1, h, z1),
                            vec3(1.0, 0.0, 0.0), side,
                        );
                    }
                    if !maze.is_wall(x - 1, y) {
                        mb.quad(
                            vec3(x0, 0.0, z0), vec3(x0, 0.0, z1),
                            vec3(x0, h, z1), vec3(x0, h, z0),
                            vec3(-1.0, 0.0, 0.0), side,
                        );
                    }
                    mb.quad(
                        vec3(x0, h, z0), vec3(x0, h, z1),
                        vec3(x1, h, z1), vec3(x1, h, z0),
                        vec3(0.0, 1.0, 0.0), top,
                    );
                }
            }
            if !mb.i.is_empty() {
                let center = vec2(
                    (cx as f32 + 0.5) * chunk_cells as f32 * CELL - maze.half(),
                    (cy as f32 + 0.5) * chunk_cells as f32 * CELL - maze.half(),
                );
                let radius = chunk_cells as f32 * CELL * 0.75;
                chunks.push((center, radius, mb.build()));
            }
        }
    }

    let mut fb = MeshBuilder::new();
    let e = maze.half() * 1.6;
    fb.quad(
        vec3(-e, 0.0, -e), vec3(-e, 0.0, e), vec3(e, 0.0, e), vec3(e, 0.0, -e),
        vec3(0.0, 1.0, 0.0), COL_FLOOR,
    );
    (chunks, fb.build())
}

impl Game {
    fn drone_chase_speed(&self) -> f32 {
        (3.4 + self.level as f32 * 0.18).min(5.5)
    }
    fn drone_sight(&self) -> f32 {
        (7.0 + self.level as f32 * 0.4).min(11.0)
    }
    fn drone_damage(&self) -> f32 {
        (16.0 + self.level as f32 * 1.5).min(30.0)
    }

    fn new(level: u32, score: i64, hp: f32, stats: RunStats) -> Game {
        let n = (13 + 2 * level as usize).min(27);
        let maze = Maze::generate(n);
        let spawn_cell = (1, 1);
        let ppos = maze.cell_center(1, 1);
        let (wall_chunks, floor_mesh) = build_world_meshes(&maze);

        let n_crystals = (6 + 2 * level as usize).min(20);
        let mut cells: Vec<(i32, i32)> = maze
            .open_cells()
            .into_iter()
            .filter(|&(x, y)| (x - spawn_cell.0).abs() + (y - spawn_cell.1).abs() > 6)
            .collect();
        shuffle(&mut cells);
        let crystals: Vec<Crystal> = cells
            .iter()
            .take(n_crystals)
            .map(|&(x, y)| Crystal {
                pos: maze.cell_center(x, y),
                phase: gen_range(0.0, 6.28),
                taken: false,
            })
            .collect();
        let mut used: Vec<(i32, i32)> = cells.iter().take(n_crystals).cloned().collect();

        let n_drones = (2 + level as usize).min(10);
        let mut dcells: Vec<(i32, i32)> = maze
            .open_cells()
            .into_iter()
            .filter(|&(x, y)| (maze.cell_center(x, y) - ppos).length() > 10.0)
            .collect();
        shuffle(&mut dcells);
        let drones: Vec<Drone> = dcells
            .iter()
            .take(n_drones)
            .map(|&(x, y)| Drone::new(maze.cell_center(x, y)))
            .collect();

        // Turrets from level 3.
        let n_turrets = if level >= 3 { ((level - 2) as usize).min(5) } else { 0 };
        let mut tcells: Vec<(i32, i32)> = maze
            .open_cells()
            .into_iter()
            .filter(|&(x, y)| {
                let c = maze.cell_center(x, y);
                (c - ppos).length() > 12.0 && !used.contains(&(x, y))
            })
            .collect();
        shuffle(&mut tcells);
        let turrets: Vec<Turret> = tcells
            .iter()
            .take(n_turrets)
            .map(|&(x, y)| {
                used.push((x, y));
                Turret {
                    pos: maze.cell_center(x, y),
                    aim: vec2(1.0, 0.0),
                    fire_cd: 1.0,
                    hp: TURRET_HP,
                    alive: true,
                    hit_flash: 0.0,
                }
            })
            .collect();

        // Pickups.
        let mut pickups = Vec::new();
        let n_health = 1 + (level as usize / 3).min(2);
        let mut pcells: Vec<(i32, i32)> = maze
            .open_cells()
            .into_iter()
            .filter(|&(x, y)| {
                (x - spawn_cell.0).abs() + (y - spawn_cell.1).abs() > 4 && !used.contains(&(x, y))
            })
            .collect();
        shuffle(&mut pcells);
        let mut pi = 0;
        for _ in 0..n_health {
            if pi < pcells.len() {
                pickups.push(Pickup {
                    pos: maze.cell_center(pcells[pi].0, pcells[pi].1),
                    kind: PickupKind::Health,
                    phase: gen_range(0.0, 6.28),
                    taken: false,
                });
                pi += 1;
            }
        }
        if level >= 2 && gen_range(0.0, 1.0) < 0.65 && pi < pcells.len() {
            pickups.push(Pickup {
                pos: maze.cell_center(pcells[pi].0, pcells[pi].1),
                kind: PickupKind::Overdrive,
                phase: gen_range(0.0, 6.28),
                taken: false,
            });
        }

        let yaw = if !maze.is_wall(2, 1) { 0.0 } else { std::f32::consts::FRAC_PI_2 };
        let total = crystals.len();

        Game {
            level,
            score,
            hp,
            maze,
            wall_chunks,
            floor_mesh,
            ppos,
            vel: Vec2::ZERO,
            yaw,
            pitch: 0.0,
            dash_t: 0.0,
            dash_cd: 0.0,
            dash_dir: vec2(1.0, 0.0),
            shot_cd: 0.0,
            recoil: 0.0,
            muzzle_flash: 0.0,
            invuln: 0.0,
            dmg_flash: 0.0,
            pick_flash: 0.0,
            hitmark_t: 0.0,
            bob_t: 0.0,
            prev_step_phase: 0.0,
            move_frac: 0.0,
            roll: 0.0,
            fov: BASE_FOV,
            shake: 0.0,
            combo: 1.0,
            combo_t: 0.0,
            overdrive_t: 0.0,
            last_hit_dir: None,
            time_in_level: 0.0,
            intro_t: 2.6,
            total_crystals: total,
            crystals,
            drones,
            turrets,
            projectiles: Vec::new(),
            pickups,
            respawns: Vec::new(),
            particles: Vec::new(),
            popups: Vec::new(),
            world_popups: Vec::new(),
            tracers: Vec::new(),
            explosions: Vec::new(),
            last_bonus: (0, 0),
            stats,
            cam_matrix: Mat4::IDENTITY,
            pending_hitstop: 0.0,
        }
    }

    fn look_dir(&self) -> Vec3 {
        let p = self.pitch + self.recoil;
        vec3(self.yaw.cos() * p.cos(), p.sin(), self.yaw.sin() * p.cos())
    }

    fn eye(&self) -> Vec3 {
        let t = get_time() as f32;
        let bob = (self.bob_t * 9.5).sin() * 0.040 * self.move_frac;
        let sx = ((t * 37.0).sin() + (t * 61.0).sin() * 0.5) * self.shake * 0.05;
        let sy = ((t * 43.0).sin() + (t * 53.0).sin() * 0.5) * self.shake * 0.05;
        let right = vec2(-self.yaw.sin(), self.yaw.cos());
        vec3(
            self.ppos.x + right.x * sx,
            EYE_H + bob + sy,
            self.ppos.y + right.y * sx,
        )
    }

    fn burst(&mut self, pos: Vec3, color: Color, count: usize, speed: f32) {
        for _ in 0..count {
            let dir = vec3(
                gen_range(-1.0, 1.0),
                gen_range(-0.4, 1.2),
                gen_range(-1.0, 1.0),
            )
            .normalize_or_zero();
            let life = gen_range(0.35, 0.8);
            self.particles.push(Particle {
                pos,
                vel: dir * gen_range(speed * 0.4, speed),
                life,
                max: life,
                size: gen_range(0.05, 0.13),
                color,
                grav: -7.0,
            });
        }
    }

    fn popup(&mut self, text: String) {
        self.popups.push(Popup { text, t: 1.0 });
    }

    fn fire_cd(&self) -> f32 {
        if self.overdrive_t > 0.0 { 0.10 } else { SHOT_CD }
    }

    fn muzzle_world(&self) -> Vec3 {
        let f = self.look_dir();
        let r = vec3(-self.yaw.sin(), 0.0, self.yaw.cos());
        let u = r.cross(f).normalize_or_zero();
        self.eye() + f * 0.55 + r * 0.16 + u * -0.12
    }

    fn shoot(&mut self, snd: &Option<Sounds>) {
        self.shot_cd = self.fire_cd();
        self.recoil = (self.recoil + 0.022).min(0.05);
        self.muzzle_flash = 1.0;
        play(snd, |s| &s.shoot, 0.5);
        let eye = self.eye();
        let dir = self.look_dir();

        let mut wall_t = SHOT_RANGE;
        let mut t = 0.3;
        while t < SHOT_RANGE {
            let p = eye + dir * t;
            if p.y <= 0.0 {
                wall_t = t;
                break;
            }
            if p.y < WALL_H {
                let (cx, cy) = self.maze.world_to_cell(vec2(p.x, p.z));
                if self.maze.is_wall(cx, cy) {
                    wall_t = t;
                    break;
                }
            }
            t += 0.15;
        }

        // Nearest target (drone or turret head) before the wall.
        let gt = get_time() as f32;
        let mut best: Option<(usize, f32, bool)> = None; // idx, t, is_turret
        for (i, d) in self.drones.iter().enumerate() {
            let center = vec3(d.pos.x, 0.9 + (gt * 3.0 + d.phase).sin() * 0.1, d.pos.y);
            let oc = eye - center;
            let b = oc.dot(dir);
            let c = oc.dot(oc) - (DRONE_R + 0.08) * (DRONE_R + 0.08);
            let disc = b * b - c;
            if disc >= 0.0 {
                let th = -b - disc.sqrt();
                if th > 0.0 && th < wall_t && best.map_or(true, |(_, bt, _)| th < bt) {
                    best = Some((i, th, false));
                }
            }
        }
        for (i, tr) in self.turrets.iter().enumerate() {
            if !tr.alive {
                continue;
            }
            let center = vec3(tr.pos.x, 1.05, tr.pos.y);
            let oc = eye - center;
            let b = oc.dot(dir);
            let c = oc.dot(oc) - 0.40 * 0.40;
            let disc = b * b - c;
            if disc >= 0.0 {
                let th = -b - disc.sqrt();
                if th > 0.0 && th < wall_t && best.map_or(true, |(_, bt, _)| th < bt) {
                    best = Some((i, th, true));
                }
            }
        }

        let muzzle = self.muzzle_world();
        let hit_t = best.map_or(wall_t, |(_, t, _)| t);
        let hit_p = eye + dir * hit_t;
        self.tracers.push(Tracer { from: muzzle, to: hit_p, ttl: 0.06 });

        // Shots alert nearby drones.
        let ppos = self.ppos;
        for d in self.drones.iter_mut() {
            if (d.pos - ppos).length() < 9.0 && d.state != DroneState::Chase {
                d.state = DroneState::Chase;
                d.last_seen = ppos;
                d.repath_t = 0.0;
                d.lost_t = 0.0;
            }
        }

        match best {
            Some((i, _, false)) => {
                self.drones[i].hp -= 1;
                self.drones[i].hit_flash = 1.0;
                self.drones[i].state = DroneState::Chase;
                self.drones[i].last_seen = ppos;
                self.drones[i].lost_t = 0.0;
                let push = vec2(dir.x, dir.z).normalize_or_zero() * 0.25;
                self.drones[i].pos = self.maze.resolve(self.drones[i].pos + push, DRONE_R);
                self.hitmark_t = 0.12;
                if self.drones[i].hp <= 0 {
                    let d = self.drones.remove(i);
                    let kill = ((50 * self.level as i64 + 100) as f32 * self.combo) as i64;
                    self.score += kill;
                    self.stats.kills += 1;
                    self.combo_t = 6.0;
                    let kp = vec3(d.pos.x, 0.9, d.pos.y);
                    self.world_popups.push(WorldPopup {
                        pos: kp,
                        text: format!("+{}", kill),
                        t: 1.0,
                    });
                    self.burst(kp, Color::new(1.0, 0.4, 0.15, 1.0), 26, 6.0);
                    self.explosions.push(Explosion { pos: kp, t: 0.0, big: false });
                    self.respawns.push(DRONE_RESPAWN);
                    self.pending_hitstop = 0.07;
                    self.shake = (self.shake + 0.25).min(1.0);
                    play(snd, |s| &s.kill, 0.6);
                } else {
                    let d = &self.drones[i];
                    self.burst(vec3(d.pos.x, 0.9, d.pos.y), Color::new(1.0, 0.8, 0.4, 1.0), 6, 3.5);
                }
            }
            Some((i, _, true)) => {
                self.turrets[i].hp -= 1;
                self.turrets[i].hit_flash = 1.0;
                self.hitmark_t = 0.12;
                if self.turrets[i].hp <= 0 {
                    self.turrets[i].alive = false;
                    let kill = ((50 * self.level as i64 + 200) as f32 * self.combo) as i64;
                    self.score += kill;
                    self.stats.turrets += 1;
                    self.combo_t = 6.0;
                    let kp = vec3(self.turrets[i].pos.x, 1.0, self.turrets[i].pos.y);
                    self.world_popups.push(WorldPopup {
                        pos: kp,
                        text: format!("+{}", kill),
                        t: 1.0,
                    });
                    self.burst(kp, Color::new(1.0, 0.3, 0.8, 1.0), 34, 7.0);
                    self.explosions.push(Explosion { pos: kp, t: 0.0, big: true });
                    self.pending_hitstop = 0.09;
                    self.shake = (self.shake + 0.4).min(1.0);
                    play(snd, |s| &s.kill, 0.8);
                } else {
                    let p = self.turrets[i].pos;
                    self.burst(vec3(p.x, 1.05, p.y), Color::new(1.0, 0.5, 0.9, 1.0), 6, 3.5);
                }
            }
            None => {
                self.burst(hit_p, Color::new(0.55, 0.35, 1.0, 1.0), 4, 2.0);
            }
        }
    }

    fn hurt(&mut self, dmg: f32, from_dir: Vec2, snd: &Option<Sounds>) {
        self.hp -= dmg;
        self.invuln = 1.2;
        self.dmg_flash = 1.0;
        self.shake = (self.shake + 0.6).min(1.2);
        self.vel += -from_dir * 9.0;
        self.last_hit_dir = Some((from_dir.y.atan2(from_dir.x), 1.2));
        let p = self.ppos + from_dir * 0.5;
        self.burst(vec3(p.x, 0.8, p.y), Color::new(1.0, 0.2, 0.2, 1.0), 14, 4.5);
        play(snd, |s| &s.hurt, 0.7);
    }

    /// Per-frame simulation.
    fn update(&mut self, dt: f32, active: bool, input: bool, snd: &Option<Sounds>) {
        let t = get_time() as f32;
        if active {
            self.time_in_level += dt;
        }
        self.intro_t -= dt;

        // ----- player input
        let mut wish = Vec2::ZERO;
        let mut sprinting = false;
        if input && dt > 0.0 {
            if is_key_down(KeyCode::Left) {
                self.yaw -= 2.4 * dt;
            }
            if is_key_down(KeyCode::Right) {
                self.yaw += 2.4 * dt;
            }
            if is_key_down(KeyCode::Up) {
                self.pitch = (self.pitch + 1.8 * dt).clamp(-1.45, 1.45);
            }
            if is_key_down(KeyCode::Down) {
                self.pitch = (self.pitch - 1.8 * dt).clamp(-1.45, 1.45);
            }

            let fwd = vec2(self.yaw.cos(), self.yaw.sin());
            let right = vec2(-self.yaw.sin(), self.yaw.cos());
            if is_key_down(KeyCode::W) {
                wish += fwd;
            }
            if is_key_down(KeyCode::S) {
                wish -= fwd;
            }
            if is_key_down(KeyCode::D) {
                wish += right;
            }
            if is_key_down(KeyCode::A) {
                wish -= right;
            }
            wish = wish.normalize_or_zero();
            sprinting = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);

            if is_key_pressed(KeyCode::Space) && self.dash_cd <= 0.0 {
                self.dash_dir = if wish.length_squared() > 0.0 { wish } else { fwd };
                self.dash_t = DASH_TIME;
                self.dash_cd = DASH_CD;
                self.invuln = self.invuln.max(0.30);
                self.vel = self.dash_dir * DASH_SPEED;
                play(snd, |s| &s.dash, 0.5);
            }

            let target_roll = -wish.dot(right) * 0.028;
            self.roll += (target_roll - self.roll) * (8.0 * dt).min(1.0);

            if active && self.shot_cd <= 0.0 && is_mouse_button_down(MouseButton::Left) {
                self.shoot(snd);
            }
        } else {
            self.roll += (0.0 - self.roll) * (8.0 * dt).min(1.0);
        }

        // ----- movement: exponential approach to wish velocity
        let speed_mult = if self.overdrive_t > 0.0 { 1.18 } else { 1.0 };
        let target_speed = if sprinting { SPRINT_SPEED } else { WALK_SPEED } * speed_mult;
        let approach = if self.dash_t > 0.0 { 2.0 } else { 11.0 };
        let k = 1.0 - (-approach * dt).exp();
        self.vel += (wish * target_speed - self.vel) * k;

        let oldp = self.ppos;
        self.ppos = self.maze.resolve(self.ppos + self.vel * dt, PLAYER_R);
        if dt > 0.0 {
            let eff = (self.ppos - oldp) / dt;
            if eff.length() < self.vel.length() {
                self.vel = eff;
            }
        }

        let speed_now = self.vel.length();
        self.move_frac = (speed_now / SPRINT_SPEED).min(1.3);
        if speed_now > 0.5 {
            self.bob_t += dt * (speed_now / WALK_SPEED).min(1.6);
            // Footsteps on bob cycle.
            let phase = (self.bob_t * 9.5 / std::f32::consts::PI).fract();
            if phase < self.prev_step_phase && active {
                play(snd, |s| &s.step, 0.16 * self.move_frac);
            }
            self.prev_step_phase = phase;
        }

        // Dash trail.
        if self.dash_t > 0.0 {
            self.particles.push(Particle {
                pos: vec3(self.ppos.x, 0.3 + gen_range(0.0, 0.5), self.ppos.y),
                vel: vec3(0.0, gen_range(0.2, 0.8), 0.0),
                life: 0.35,
                max: 0.35,
                size: 0.10,
                color: COL_CRYSTAL,
                grav: 0.0,
            });
        }

        // FOV target.
        let mut fov_target = BASE_FOV;
        if speed_now > 5.4 {
            fov_target += 5.0;
        }
        if self.dash_t > 0.0 {
            fov_target += 11.0;
        }
        if self.overdrive_t > 0.0 {
            fov_target += 2.0;
        }
        self.fov += (fov_target - self.fov) * (9.0 * dt).min(1.0);

        // Timers.
        self.dash_t -= dt;
        self.dash_cd -= dt;
        self.shot_cd -= dt;
        self.invuln -= dt;
        self.overdrive_t -= dt;
        self.recoil *= 0.0002_f32.powf(dt);
        self.muzzle_flash = (self.muzzle_flash - dt * 14.0).max(0.0);
        self.dmg_flash = (self.dmg_flash - dt * 1.6).max(0.0);
        self.pick_flash = (self.pick_flash - dt * 2.5).max(0.0);
        self.hitmark_t -= dt;
        self.shake = (self.shake - dt * 2.6).max(0.0);
        if self.combo_t > 0.0 {
            self.combo_t -= dt;
            if self.combo_t <= 0.0 {
                self.combo = 1.0;
            }
        }
        if let Some((a, ttl)) = self.last_hit_dir {
            let ttl = ttl - dt;
            self.last_hit_dir = if ttl > 0.0 { Some((a, ttl)) } else { None };
        }

        // ----- crystals (with magnet)
        if active {
            let ppos = self.ppos;
            let mut collected: Vec<Vec3> = Vec::new();
            for c in self.crystals.iter_mut() {
                if c.taken {
                    continue;
                }
                let d = ppos - c.pos;
                let dist = d.length();
                if dist < 2.8 && dist > 0.01 {
                    c.pos += d / dist * (6.5 * (1.0 - dist / 2.8) + 1.5) * dt;
                }
                if dist < 0.9 {
                    c.taken = true;
                    collected.push(vec3(c.pos.x, 1.0, c.pos.y));
                }
            }
            for pos in collected {
                let pts = ((100 + 25 * (self.level as i64 - 1)) as f32 * self.combo) as i64;
                self.score += pts;
                self.stats.crystals += 1;
                self.hp = (self.hp + 4.0).min(100.0);
                self.pick_flash = 1.0;
                self.combo = (self.combo + 1.0).min(6.0);
                self.combo_t = 6.0;
                self.popup(format!("+{} CRYSTAL", pts));
                self.burst(pos, COL_CRYSTAL, 22, 5.0);
                play(snd, |s| &s.pickup, 0.65);
            }

            // Pickups.
            let mut got: Vec<(PickupKind, Vec3)> = Vec::new();
            for p in self.pickups.iter_mut() {
                if p.taken {
                    continue;
                }
                let d = ppos - p.pos;
                let dist = d.length();
                if dist < 2.4 && dist > 0.01 {
                    p.pos += d / dist * (5.0 * (1.0 - dist / 2.4) + 1.0) * dt;
                }
                if dist < 0.9 {
                    p.taken = true;
                    got.push((p.kind, vec3(p.pos.x, 0.8, p.pos.y)));
                }
            }
            for (kind, pos) in got {
                match kind {
                    PickupKind::Health => {
                        self.hp = (self.hp + 30.0).min(100.0);
                        self.popup("+30 HP".to_string());
                        self.burst(pos, GREEN, 18, 4.5);
                        play(snd, |s| &s.health, 0.7);
                    }
                    PickupKind::Overdrive => {
                        self.overdrive_t = 8.0;
                        self.popup("OVERDRIVE".to_string());
                        self.burst(pos, COL_OVERDRIVE, 24, 5.5);
                        play(snd, |s| &s.pickup, 0.8);
                    }
                }
            }
        }

        // Ambient sparkles above crystals.
        let mut sparkles: Vec<Particle> = Vec::new();
        for c in self.crystals.iter().filter(|c| !c.taken) {
            if (c.pos - self.ppos).length() < 18.0 && gen_range(0.0, 1.0) < dt * 2.5 {
                sparkles.push(Particle {
                    pos: vec3(
                        c.pos.x + gen_range(-0.3, 0.3),
                        gen_range(0.3, 0.8),
                        c.pos.y + gen_range(-0.3, 0.3),
                    ),
                    vel: vec3(0.0, gen_range(0.5, 1.1), 0.0),
                    life: 0.9,
                    max: 0.9,
                    size: 0.045,
                    color: COL_CRYSTAL,
                    grav: 0.0,
                });
            }
        }
        self.particles.append(&mut sparkles);

        // ----- drones
        let chase_speed = self.drone_chase_speed();
        let sight = self.drone_sight();
        let damage = self.drone_damage();
        let ppos = self.ppos;
        let maze = &self.maze;
        let mut hit_player: Option<Vec2> = None;

        for d in self.drones.iter_mut() {
            d.hit_flash = (d.hit_flash - dt * 6.0).max(0.0);
            let to_player = ppos - d.pos;
            let dist = to_player.length();
            let sees = active && dist < sight && maze.los(d.pos, ppos);
            let my_cell = maze.world_to_cell(d.pos);

            match d.state {
                DroneState::Patrol => {
                    if sees {
                        d.state = DroneState::Chase;
                        d.last_seen = ppos;
                        d.lost_t = 0.0;
                        d.repath_t = 0.0;
                    } else if d.path_i >= d.path.len() {
                        // Pick a new patrol destination 4-12 cells away.
                        let opens = maze.open_cells();
                        for _ in 0..8 {
                            let c = opens[gen_range(0, opens.len())];
                            let dd = (c.0 - my_cell.0).abs() + (c.1 - my_cell.1).abs();
                            if dd >= 4 && dd <= 14 {
                                d.path = maze.bfs(my_cell, c);
                                d.path_i = 0;
                                break;
                            }
                        }
                    }
                }
                DroneState::Chase => {
                    if sees {
                        d.last_seen = ppos;
                        d.lost_t = 0.0;
                    } else {
                        d.lost_t += dt;
                    }
                    if !active || d.lost_t > 3.5 {
                        d.state = DroneState::Investigate;
                        d.investigate_t = 1.2;
                        d.path.clear();
                        d.path_i = 0;
                    }
                }
                DroneState::Investigate => {
                    if sees {
                        d.state = DroneState::Chase;
                        d.last_seen = ppos;
                        d.lost_t = 0.0;
                        d.repath_t = 0.0;
                    } else {
                        d.investigate_t -= dt;
                        if d.investigate_t <= 0.0 {
                            d.state = DroneState::Patrol;
                            d.path.clear();
                            d.path_i = 0;
                        }
                    }
                }
            }

            // Movement.
            let (speed, goal) = match d.state {
                DroneState::Patrol => {
                    let goal = if d.path_i < d.path.len() {
                        let c = d.path[d.path_i];
                        maze.cell_center(c.0, c.1)
                    } else {
                        d.pos
                    };
                    (1.8, goal)
                }
                DroneState::Chase => {
                    if sees {
                        (chase_speed, ppos)
                    } else {
                        // Path toward last seen position.
                        d.repath_t -= dt;
                        if d.repath_t <= 0.0 {
                            d.repath_t = 0.5;
                            let target_cell = maze.world_to_cell(d.last_seen);
                            d.path = maze.bfs(my_cell, target_cell);
                            d.path_i = 0;
                        }
                        let goal = if d.path_i < d.path.len() {
                            let c = d.path[d.path_i];
                            maze.cell_center(c.0, c.1)
                        } else {
                            d.last_seen
                        };
                        (chase_speed * 0.85, goal)
                    }
                }
                DroneState::Investigate => {
                    // Spin in place looking around.
                    let a = t * 3.0 + d.phase;
                    d.dir = vec2(a.cos(), a.sin());
                    (0.0, d.pos)
                }
            };

            let dirv = goal - d.pos;
            let step = if dirv.length() > 0.05 && speed > 0.0 {
                dirv.normalize() * speed * dt
            } else {
                Vec2::ZERO
            };
            let newp = maze.resolve(d.pos + step, DRONE_R);
            let moved = (newp - d.pos).length();
            if step.length() > 0.0001 && moved < step.length() * 0.3 {
                d.stuck_t += dt;
            } else {
                d.stuck_t = 0.0;
            }
            if moved > 0.0001 && d.state != DroneState::Investigate {
                let nd = (newp - d.pos).normalize();
                d.dir = (d.dir + (nd - d.dir) * (7.0 * dt).min(1.0)).normalize_or_zero();
            }
            d.pos = newp;

            // Advance waypoints.
            if d.path_i < d.path.len() {
                let c = d.path[d.path_i];
                if (d.pos - maze.cell_center(c.0, c.1)).length() < 0.35 {
                    d.path_i += 1;
                }
            }
            if d.stuck_t > 0.8 {
                d.stuck_t = 0.0;
                d.path.clear();
                d.path_i = 0;
                d.repath_t = 0.0;
            }

            if active && self.invuln <= 0.0 && dist < PLAYER_R + DRONE_R + 0.05 {
                hit_player = Some(to_player.normalize_or_zero());
            }
        }

        // Drone separation.
        for i in 0..self.drones.len() {
            for j in (i + 1)..self.drones.len() {
                let d = self.drones[j].pos - self.drones[i].pos;
                let l = d.length();
                if l < DRONE_R * 2.0 && l > 1e-4 {
                    let push = d / l * (DRONE_R * 2.0 - l) * 0.5;
                    self.drones[i].pos -= push;
                    self.drones[j].pos += push;
                }
            }
        }

        // ----- turrets
        let fire_interval = (2.4 - 0.1 * self.level as f32).max(1.4);
        let mut shots: Vec<(Vec2, Vec2)> = Vec::new();
        for tr in self.turrets.iter_mut() {
            tr.hit_flash = (tr.hit_flash - dt * 6.0).max(0.0);
            if !tr.alive {
                continue;
            }
            let to_p = ppos - tr.pos;
            let dist = to_p.length();
            if active && dist < 15.0 && dist > 0.5 && maze.los(tr.pos, ppos) {
                let want = to_p / dist;
                tr.aim = (tr.aim + (want - tr.aim) * (3.0 * dt).min(1.0)).normalize_or_zero();
                tr.fire_cd -= dt;
                if tr.fire_cd <= 0.0 && tr.aim.dot(want) > 0.92 {
                    tr.fire_cd = fire_interval;
                    // Slight lead on the player.
                    let lead = (ppos + self.vel * (dist / 7.5) * 0.35) - tr.pos;
                    shots.push((tr.pos, lead.normalize_or_zero()));
                }
            } else {
                tr.fire_cd = tr.fire_cd.max(0.6);
            }
        }
        for (pos, dir) in shots {
            self.projectiles.push(Projectile {
                pos: pos + dir * 0.5,
                vel: dir * 7.5,
                ttl: 4.0,
            });
            let vol = (1.0 - (pos - ppos).length() / 20.0).clamp(0.1, 1.0) * 0.55;
            play(snd, |s| &s.turret, vol);
        }

        // ----- projectiles
        let mut proj_hit: Option<Vec2> = None;
        let mut proj_particles: Vec<Vec3> = Vec::new();
        self.projectiles.retain_mut(|p| {
            p.pos += p.vel * dt;
            p.ttl -= dt;
            let (cx, cy) = maze.world_to_cell(p.pos);
            if maze.is_wall(cx, cy) || p.ttl <= 0.0 {
                proj_particles.push(vec3(p.pos.x, 1.0, p.pos.y));
                return false;
            }
            if active && (p.pos - ppos).length() < 0.16 + PLAYER_R {
                if self.invuln <= 0.0 {
                    proj_hit = Some(p.vel.normalize_or_zero() * -1.0);
                }
                proj_particles.push(vec3(p.pos.x, 1.0, p.pos.y));
                return false;
            }
            true
        });
        for pp in proj_particles {
            self.burst(pp, COL_OVERDRIVE, 7, 3.0);
        }
        if let Some(dir) = hit_player {
            self.hurt(damage, dir, snd);
        }
        if let Some(dir) = proj_hit {
            self.hurt(12.0, dir, snd);
        }

        // ----- drone respawns
        let mut respawn_now = 0;
        self.respawns.retain_mut(|r| {
            *r -= dt;
            if *r <= 0.0 {
                respawn_now += 1;
                false
            } else {
                true
            }
        });
        for _ in 0..respawn_now {
            let mut cells: Vec<(i32, i32)> = self
                .maze
                .open_cells()
                .into_iter()
                .filter(|&(x, y)| (self.maze.cell_center(x, y) - self.ppos).length() > 12.0)
                .collect();
            if cells.is_empty() {
                cells = self.maze.open_cells();
            }
            let (x, y) = cells[gen_range(0, cells.len())];
            self.drones.push(Drone::new(self.maze.cell_center(x, y)));
        }

        // ----- particles / popups / tracers / explosions
        self.particles.retain_mut(|p| {
            p.vel.y += p.grav * dt;
            p.pos += p.vel * dt;
            p.life -= dt;
            p.life > 0.0
        });
        self.popups.retain_mut(|p| {
            p.t -= dt;
            p.t > 0.0
        });
        self.world_popups.retain_mut(|p| {
            p.t -= dt;
            p.pos.y += dt * 0.8;
            p.t > 0.0
        });
        self.tracers.retain_mut(|tr| {
            tr.ttl -= dt;
            tr.ttl > 0.0
        });
        self.explosions.retain_mut(|e| {
            e.t += dt;
            e.t < 0.4
        });

        let _ = t;
    }

    // -------------------------------------------------------------- lights

    fn collect_lights(&self, eye: Vec3) -> ([Vec4; 12], [Vec4; 12]) {
        let t = get_time() as f32;
        let mut lights: Vec<LightSrc> = Vec::new();

        // Player headlight.
        lights.push(LightSrc {
            pos: vec3(self.ppos.x, 1.4, self.ppos.y),
            color: vec3(1.0, 0.95, 0.88),
            radius: 10.0,
            intensity: 0.85,
        });
        if self.muzzle_flash > 0.0 {
            lights.push(LightSrc {
                pos: self.muzzle_world(),
                color: vec3(0.4, 1.0, 1.0),
                radius: 7.0,
                intensity: 2.2 * self.muzzle_flash,
            });
        }
        for c in self.crystals.iter().filter(|c| !c.taken) {
            let pulse = 0.85 + 0.15 * (t * 2.5 + c.phase).sin();
            lights.push(LightSrc {
                pos: vec3(c.pos.x, 1.1, c.pos.y),
                color: vec3(0.15, 0.85, 1.0),
                radius: 5.0,
                intensity: 1.0 * pulse,
            });
        }
        for d in &self.drones {
            let (col, int) = if d.state == DroneState::Chase {
                (vec3(1.0, 0.18, 0.12), 1.1)
            } else {
                (vec3(1.0, 0.5, 0.12), 0.7)
            };
            lights.push(LightSrc {
                pos: vec3(d.pos.x, 1.0, d.pos.y),
                color: col,
                radius: 4.5,
                intensity: int,
            });
        }
        for tr in self.turrets.iter().filter(|t| t.alive) {
            let charging = (1.0 - tr.fire_cd / 1.0).clamp(0.0, 1.0);
            lights.push(LightSrc {
                pos: vec3(tr.pos.x, 1.2, tr.pos.y),
                color: vec3(1.0, 0.25, 0.85),
                radius: 4.0,
                intensity: 0.5 + 0.5 * charging,
            });
        }
        for p in &self.projectiles {
            lights.push(LightSrc {
                pos: vec3(p.pos.x, 1.0, p.pos.y),
                color: vec3(1.0, 0.3, 0.9),
                radius: 4.5,
                intensity: 1.2,
            });
        }
        for p in self.pickups.iter().filter(|p| !p.taken) {
            let col = match p.kind {
                PickupKind::Health => vec3(0.2, 1.0, 0.4),
                PickupKind::Overdrive => vec3(1.0, 0.3, 0.9),
            };
            lights.push(LightSrc {
                pos: vec3(p.pos.x, 0.9, p.pos.y),
                color: col,
                radius: 3.5,
                intensity: 0.7,
            });
        }
        for e in &self.explosions {
            let k = 1.0 - e.t / 0.4;
            lights.push(LightSrc {
                pos: e.pos,
                color: vec3(1.0, 0.5, 0.2),
                radius: if e.big { 9.0 } else { 6.5 },
                intensity: 2.0 * k,
            });
        }

        lights.sort_by(|a, b| {
            let da = (a.pos - eye).length() / a.intensity.max(0.2);
            let db = (b.pos - eye).length() / b.intensity.max(0.2);
            da.partial_cmp(&db).unwrap()
        });

        let mut pos = [Vec4::ZERO; 12];
        let mut col = [Vec4::ZERO; 12];
        for (i, l) in lights.iter().take(12).enumerate() {
            pos[i] = vec4(l.pos.x, l.pos.y, l.pos.z, l.radius);
            col[i] = vec4(l.color.x, l.color.y, l.color.z, l.intensity);
        }
        (pos, col)
    }

    // ------------------------------------------------------------ rendering

    fn draw_world(&mut self, rend: &Renderer, eye: Vec3, target: Vec3, fog_max: f32, fp_view: bool) {
        let t = get_time() as f32;
        let look = (target - eye).normalize_or_zero();
        let up = Quat::from_axis_angle(look, self.roll).mul_vec3(Vec3::Y);

        let cam = Camera3D {
            position: eye,
            target,
            up,
            fovy: self.fov.to_radians(),
            ..Default::default()
        };
        self.cam_matrix = cam.matrix();
        set_camera(&cam);

        let eye2 = vec2(eye.x, eye.z);
        let fog_density = 3.0 / (fog_max * fog_max);

        // ---- lit pass
        if let Some(mat) = &rend.world_mat {
            let (lp, lc) = self.collect_lights(eye);
            mat.set_uniform_array("LightPos", &lp[..]);
            mat.set_uniform_array("LightCol", &lc[..]);
            mat.set_uniform("CamPos", eye);
            mat.set_uniform("FogInfo", vec4(COL_FOG.r, COL_FOG.g, COL_FOG.b, fog_density));
            mat.set_uniform("GameTime", t);
            gl_use_material(mat);
        }

        draw_mesh(&self.floor_mesh);
        for (center, radius, mesh) in &self.wall_chunks {
            if (*center - eye2).length() - radius < fog_max + 2.0 {
                draw_mesh(mesh);
            }
        }

        // Drone bodies (lit).
        let mut lit = MeshBuilder::new();
        for d in &self.drones {
            let dist = (d.pos - eye2).length();
            if dist > fog_max + 4.0 {
                continue;
            }
            let dy = 0.9 + (t * 3.0 + d.phase).sin() * 0.1;
            let center = vec3(d.pos.x, dy, d.pos.y);
            let base = Color::new(0.16, 0.16, 0.22, 1.0);
            let body = clerp(base, WHITE, d.hit_flash);
            lit.sphere(center, DRONE_R - 0.03, 7, 9, body);
        }
        // Turret bodies (lit).
        for tr in &self.turrets {
            let dist = (tr.pos - eye2).length();
            if dist > fog_max + 4.0 {
                continue;
            }
            let base = vec3(tr.pos.x, 0.0, tr.pos.y);
            let bodyc = if tr.alive {
                clerp(Color::new(0.20, 0.16, 0.28, 1.0), WHITE, tr.hit_flash)
            } else {
                Color::new(0.08, 0.07, 0.10, 1.0)
            };
            lit.box_center(base + vec3(0.0, 0.42, 0.0), vec3(0.30, 0.0, 0.0), vec3(0.0, 0.84, 0.0), vec3(0.0, 0.0, 0.30), bodyc);
            if tr.alive {
                lit.sphere(base + vec3(0.0, 1.05, 0.0), 0.30, 6, 8, bodyc);
                let aim3 = vec3(tr.aim.x, 0.0, tr.aim.y);
                let side = vec3(-tr.aim.y, 0.0, tr.aim.x);
                lit.box_center(
                    base + vec3(0.0, 1.05, 0.0) + aim3 * 0.30,
                    side * 0.10,
                    vec3(0.0, 0.10, 0.0),
                    aim3 * 0.40,
                    Color::new(0.13, 0.12, 0.18, 1.0),
                );
            }
        }
        // Viewmodel (lit, first-person only).
        let mut vm_basis = None;
        if fp_view {
            let f = look;
            let r = f.cross(Vec3::Y).normalize_or_zero();
            let u = r.cross(f).normalize_or_zero();
            let bob = (self.bob_t * 9.5).sin() * 0.010 * self.move_frac;
            let bob2 = (self.bob_t * 19.0).sin() * 0.005 * self.move_frac;
            let anchor = eye + f * (0.36 - self.recoil * 1.6)
                + r * (0.165 + bob2)
                + u * (-0.135 + bob);
            let gun = Color::new(0.17, 0.18, 0.24, 1.0);
            let dark = Color::new(0.10, 0.10, 0.15, 1.0);
            lit.box_center(anchor, r * 0.085, u * 0.075, f * 0.20, gun);
            lit.box_center(anchor + f * 0.16 + u * 0.008, r * 0.048, u * 0.048, f * 0.16, dark);
            lit.box_center(anchor - f * 0.04 + u * -0.065, r * 0.065, u * 0.09, f * 0.06, dark);
            vm_basis = Some((anchor, r, u, f));
        }
        if !lit.i.is_empty() {
            draw_mesh(&lit.build());
        }

        gl_use_default_material();

        // Emissive gun strip + charge light.
        let mut muzzle_vm = None;
        if let Some((anchor, r, u, f)) = vm_basis {
            let ready = self.shot_cd <= 0.0;
            let strip = if self.overdrive_t > 0.0 {
                with_alpha(COL_OVERDRIVE, 0.95)
            } else if ready {
                Color::new(0.25, 0.95, 1.0, 0.95)
            } else {
                Color::new(0.10, 0.35, 0.45, 0.95)
            };
            let mut em = MeshBuilder::new();
            em.box_center(anchor + u * 0.042, r * 0.012, u * 0.012, f * 0.19, strip);
            em.box_center(anchor + f * 0.245 + u * 0.008, r * 0.030, u * 0.030, f * 0.012, strip);
            draw_mesh(&em.build());
            muzzle_vm = Some(anchor + f * 0.26 + u * 0.008);
        }

        // ---- emissive pass
        // Crystals: spinning double octahedra.
        for c in self.crystals.iter().filter(|c| !c.taken) {
            let dist = (c.pos - eye2).length();
            if dist > fog_max + 6.0 {
                continue;
            }
            let cy = 1.0 + (t * 2.0 + c.phase).sin() * 0.15;
            let center = vec3(c.pos.x, cy, c.pos.y);
            let a = t * 1.6 + c.phase;
            self.draw_octahedron(center, 0.30, 0.46, a, COL_CRYSTAL);
            self.draw_octahedron(center, 0.45, 0.69, -a * 0.7, with_alpha(COL_CRYSTAL, 0.16));
            // Beacon column.
            draw_cube(
                vec3(c.pos.x, 2.8, c.pos.y),
                vec3(0.06, 5.6, 0.06),
                None,
                with_alpha(COL_CRYSTAL, 0.10),
            );
        }

        // Drone accents: ring + eye.
        for d in &self.drones {
            let dist = (d.pos - eye2).length();
            if dist > fog_max + 4.0 {
                continue;
            }
            let dy = 0.9 + (t * 3.0 + d.phase).sin() * 0.1;
            let center = vec3(d.pos.x, dy, d.pos.y);
            let chasing = d.state == DroneState::Chase;
            let accent = if chasing {
                Color::new(1.0, 0.15, 0.12, 1.0)
            } else {
                Color::new(1.0, 0.55, 0.12, 1.0)
            };
            let spin = t * if chasing { 7.0 } else { 2.2 } + d.phase;
            for k in 0..8 {
                let ang = spin + k as f32 * std::f32::consts::TAU / 8.0;
                let rp = center + vec3(ang.cos() * 0.58, 0.0, ang.sin() * 0.58);
                draw_cube(rp, Vec3::splat(0.07), None, accent);
            }
            let ed = vec3(d.dir.x, 0.0, d.dir.y).normalize_or_zero();
            let eye_col = if chasing {
                Color::new(1.0, 0.9, 0.5, 1.0)
            } else {
                Color::new(0.9, 0.6, 0.2, 1.0)
            };
            draw_sphere(center + ed * (DRONE_R - 0.06), 0.13, None, eye_col);
        }

        // Turret lenses.
        for tr in self.turrets.iter().filter(|t| t.alive) {
            let dist = (tr.pos - eye2).length();
            if dist > fog_max + 4.0 {
                continue;
            }
            let aim3 = vec3(tr.aim.x, 0.0, tr.aim.y);
            let charge = (1.0 - tr.fire_cd / 0.8).clamp(0.2, 1.0);
            draw_sphere(
                vec3(tr.pos.x, 1.05, tr.pos.y) + aim3 * 0.42,
                0.10,
                None,
                Color::new(1.0, 0.3 * charge + 0.2, 0.9, 1.0),
            );
        }

        // Projectile cores.
        for p in &self.projectiles {
            draw_sphere(vec3(p.pos.x, 1.0, p.pos.y), 0.13, None, Color::new(1.0, 0.6, 1.0, 1.0));
        }

        // Pickups.
        for p in self.pickups.iter().filter(|p| !p.taken) {
            let dist = (p.pos - eye2).length();
            if dist > fog_max + 4.0 {
                continue;
            }
            let py = 0.55 + (t * 2.2 + p.phase).sin() * 0.10;
            let center = vec3(p.pos.x, py, p.pos.y);
            match p.kind {
                PickupKind::Health => {
                    let g = Color::new(0.25, 1.0, 0.45, 1.0);
                    draw_cube(center, vec3(0.34, 0.115, 0.115), None, g);
                    draw_cube(center, vec3(0.115, 0.34, 0.115), None, g);
                }
                PickupKind::Overdrive => {
                    let a = t * 2.5 + p.phase;
                    self.draw_octahedron(center, 0.22, 0.34, a, COL_OVERDRIVE);
                }
            }
        }

        // Tracers.
        for tr in &self.tracers {
            let a = (tr.ttl / 0.06).clamp(0.0, 1.0);
            draw_line_3d(tr.from, tr.to, Color::new(0.6, 1.0, 1.0, a));
            draw_line_3d(
                tr.from + vec3(0.0, 0.01, 0.0),
                tr.to,
                Color::new(0.2, 0.6, 1.0, a * 0.5),
            );
        }

        // Particles.
        for p in &self.particles {
            let a = (p.life / p.max).clamp(0.0, 1.0);
            draw_cube(p.pos, Vec3::splat(p.size), None, with_alpha(p.color, a));
        }

        // ---- glow pass (additive billboards)
        if let Some(gm) = &rend.glow_mat {
            gl_use_material(gm);
            let cam_r = look.cross(up).normalize_or_zero();
            let cam_u = cam_r.cross(look).normalize_or_zero();
            let glow = |center: Vec3, size: f32, color: Color| {
                let o = center - cam_r * (size * 0.5) - cam_u * (size * 0.5);
                draw_affine_parallelogram(o, cam_r * size, cam_u * size, Some(&rend.glow_tex), color);
            };
            for c in self.crystals.iter().filter(|c| !c.taken) {
                if (c.pos - eye2).length() > fog_max + 8.0 {
                    continue;
                }
                let cy = 1.0 + (t * 2.0 + c.phase).sin() * 0.15;
                let pulse = 0.75 + 0.25 * (t * 2.5 + c.phase).sin();
                glow(vec3(c.pos.x, cy, c.pos.y), 1.7, with_alpha(COL_CRYSTAL, 0.35 * pulse));
            }
            for d in &self.drones {
                if (d.pos - eye2).length() > fog_max + 6.0 {
                    continue;
                }
                let dy = 0.9 + (t * 3.0 + d.phase).sin() * 0.1;
                let c = if d.state == DroneState::Chase {
                    Color::new(1.0, 0.15, 0.10, 0.40)
                } else {
                    Color::new(1.0, 0.5, 0.10, 0.28)
                };
                glow(vec3(d.pos.x, dy, d.pos.y), 1.6, c);
            }
            for tr in self.turrets.iter().filter(|t| t.alive) {
                if (tr.pos - eye2).length() > fog_max + 6.0 {
                    continue;
                }
                glow(vec3(tr.pos.x, 1.1, tr.pos.y), 1.2, Color::new(1.0, 0.25, 0.85, 0.30));
            }
            for p in &self.projectiles {
                glow(vec3(p.pos.x, 1.0, p.pos.y), 1.1, Color::new(1.0, 0.4, 0.95, 0.6));
            }
            for p in self.pickups.iter().filter(|p| !p.taken) {
                let py = 0.55 + (t * 2.2 + p.phase).sin() * 0.10;
                let c = match p.kind {
                    PickupKind::Health => Color::new(0.2, 1.0, 0.4, 0.30),
                    PickupKind::Overdrive => with_alpha(COL_OVERDRIVE, 0.35),
                };
                glow(vec3(p.pos.x, py, p.pos.y), 1.2, c);
            }
            for e in &self.explosions {
                let k = (e.t / 0.4).clamp(0.0, 1.0);
                let size = (if e.big { 4.5 } else { 3.0 }) * (0.3 + k * 0.7);
                glow(e.pos, size, Color::new(1.0, 0.45, 0.15, (1.0 - k) * 0.8));
            }
            if self.muzzle_flash > 0.0 {
                if let Some(m) = muzzle_vm {
                    glow(m, 0.45, Color::new(0.5, 1.0, 1.0, self.muzzle_flash * 0.9));
                }
            }
            gl_use_default_material();
        }
    }

    fn draw_octahedron(&self, center: Vec3, w: f32, h: f32, spin: f32, color: Color) {
        let top = center + vec3(0.0, h, 0.0);
        let bot = center - vec3(0.0, h, 0.0);
        let mut eq = [Vec3::ZERO; 4];
        for (i, e) in eq.iter_mut().enumerate() {
            let a = spin + i as f32 * std::f32::consts::FRAC_PI_2;
            *e = center + vec3(a.cos() * w, 0.0, a.sin() * w);
        }
        let mut mb = MeshBuilder::new();
        for i in 0..4 {
            let j = (i + 1) % 4;
            let shade = 0.78 + 0.22 * ((spin + i as f32 * 1.57).sin() * 0.5 + 0.5);
            let ct = cmul(color, shade);
            let cb = cmul(color, shade * 0.75);
            let a = mb.vert(top, Vec3::Y, ct);
            let b = mb.vert(eq[i], Vec3::Y, ct);
            let c2 = mb.vert(eq[j], Vec3::Y, ct);
            mb.i.extend_from_slice(&[a, b, c2]);
            let a = mb.vert(bot, Vec3::NEG_Y, cb);
            let b = mb.vert(eq[j], Vec3::NEG_Y, cb);
            let c2 = mb.vert(eq[i], Vec3::NEG_Y, cb);
            mb.i.extend_from_slice(&[a, b, c2]);
        }
        draw_mesh(&mb.build());
    }

    fn draw_sky(&self, rend: &Renderer, yaw: f32, pitch: f32) {
        let sw = screen_width();
        let sh = screen_height();
        let t = get_time() as f32;
        let top = Color::new(0.018, 0.000, 0.055, 1.0);
        let mid = Color::new(0.110, 0.030, 0.190, 1.0);
        let strips = 36;
        for i in 0..strips {
            let f0 = i as f32 / strips as f32;
            let c = if f0 < 0.55 {
                clerp(top, mid, f0 / 0.55)
            } else {
                clerp(mid, COL_FOG, (f0 - 0.55) / 0.45)
            };
            let y = f0 * sh;
            draw_rectangle(0.0, y, sw, sh / strips as f32 + 1.0, c);
        }
        // Star field with yaw/pitch parallax.
        let vfov = self.fov.to_radians();
        let hfov = vfov * (sw / sh);
        for &(az, el, size, ph) in &rend.stars {
            let dx = wrap_angle(az - yaw);
            if dx.abs() > hfov * 0.65 {
                continue;
            }
            let sy = sh * 0.5 - (el - pitch) / vfov * sh;
            if sy < -10.0 || sy > sh * 0.75 {
                continue;
            }
            let sx = sw * 0.5 + dx / hfov * sw;
            let tw = 0.45 + 0.55 * (t * 1.5 + ph).sin().abs();
            draw_circle(sx, sy, size * 0.9, Color::new(0.75, 0.85, 1.0, 0.5 * tw));
        }
    }

    fn world_to_screen(&self, p: Vec3) -> Option<Vec2> {
        let clip = self.cam_matrix * vec4(p.x, p.y, p.z, 1.0);
        if clip.w <= 0.01 {
            return None;
        }
        Some(vec2(
            (clip.x / clip.w * 0.5 + 0.5) * screen_width(),
            (0.5 - clip.y / clip.w * 0.5) * screen_height(),
        ))
    }

    fn draw_hud(&self, rend: &Renderer) {
        let sw = screen_width();
        let sh = screen_height();
        let t = get_time() as f32;
        let (cx, cy) = (sw / 2.0, sh / 2.0);

        // Vignette.
        draw_texture_ex(
            &rend.vignette_tex,
            0.0,
            0.0,
            WHITE,
            DrawTextureParams { dest_size: Some(vec2(sw, sh)), ..Default::default() },
        );

        // World popups (kill scores).
        for wp in &self.world_popups {
            if let Some(s) = self.world_to_screen(wp.pos) {
                let a = wp.t.clamp(0.0, 1.0);
                draw_text(&wp.text, s.x, s.y, 26.0, with_alpha(Color::new(1.0, 0.8, 0.3, 1.0), a));
            }
        }

        // Crosshair with recoil spread.
        let sp = 4.0 + self.recoil * 280.0;
        let ch = with_alpha(WHITE, 0.85);
        draw_line(cx - sp - 8.0, cy, cx - sp, cy, 2.0, ch);
        draw_line(cx + sp, cy, cx + sp + 8.0, cy, 2.0, ch);
        draw_line(cx, cy - sp - 8.0, cx, cy - sp, 2.0, ch);
        draw_line(cx, cy + sp, cx, cy + sp + 8.0, 2.0, ch);
        if self.hitmark_t > 0.0 {
            let hc = Color::new(1.0, 0.9, 0.4, 1.0);
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0_f32)] {
                draw_line(cx + dx * 7.0, cy + dy * 7.0, cx + dx * 14.0, cy + dy * 14.0, 2.5, hc);
            }
        }

        // Compass chevron to nearest crystal.
        if let Some(nc) = self
            .crystals
            .iter()
            .filter(|c| !c.taken)
            .min_by(|a, b| {
                let da = (a.pos - self.ppos).length();
                let db = (b.pos - self.ppos).length();
                da.partial_cmp(&db).unwrap()
            })
        {
            let d = nc.pos - self.ppos;
            let ang = wrap_angle(d.y.atan2(d.x) - self.yaw);
            if ang.abs() > 0.35 {
                let sa = ang - std::f32::consts::FRAC_PI_2;
                let r = 54.0;
                let px = cx + sa.cos() * r;
                let py = cy + sa.sin() * r;
                let pulse = 0.45 + 0.25 * (t * 4.0).sin();
                let tip = vec2(px + sa.cos() * 9.0, py + sa.sin() * 9.0);
                let l = vec2(px + (sa + 2.4).cos() * 7.0, py + (sa + 2.4).sin() * 7.0);
                let rr = vec2(px + (sa - 2.4).cos() * 7.0, py + (sa - 2.4).sin() * 7.0);
                draw_triangle(tip, l, rr, with_alpha(COL_CRYSTAL, pulse));
            }
        }

        // Damage direction indicator.
        if let Some((world_ang, ttl)) = self.last_hit_dir {
            let rel = wrap_angle(world_ang - self.yaw) - std::f32::consts::FRAC_PI_2;
            let r = 78.0;
            let px = cx + rel.cos() * r;
            let py = cy + rel.sin() * r;
            let a = (ttl / 1.2).clamp(0.0, 1.0) * 0.9;
            let tip = vec2(px + rel.cos() * 16.0, py + rel.sin() * 16.0);
            let l = vec2(px + (rel + 2.2).cos() * 10.0, py + (rel + 2.2).sin() * 10.0);
            let rr = vec2(px + (rel - 2.2).cos() * 10.0, py + (rel - 2.2).sin() * 10.0);
            draw_triangle(tip, l, rr, Color::new(1.0, 0.15, 0.15, a));
        }

        // Score block.
        let remaining = self.crystals.iter().filter(|c| !c.taken).count();
        draw_text(&format!("SCORE  {}", self.score), 22.0, 40.0, 30.0, WHITE);
        draw_text(&format!("LEVEL  {}", self.level), 22.0, 72.0, 30.0, with_alpha(WHITE, 0.85));
        let ccol = if remaining == 0 { GREEN } else { COL_UI };
        draw_text(
            &format!("CRYSTALS  {}/{}", self.total_crystals - remaining, self.total_crystals),
            22.0,
            104.0,
            30.0,
            ccol,
        );

        // Combo.
        if self.combo > 1.0 && self.combo_t > 0.0 {
            let k = (self.combo_t / 6.0).clamp(0.0, 1.0);
            let txt = format!("COMBO  x{}", self.combo as i32);
            let d = measure_text(&txt, None, 34, 1.0);
            draw_text(
                &txt,
                cx - d.width / 2.0,
                cy - 64.0,
                34.0,
                with_alpha(Color::new(1.0, 0.85, 0.25, 1.0), 0.5 + 0.5 * k),
            );
            draw_rectangle(cx - 50.0, cy - 56.0, 100.0 * k, 4.0, Color::new(1.0, 0.85, 0.25, 0.7));
        }

        // Health bar.
        let (bx, by, bw, bh) = (22.0, sh - 64.0, 240.0, 18.0);
        draw_rectangle(bx - 2.0, by - 2.0, bw + 4.0, bh + 4.0, with_alpha(BLACK, 0.55));
        let frac = (self.hp / 100.0).clamp(0.0, 1.0);
        let hcol = clerp(Color::new(0.9, 0.15, 0.15, 1.0), Color::new(0.2, 0.95, 0.45, 1.0), frac);
        draw_rectangle(bx, by, bw * frac, bh, hcol);
        for i in 1..4 {
            draw_line(bx + bw * 0.25 * i as f32, by, bx + bw * 0.25 * i as f32, by + bh, 1.0, with_alpha(BLACK, 0.4));
        }
        draw_rectangle_lines(bx - 2.0, by - 2.0, bw + 4.0, bh + 4.0, 2.0, with_alpha(WHITE, 0.4));
        draw_text("HP", bx, by - 8.0, 22.0, with_alpha(WHITE, 0.8));

        // Dash bar.
        let dfrac = (1.0 - self.dash_cd / DASH_CD).clamp(0.0, 1.0);
        let dcol = if dfrac >= 1.0 { COL_UI } else { with_alpha(COL_UI, 0.35) };
        draw_rectangle(bx, by + bh + 10.0, bw * dfrac, 8.0, dcol);
        draw_rectangle_lines(bx - 2.0, by + bh + 8.0, bw + 4.0, 12.0, 2.0, with_alpha(WHITE, 0.3));
        draw_text("DASH [SPACE]", bx, by + bh + 38.0, 20.0, with_alpha(WHITE, 0.55));

        // Overdrive bar.
        if self.overdrive_t > 0.0 {
            let ofrac = (self.overdrive_t / 8.0).clamp(0.0, 1.0);
            draw_rectangle(bx, by - 26.0, bw * ofrac, 8.0, COL_OVERDRIVE);
            draw_text("OVERDRIVE", bx + bw * 0.5 - 44.0, by - 32.0, 18.0, with_alpha(COL_OVERDRIVE, 0.9));
        }

        // Minimap.
        let mm = 170.0;
        let pad = 14.0;
        let left = sw - mm - pad;
        let topy = pad;
        let scale = mm / (self.maze.n as f32 * CELL);
        draw_rectangle(left, topy, mm, mm, with_alpha(BLACK, 0.55));
        let n = self.maze.n as i32;
        let cellpx = mm / n as f32;
        for y in 0..n {
            for x in 0..n {
                if self.maze.is_wall(x, y) {
                    draw_rectangle(
                        left + x as f32 * cellpx,
                        topy + y as f32 * cellpx,
                        cellpx + 0.5,
                        cellpx + 0.5,
                        Color::new(0.42, 0.25, 0.80, 0.85),
                    );
                }
            }
        }
        let to_mm = |p: Vec2| {
            vec2(left + (p.x + self.maze.half()) * scale, topy + (p.y + self.maze.half()) * scale)
        };
        for c in self.crystals.iter().filter(|c| !c.taken) {
            let p = to_mm(c.pos);
            let pulse = 0.6 + 0.4 * (t * 4.0 + c.phase).sin();
            draw_circle(p.x, p.y, 2.6, with_alpha(COL_CRYSTAL, pulse));
        }
        for p in self.pickups.iter().filter(|p| !p.taken) {
            let mp = to_mm(p.pos);
            let col = match p.kind {
                PickupKind::Health => GREEN,
                PickupKind::Overdrive => COL_OVERDRIVE,
            };
            draw_circle(mp.x, mp.y, 2.2, col);
        }
        for tr in self.turrets.iter().filter(|t| t.alive) {
            let p = to_mm(tr.pos);
            draw_rectangle(p.x - 2.5, p.y - 2.5, 5.0, 5.0, Color::new(1.0, 0.3, 0.85, 1.0));
        }
        let mut any_chasing = false;
        for d in &self.drones {
            let p = to_mm(d.pos);
            let chasing = d.state == DroneState::Chase;
            any_chasing |= chasing;
            let col = if chasing { RED } else { ORANGE };
            draw_circle(p.x, p.y, 2.8, col);
        }
        let pp = to_mm(self.ppos);
        let fd = vec2(self.yaw.cos(), self.yaw.sin());
        draw_line(pp.x, pp.y, pp.x + fd.x * 9.0, pp.y + fd.y * 9.0, 2.0, with_alpha(WHITE, 0.8));
        draw_circle(pp.x, pp.y, 3.4, WHITE);
        let border = if any_chasing {
            with_alpha(RED, 0.5 + 0.4 * (t * 6.0).sin().abs())
        } else {
            with_alpha(COL_UI, 0.6)
        };
        draw_rectangle_lines(left, topy, mm, mm, 2.0, border);

        // Popups under crosshair.
        for (i, p) in self.popups.iter().enumerate() {
            let a = p.t.clamp(0.0, 1.0);
            let rise = (1.0 - a) * 26.0;
            let d = measure_text(&p.text, None, 26, 1.0);
            draw_text(
                &p.text,
                cx - d.width / 2.0,
                cy + 56.0 + i as f32 * 26.0 - rise,
                26.0,
                with_alpha(COL_UI, a),
            );
        }

        // Dash speedlines.
        if self.dash_t > 0.0 {
            let k = (self.dash_t / DASH_TIME).clamp(0.0, 1.0);
            for i in 0..14 {
                let a = hash01(i * 97 + 13) * std::f32::consts::TAU;
                let r0 = sw.max(sh) * 0.52;
                let r1 = r0 * (0.72 + hash01(i * 31) * 0.1);
                draw_line(
                    cx + a.cos() * r0,
                    cy + a.sin() * r0,
                    cx + a.cos() * r1,
                    cy + a.sin() * r1,
                    2.0,
                    Color::new(0.6, 0.95, 1.0, 0.30 * k),
                );
            }
        }

        // Screen flashes.
        if self.dmg_flash > 0.0 {
            draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.9, 0.05, 0.05, self.dmg_flash * 0.38));
        }
        if self.pick_flash > 0.0 {
            draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.1, 0.9, 1.0, self.pick_flash * 0.10));
        }
        if self.overdrive_t > 0.0 {
            draw_rectangle(0.0, 0.0, sw, sh, with_alpha(COL_OVERDRIVE, 0.035));
        }

        // Low-health vignette.
        if self.hp < 30.0 && self.hp > 0.0 {
            let a = 0.10 + 0.14 * (t * 5.0).sin().abs();
            let edge = Color::new(0.8, 0.0, 0.0, a);
            draw_rectangle(0.0, 0.0, sw, 26.0, edge);
            draw_rectangle(0.0, sh - 26.0, sw, 26.0, edge);
            draw_rectangle(0.0, 0.0, 26.0, sh, edge);
            draw_rectangle(sw - 26.0, 0.0, 26.0, sh, edge);
        }

        // Level intro.
        if self.intro_t > 0.0 {
            let a = (self.intro_t / 0.8).clamp(0.0, 1.0);
            center_text(
                &format!("LEVEL {}", self.level),
                sh * 0.24,
                52.0,
                with_alpha(COL_UI, a),
            );
            center_text(
                &format!("collect {} crystals", self.total_crystals),
                sh * 0.24 + 38.0,
                26.0,
                with_alpha(WHITE, a * 0.8),
            );
        }

        draw_text(
            &format!("{} FPS", get_fps()),
            sw - 86.0,
            sh - 16.0,
            20.0,
            with_alpha(WHITE, 0.35),
        );
    }
}

// --------------------------------------------------------------------- main

enum Mode {
    Menu,
    Playing,
    LevelDone(f32),
    Dead,
}

fn window_conf() -> Conf {
    Conf {
        window_title: "Crystal Rush".to_owned(),
        window_width: 1280,
        window_height: 720,
        sample_count: 2,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let shot_var = std::env::var("CR_SHOT").unwrap_or_default();
    let shot_mode = !shot_var.is_empty();
    if shot_mode {
        srand(12345);
    } else {
        srand(macroquad::miniquad::date::now() as u64);
    }

    let rend = Renderer::new();
    if rend.world_mat.is_none() {
        eprintln!("warning: lighting shader failed to compile, falling back to unlit rendering");
    }

    let no_audio = shot_mode || std::env::var("CR_NOAUDIO").is_ok();
    let sounds: Option<Sounds> = if no_audio { None } else { load_sounds().await };
    let audiotest = std::env::var("CR_AUDIOTEST").is_ok();
    if let Some(s) = &sounds {
        play_sound(&s.music, PlaySoundParams { looped: true, volume: 0.32 });
        if audiotest {
            play_sound(&s.shoot, PlaySoundParams { looped: false, volume: 0.5 });
        }
    }

    let mut game = Game::new(1, 0, 100.0, RunStats::default());
    let mut mode = if shot_mode && shot_var != "menu" { Mode::Playing } else { Mode::Menu };
    let mut paused = false;
    let mut grabbed = false;
    let mut last_mouse: Vec2 = mouse_position().into();
    let mut last_score: Option<(i64, u32)> = None;
    let mut sens: f32 = 1.0;
    let mut hitstop = 0.0_f32;
    let mut frame: u32 = 0;

    // Combat screenshot scenario: place a chasing drone ahead of the player.
    if shot_var == "combat" && !game.drones.is_empty() {
        let fwd = vec2(game.yaw.cos(), game.yaw.sin());
        game.drones[0].pos = game.maze.resolve(game.ppos + fwd * 7.0, DRONE_R);
        game.drones[0].state = DroneState::Chase;
        game.drones[0].last_seen = game.ppos;
        if game.drones.len() > 1 {
            game.drones[1].pos = game.maze.resolve(game.ppos + fwd * 5.0 + vec2(-fwd.y, fwd.x) * 1.2, DRONE_R);
        }
        game.intro_t = 0.0;
    }

    fn set_grab(g: bool, grabbed: &mut bool) {
        if g != *grabbed {
            set_cursor_grab(g);
            show_mouse(!g);
            *grabbed = g;
        }
    }

    loop {
        let real_dt = get_frame_time().min(0.05);
        let dt = if hitstop > 0.0 { real_dt * 0.12 } else { real_dt };
        hitstop = (hitstop - real_dt).max(0.0);
        let t = get_time() as f32;
        let mp: Vec2 = mouse_position().into();
        let mouse_delta = mp - last_mouse;
        last_mouse = mp;

        clear_background(COL_BG);

        match mode {
            Mode::Menu => {
                set_grab(false, &mut grabbed);
                game.update(dt, false, false, &sounds);

                let r = game.maze.half() * 1.25;
                let eye = vec3((t * 0.10).cos() * r, game.maze.half() * 0.9, (t * 0.10).sin() * r);
                let menu_yaw = (vec2(0.0, 0.0) - vec2(eye.x, eye.z)).to_angle();
                game.draw_sky(&rend, menu_yaw, -0.6);
                game.draw_world(&rend, eye, vec3(0.0, 0.0, 0.0), 300.0, false);
                set_default_camera();

                draw_rectangle(0.0, 0.0, screen_width(), screen_height(), with_alpha(BLACK, 0.35));
                let ch = screen_height() / 2.0;
                let pulse = 0.7 + 0.3 * (t * 2.4).sin();
                center_text("CRYSTAL RUSH", ch - 150.0, 96.0, with_alpha(COL_UI, 0.35 + 0.1 * (t * 1.7).sin()));
                center_text("CRYSTAL RUSH", ch - 152.0, 92.0, COL_UI);
                center_text("a neon maze raid", ch - 110.0, 28.0, with_alpha(WHITE, 0.7));
                center_text(
                    "collect every crystal ... the machines disagree",
                    ch - 52.0,
                    26.0,
                    with_alpha(WHITE, 0.85),
                );
                center_text(
                    "MOUSE look    WASD move    SHIFT sprint    SPACE dash    LMB shoot",
                    ch - 8.0,
                    24.0,
                    with_alpha(WHITE, 0.6),
                );
                center_text(
                    "ESC pause    [ ] sensitivity",
                    ch + 22.0,
                    24.0,
                    with_alpha(WHITE, 0.6),
                );
                if let Some((s, l)) = last_score {
                    center_text(
                        &format!("last run:  {} pts,  level {}", s, l),
                        ch + 64.0,
                        24.0,
                        with_alpha(COL_CRYSTAL, 0.8),
                    );
                }
                center_text(
                    "press  ENTER  or  CLICK  to start",
                    ch + 120.0,
                    32.0,
                    with_alpha(WHITE, pulse),
                );
                center_text("Q to quit", ch + 156.0, 20.0, with_alpha(WHITE, 0.4));

                if is_key_pressed(KeyCode::Enter) || is_mouse_button_pressed(MouseButton::Left) {
                    game = Game::new(1, 0, 100.0, RunStats::default());
                    mode = Mode::Playing;
                    paused = false;
                    set_grab(true, &mut grabbed);
                }
                if is_key_pressed(KeyCode::Q) || is_key_pressed(KeyCode::Escape) {
                    break;
                }
            }

            Mode::Playing | Mode::LevelDone(_) => {
                let in_transition = matches!(mode, Mode::LevelDone(_));

                if !paused {
                    if grabbed {
                        game.yaw += mouse_delta.x * 0.0022 * sens;
                        game.pitch = (game.pitch - mouse_delta.y * 0.0022 * sens).clamp(-1.45, 1.45);
                    }
                    if is_key_pressed(KeyCode::LeftBracket) {
                        sens = (sens - 0.1).max(0.4);
                        game.popup(format!("sensitivity {:.1}", sens));
                    }
                    if is_key_pressed(KeyCode::RightBracket) {
                        sens = (sens + 0.1).min(2.5);
                        game.popup(format!("sensitivity {:.1}", sens));
                    }
                    game.update(dt, !in_transition, true, &sounds);
                    if game.pending_hitstop > 0.0 {
                        hitstop = game.pending_hitstop;
                        game.pending_hitstop = 0.0;
                    }
                } else {
                    game.update(0.0, false, false, &sounds);
                }

                let eye = game.eye();
                let look = game.look_dir();
                game.draw_sky(&rend, game.yaw, game.pitch);
                game.draw_world(&rend, eye, eye + look, FOG_MAX, true);
                set_default_camera();
                game.draw_hud(&rend);

                if paused {
                    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), with_alpha(BLACK, 0.55));
                    let chh = screen_height() / 2.0;
                    center_text("PAUSED", chh - 60.0, 64.0, WHITE);
                    let remaining = game.crystals.iter().filter(|c| !c.taken).count();
                    center_text(
                        &format!("{} crystals left   |   {} drones up", remaining, game.drones.len()),
                        chh - 10.0,
                        24.0,
                        with_alpha(WHITE, 0.6),
                    );
                    center_text(
                        "ESC / CLICK  resume      Q  quit to menu",
                        chh + 36.0,
                        26.0,
                        with_alpha(WHITE, 0.7),
                    );

                    if is_key_pressed(KeyCode::Escape)
                        || is_key_pressed(KeyCode::P)
                        || is_mouse_button_pressed(MouseButton::Left)
                    {
                        paused = false;
                        set_grab(true, &mut grabbed);
                    }
                    if is_key_pressed(KeyCode::Q) {
                        last_score = Some((game.score, game.level));
                        paused = false;
                        mode = Mode::Menu;
                        set_grab(false, &mut grabbed);
                    }
                } else {
                    if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::P) {
                        paused = true;
                        set_grab(false, &mut grabbed);
                    }

                    if let Mode::LevelDone(ref mut timer) = mode {
                        *timer += real_dt;
                        let chh = screen_height() * 0.30;
                        center_text(&format!("LEVEL {} CLEARED", game.level), chh, 56.0, GREEN);
                        center_text(
                            &format!(
                                "clear bonus +{}      time bonus +{}",
                                game.last_bonus.0, game.last_bonus.1
                            ),
                            chh + 42.0,
                            28.0,
                            with_alpha(WHITE, 0.85),
                        );
                        if *timer > 2.6 {
                            let (level, score, hp, stats) =
                                (game.level + 1, game.score, (game.hp + 15.0).min(100.0), game.stats);
                            game = Game::new(level, score, hp, stats);
                            mode = Mode::Playing;
                        }
                    } else {
                        if game.crystals.iter().all(|c| c.taken) {
                            let clear = 200 + 100 * game.level as i64;
                            let time_b = ((90.0 - game.time_in_level).max(0.0) * 5.0) as i64;
                            game.score += clear + time_b;
                            game.last_bonus = (clear, time_b);
                            mode = Mode::LevelDone(0.0);
                            play(&sounds, |s| &s.clear, 0.8);
                        } else if game.hp <= 0.0 {
                            game.hp = 0.0;
                            game.dmg_flash = 1.4;
                            last_score = Some((game.score, game.level));
                            mode = Mode::Dead;
                            set_grab(false, &mut grabbed);
                            play(&sounds, |s| &s.death, 0.8);
                        }
                    }
                }
            }

            Mode::Dead => {
                game.update(dt, false, false, &sounds);
                let eye = game.eye();
                let look = game.look_dir();
                game.draw_sky(&rend, game.yaw, game.pitch);
                game.draw_world(&rend, eye, eye + look, FOG_MAX, false);
                set_default_camera();
                game.draw_hud(&rend);

                draw_rectangle(0.0, 0.0, screen_width(), screen_height(), with_alpha(BLACK, 0.55));
                let chh = screen_height() / 2.0;
                center_text("YOU DIED", chh - 100.0, 84.0, Color::new(1.0, 0.2, 0.2, 1.0));
                center_text(&format!("final score   {}", game.score), chh - 30.0, 34.0, WHITE);
                center_text(
                    &format!(
                        "level {}   |   {} crystals   |   {} drones   |   {} turrets",
                        game.level, game.stats.crystals, game.stats.kills, game.stats.turrets
                    ),
                    chh + 8.0,
                    26.0,
                    with_alpha(WHITE, 0.8),
                );
                center_text("R  retry        ENTER  menu", chh + 70.0, 28.0, with_alpha(WHITE, 0.7));

                if is_key_pressed(KeyCode::R) {
                    game = Game::new(1, 0, 100.0, RunStats::default());
                    mode = Mode::Playing;
                    paused = false;
                    set_grab(true, &mut grabbed);
                }
                if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Escape) {
                    mode = Mode::Menu;
                }
            }
        }

        frame += 1;
        if shot_var == "combat" && frame == 36 {
            game.shoot(&sounds);
        }
        if shot_mode && frame == 40 {
            get_screen_data().export_png("/tmp/crystal_rush.png");
            break;
        }
        if audiotest && frame == 90 {
            break;
        }

        next_frame().await;
    }
}
