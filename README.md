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

The executable is fully self-contained (~1.5 MB) — no installation, no
asset files. It needs a Linux desktop with X11 (or XWayland), OpenGL and
ALSA/PipeWire for sound (all standard on any desktop install).

## Menu

The title screen is a vertical list — move it with `↑`/`↓` (or `W`/`S`),
hover with the mouse, and `Enter` or click to select:

- **PLAY** — start a solo run.
- **HOST CO-OP** / **JOIN CO-OP** — start or join an online game (`H`/`J`
  still jump straight there).
- **PILOT NAME** — set the name shown on your death screen, your partner's
  screen in co-op, and the minimap. Blank names fall back to `P1`–`P4`.
- **CAREER** — your Pilot Rank and full record (deepest level, best score,
  peak combo, accuracy, lifetime crystals/kills/turrets, flawless clears).
- **SETTINGS** — mouse sensitivity and master audio mute. Both persist.
- **CONTROLS** — the key reference.

Your name, sensitivity and mute choice live in the same save file as your
career records, so they survive restarts. `Esc` backs out of any sub-screen.

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
- Pickups: **medkits** (+30 HP), **overdrive** (8 s of double fire
  rate + extra speed), and **SKY VIEW** (level 2+): lifts you safely
  above the maze for 6 seconds to scout routes. You're frozen and invulnerable
  while airborne.
- **Hidden maze** (level 2+): the world and minimap flicker on/off together.
  Reveal time shrinks and gaps grow each round — use SKY VIEW wisely.
- Difficulty ramps each level: faster/stronger enemies, more drones/turrets,
  and tougher turrets.
- Score: crystals, kills, clear bonus, time bonus — all scaled by combo.
  Health carries between levels (+15 on clear).

## Pilot Ledger

Your skill is tracked across runs — the only thing that persists. Every solo
run records your deepest level, best score, peak combo, accuracy, and flawless
(no-hit) level clears to a small save file, and earns a **Pilot Rank** shown on
the menu and the death screen:

> ROOKIE → PILOT → NAVIGATOR → ACE → VANGUARD → GHOST

Ranks are earned only by *demonstrated skill* — each tier is a combination of
distinct feats (clear depth **and** a sustained combo **and** accuracy, etc.),
never by playtime. The ledger grants **zero** in-game power: it's a mirror of
how good you've gotten, not a stat boost. Beat one of your records and the death
screen calls it out with a **NEW PERSONAL BEST** banner.

The save lives in your platform config dir (`~/.config/crystal-rush/pilot.sav`
on Linux), overridable with `CRYSTAL_RUSH_DIR`. It's plain text, versioned, and
self-heals: a missing or corrupt file just starts you fresh as ROOKIE. Co-op
runs are not recorded (they never touch the deterministic netcode). Set
`CR_NOLEDGER=1` to disable persistence entirely.

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
at least one player alive. Partners show up tagged with their pilot name, a health bar,
and a colored dot on the minimap.

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
`src/net.rs` (~5300 lines total):

- **Lighting** — custom GLSL material with 12 dynamic point lights
  (crystals, drones, projectiles, muzzle flash, explosions, headlamp),
  Blinn-Phong specular, fresnel rim light, procedural surface detail, a
  glossy wet floor, emissive wall trim, animated floor grid, and per-pixel
  height + distance fog.
- **Post-processing** — the 3D scene renders to an off-screen buffer, then
  a full HDR-style chain runs as fullscreen passes: soft-knee bright
  extraction, three-scale Gaussian bloom, ACES filmic tone-mapping, a
  colour grade (teal/magenta split-tone), vignette, subtle chromatic
  aberration, film-grain dither and FXAA. Bright neon cores clamp to white
  so bloom + tone-map read like true HDR. (See `PostStack` in
  `src/main.rs`; it falls back to direct rendering if the shaders fail.)
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
- `CRYSTAL_RUSH_DIR=/tmp/cr CR_SHOT=ledger ./crystal-rush` — commits a synthetic
  finished run through the real run-end path and saves `/tmp/crystal_rush_ledger.png`
  of the death screen; the ledger lands in `$CRYSTAL_RUSH_DIR/pilot.sav`.
- `cargo test` — unit-tests the ledger (save/load round-trip, corrupt-file
  safety, rank thresholds).
