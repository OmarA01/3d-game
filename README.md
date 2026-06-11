# Crystal Rush

A 3D first-person neon maze raid. Collect every crystal in the maze while
the machines hunt you down — drones patrol and chase with real pathfinding,
turrets snipe you with dodgeable plasma bolts. Outrun them, dash through
gaps, or shoot everything out of the air. Each level the maze grows and
the machines get faster.

## Play

```sh
./crystal-rush
```

The executable is fully self-contained (~1.4 MB) — no installation, no
asset files. It needs a Linux desktop with X11 (or XWayland), OpenGL and
ALSA/PipeWire for sound (all standard on any desktop install).

## Controls

| Input        | Action                            |
|--------------|-----------------------------------|
| Mouse        | Look                              |
| `W A S D`    | Move                              |
| `Shift`      | Sprint                            |
| `Space`      | Dash (brief invulnerability)      |
| Left click   | Shoot (hold for autofire)         |
| `[` / `]`    | Mouse sensitivity                 |
| Arrow keys   | Look (keyboard fallback)          |
| `Esc` / `P`  | Pause                             |
| `R`          | Retry (after death)               |

## Rules

- Collect **all crystals** to clear the level. Crystals are magnetic at
  close range, heal a little, and their beacons mark them over the walls.
  A cyan chevron around the crosshair points to the nearest one.
- **Drones** patrol the maze. Spotted? They turn red and chase — and they
  path-find, so corners won't save you for long. Gunfire attracts anything
  nearby. Three hits destroys one; they respawn elsewhere after a few
  seconds.
- **Turrets** (level 3+) lead their shots; sidestep or dash through the
  bolts. Four hits destroys one for good.
- Chain crystals and kills within 6 seconds to build a **combo
  multiplier** (up to x6).
- Pickups: **medkits** (+30 HP) and **overdrive** (8 s of double fire
  rate + extra speed).
- Score: crystals, kills, clear bonus, time bonus — all scaled by combo.
  Health carries between levels (+15 on clear).

## Tech notes

Built with [macroquad](https://github.com/not-fl3/macroquad); everything
is procedural in a single `src/main.rs` (~2900 lines):

- **Lighting** — custom GLSL material with 12 dynamic point lights
  (crystals, drones, projectiles, muzzle flash, explosions, headlamp),
  per-pixel exponential fog, emissive wall trim and animated floor grid.
- **Geometry** — maze walls are baked into chunked meshes with real
  normals and hidden-face culling; drones/turrets/viewmodel are generated
  meshes; additive billboard sprites provide the glow.
- **Audio** — every sound effect and the music loop are synthesized at
  startup (WAV in memory); no asset files.
- **Feel** — exponential-approach movement, momentum-preserving dash with
  i-frames, FOV kick, head bob, strafe roll, screen shake, hit-stop,
  hitmarkers, damage-direction indicator.

## Building from source

Requires a Rust toolchain (`rustup`). No system dev packages are needed:
graphics libraries are loaded at runtime, and ALSA is linked through the
symlink stub in `.linkstubs/` (see `.cargo/config.toml`). If you have
`libasound2-dev` installed the stub is harmless.

```sh
cargo build --release
# binary at target/release/crystal-rush
```

## Self-test

- `CR_SHOT=1 ./crystal-rush` — renders 40 frames of a deterministic run
  and saves `/tmp/crystal_rush.png` (`menu` and `combat` variants too).
- `CR_AUDIOTEST=1 ./crystal-rush` — exercises the audio path briefly.
- `CR_NOAUDIO=1 ./crystal-rush` — run silent.
