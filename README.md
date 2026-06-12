# Crystal Rush

A 3D first-person neon maze raid — solo or **online co-op for up to 4
players**. Collect every crystal in the maze while the machines hunt you
down — drones patrol and chase with real pathfinding, turrets snipe you
with dodgeable plasma bolts. Outrun them, dash through gaps, or shoot
everything out of the air. Each level the maze grows and the machines get
faster.

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
| `H` (menu)   | Host an online co-op game         |
| `J` (menu)   | Join a co-op game by IP           |

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

## Online co-op (up to 4 players)

One player hosts, the others join by IP:

```sh
./crystal-rush --host          # host on UDP port 24777 (or: press H in the menu)
./crystal-rush --host 5000     # host on a custom port
./crystal-rush --join 192.168.1.10        # join (or: press J in the menu)
./crystal-rush --join 192.168.1.10:5000
```

Crystals, score and combo are shared by the team; drones and turrets hunt
whoever they see first. If you go down you respawn after 5 seconds — keep
at least one player alive. Partners show up with name tags, health bars,
and colored dots on the minimap.

It works out of the box on a LAN or any VPN (Tailscale, ZeroTier, WireGuard).
To host across the open internet, forward UDP port 24777 to the hosting
machine — there is no matchmaking server; the game connects directly.

Netcode: host-authoritative UDP (~30 snapshots/s), client-side prediction
for your own movement, snapshot interpolation for everything else, and a
reliable-delivery channel for shots. Levels regenerate deterministically
from a shared seed, so only deltas travel over the wire.

## Tech notes

Built with [macroquad](https://github.com/not-fl3/macroquad); everything
is procedural in `src/main.rs` plus a small UDP protocol module
`src/net.rs` (~5000 lines total):

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
- **Netcode** — `std::net` UDP only (no extra dependencies): hand-rolled
  binary protocol, host-authoritative simulation, state-diff effects so
  packet loss self-heals, per-player colored avatars with lit meshes and
  emissive visors.

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
- `CR_SHOT=mphost ./crystal-rush` + `CR_SHOT=mpjoin ./crystal-rush` (two
  processes) — full loopback co-op session: connects over UDP, exchanges
  state and shots, saves `/tmp/crystal_rush_host.png` and
  `/tmp/crystal_rush_client.png` showing each other's avatars.
- `CR_AUDIOTEST=1 ./crystal-rush` — exercises the audio path briefly.
- `CR_NOAUDIO=1 ./crystal-rush` — run silent.
- `CR_JOIN=<ip>` — prefills the join-address box in the menu.
