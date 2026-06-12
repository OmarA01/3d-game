// UDP netcode for Crystal Rush co-op.
//
// Host-authoritative model: the host runs the full simulation and sends
// snapshots ~30 Hz; clients send their own position + look + reliable shot
// events every frame. Everything except shots is full-state in every
// snapshot, so a lost packet heals on the next one. Level changes ride in
// the snapshot header (level + seed) — clients regenerate the identical
// maze from the seed.

use macroquad::math::{vec2, vec3, Vec2, Vec3};
use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

pub const MAGIC: u16 = 0xC57A;
pub const VER: u8 = 1;
pub const DEFAULT_PORT: u16 = 24777;
pub const MAX_PLAYERS: usize = 4;

// player blob flags
pub const PF_ALIVE: u8 = 1;
pub const PF_DASH: u8 = 2;
pub const PF_OVERDRIVE: u8 = 4;

pub fn quant_angle(a: f32) -> u8 {
    let tau = std::f32::consts::TAU;
    (((a % tau + tau) % tau) / tau * 255.0) as u8
}

pub fn dequant_angle(q: u8) -> f32 {
    q as f32 / 255.0 * std::f32::consts::TAU
}

// ------------------------------------------------------------- byte buffers

pub struct Wr {
    pub b: Vec<u8>,
}

impl Wr {
    fn new(kind: u8) -> Wr {
        let mut w = Wr { b: Vec::with_capacity(1200) };
        w.u16(MAGIC);
        w.u8(kind);
        w
    }
    fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn v2(&mut self, v: Vec2) {
        self.f32(v.x);
        self.f32(v.y);
    }
    fn v3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
}

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Rd<'a> {
    fn new(b: &'a [u8]) -> Rd<'a> {
        Rd { b, i: 0 }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.i + n > self.b.len() {
            return None;
        }
        let s = &self.b[self.i..self.i + n];
        self.i += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn v2(&mut self) -> Option<Vec2> {
        Some(vec2(self.f32()?, self.f32()?))
    }
    fn v3(&mut self) -> Option<Vec3> {
        Some(vec3(self.f32()?, self.f32()?, self.f32()?))
    }
}

// ------------------------------------------------------------------ packets

#[derive(Clone)]
pub struct PlayerBlob {
    pub id: u8,
    pub pos: Vec2,
    pub vel: Vec2,
    pub yaw: f32,
    pub pitch: f32,
    pub hp: f32,
    pub flags: u8,
    pub respawn_t: u8, // deciseconds
    pub combo: u8,
    pub combo_t: u8, // deciseconds
    pub hurt_ctr: u8,
    pub hurt_dir: u8, // quantized world angle
    pub shot_ctr: u8,
    pub od_t: u8, // overdrive remaining, deciseconds
}

#[derive(Clone)]
pub struct DroneBlob {
    pub id: u8,
    pub pos: Vec2,
    pub dir: u8,
    pub state: u8, // 0 patrol, 1 chase, 2 investigate
    pub hp: u8,
}

#[derive(Clone)]
pub struct TurretBlob {
    pub alive: bool,
    pub aim: u8,
    pub charge: u8, // 0..255 ~ fire readiness
    pub hp: u8,
}

#[derive(Clone)]
pub struct ProjBlob {
    pub pos: Vec2,
    pub vel: Vec2,
}

#[derive(Clone)]
pub struct PickupBlob {
    pub kind: u8,
    pub pos: Vec2,
    pub taken: bool,
}

#[derive(Clone)]
pub struct Snapshot {
    pub seq: u32,
    pub shot_ack: u16,
    pub echo_seq: u32,
    pub level: u32,
    pub seed: u64,
    pub score: i64,
    pub phase: u8, // 0 playing, 1 level-clear transition
    pub kill_ctr: u8,
    pub kill_pos: Vec2,
    pub kill_big: u8,
    pub crystal_mask: u32,
    pub players: Vec<PlayerBlob>,
    pub drones: Vec<DroneBlob>,
    pub turrets: Vec<TurretBlob>,
    pub projectiles: Vec<ProjBlob>,
    pub pickups: Vec<PickupBlob>,
}

#[derive(Clone)]
pub struct ShotEv {
    pub id: u16,
    pub origin: Vec3,
    pub dir: Vec3,
}

#[derive(Clone)]
pub struct ClientState {
    pub id: u8,
    pub seq: u32,
    pub pos: Vec2,
    pub vel: Vec2,
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u8,
    pub shots: Vec<ShotEv>,
}

pub enum Packet {
    Hello { ver: u8 },
    Welcome { ver: u8, id: u8, level: u32, seed: u64, score: i64, spawn: Vec2 },
    State(ClientState),
    Snap(Box<Snapshot>),
    Bye { id: u8 },
}

impl Packet {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Packet::Hello { ver } => {
                let mut w = Wr::new(1);
                w.u8(*ver);
                w.b
            }
            Packet::Welcome { ver, id, level, seed, score, spawn } => {
                let mut w = Wr::new(2);
                w.u8(*ver);
                w.u8(*id);
                w.u32(*level);
                w.u64(*seed);
                w.i64(*score);
                w.v2(*spawn);
                w.b
            }
            Packet::State(s) => {
                let mut w = Wr::new(3);
                w.u8(s.id);
                w.u32(s.seq);
                w.v2(s.pos);
                w.v2(s.vel);
                w.f32(s.yaw);
                w.f32(s.pitch);
                w.u8(s.flags);
                w.u8(s.shots.len().min(12) as u8);
                for sh in s.shots.iter().take(12) {
                    w.u16(sh.id);
                    w.v3(sh.origin);
                    w.v3(sh.dir);
                }
                w.b
            }
            Packet::Snap(s) => {
                let mut w = Wr::new(4);
                w.u32(s.seq);
                w.u16(s.shot_ack);
                w.u32(s.echo_seq);
                w.u32(s.level);
                w.u64(s.seed);
                w.i64(s.score);
                w.u8(s.phase);
                w.u8(s.kill_ctr);
                w.v2(s.kill_pos);
                w.u8(s.kill_big);
                w.u32(s.crystal_mask);
                w.u8(s.players.len() as u8);
                for p in &s.players {
                    w.u8(p.id);
                    w.v2(p.pos);
                    w.v2(p.vel);
                    w.f32(p.yaw);
                    w.f32(p.pitch);
                    w.f32(p.hp);
                    w.u8(p.flags);
                    w.u8(p.respawn_t);
                    w.u8(p.combo);
                    w.u8(p.combo_t);
                    w.u8(p.hurt_ctr);
                    w.u8(p.hurt_dir);
                    w.u8(p.shot_ctr);
                    w.u8(p.od_t);
                }
                w.u8(s.drones.len().min(255) as u8);
                for d in &s.drones {
                    w.u8(d.id);
                    w.v2(d.pos);
                    w.u8(d.dir);
                    w.u8(d.state);
                    w.u8(d.hp);
                }
                w.u8(s.turrets.len() as u8);
                for t in &s.turrets {
                    w.u8(t.alive as u8);
                    w.u8(t.aim);
                    w.u8(t.charge);
                    w.u8(t.hp);
                }
                w.u8(s.projectiles.len().min(48) as u8);
                for p in s.projectiles.iter().take(48) {
                    w.v2(p.pos);
                    w.v2(p.vel);
                }
                w.u8(s.pickups.len() as u8);
                for p in &s.pickups {
                    w.u8(p.kind);
                    w.v2(p.pos);
                    w.u8(p.taken as u8);
                }
                w.b
            }
            Packet::Bye { id } => {
                let mut w = Wr::new(5);
                w.u8(*id);
                w.b
            }
        }
    }

    pub fn decode(buf: &[u8]) -> Option<Packet> {
        let mut r = Rd::new(buf);
        if r.u16()? != MAGIC {
            return None;
        }
        match r.u8()? {
            1 => Some(Packet::Hello { ver: r.u8()? }),
            2 => Some(Packet::Welcome {
                ver: r.u8()?,
                id: r.u8()?,
                level: r.u32()?,
                seed: r.u64()?,
                score: r.i64()?,
                spawn: r.v2()?,
            }),
            3 => {
                let id = r.u8()?;
                let seq = r.u32()?;
                let pos = r.v2()?;
                let vel = r.v2()?;
                let yaw = r.f32()?;
                let pitch = r.f32()?;
                let flags = r.u8()?;
                let n = r.u8()? as usize;
                let mut shots = Vec::with_capacity(n);
                for _ in 0..n {
                    shots.push(ShotEv { id: r.u16()?, origin: r.v3()?, dir: r.v3()? });
                }
                Some(Packet::State(ClientState { id, seq, pos, vel, yaw, pitch, flags, shots }))
            }
            4 => {
                let seq = r.u32()?;
                let shot_ack = r.u16()?;
                let echo_seq = r.u32()?;
                let level = r.u32()?;
                let seed = r.u64()?;
                let score = r.i64()?;
                let phase = r.u8()?;
                let kill_ctr = r.u8()?;
                let kill_pos = r.v2()?;
                let kill_big = r.u8()?;
                let crystal_mask = r.u32()?;
                let np = r.u8()? as usize;
                let mut players = Vec::with_capacity(np);
                for _ in 0..np {
                    players.push(PlayerBlob {
                        id: r.u8()?,
                        pos: r.v2()?,
                        vel: r.v2()?,
                        yaw: r.f32()?,
                        pitch: r.f32()?,
                        hp: r.f32()?,
                        flags: r.u8()?,
                        respawn_t: r.u8()?,
                        combo: r.u8()?,
                        combo_t: r.u8()?,
                        hurt_ctr: r.u8()?,
                        hurt_dir: r.u8()?,
                        shot_ctr: r.u8()?,
                        od_t: r.u8()?,
                    });
                }
                let nd = r.u8()? as usize;
                let mut drones = Vec::with_capacity(nd);
                for _ in 0..nd {
                    drones.push(DroneBlob {
                        id: r.u8()?,
                        pos: r.v2()?,
                        dir: r.u8()?,
                        state: r.u8()?,
                        hp: r.u8()?,
                    });
                }
                let nt = r.u8()? as usize;
                let mut turrets = Vec::with_capacity(nt);
                for _ in 0..nt {
                    turrets.push(TurretBlob {
                        alive: r.u8()? != 0,
                        aim: r.u8()?,
                        charge: r.u8()?,
                        hp: r.u8()?,
                    });
                }
                let npr = r.u8()? as usize;
                let mut projectiles = Vec::with_capacity(npr);
                for _ in 0..npr {
                    projectiles.push(ProjBlob { pos: r.v2()?, vel: r.v2()? });
                }
                let npk = r.u8()? as usize;
                let mut pickups = Vec::with_capacity(npk);
                for _ in 0..npk {
                    pickups.push(PickupBlob { kind: r.u8()?, pos: r.v2()?, taken: r.u8()? != 0 });
                }
                Some(Packet::Snap(Box::new(Snapshot {
                    seq,
                    shot_ack,
                    echo_seq,
                    level,
                    seed,
                    score,
                    phase,
                    kill_ctr,
                    kill_pos,
                    kill_big,
                    crystal_mask,
                    players,
                    drones,
                    turrets,
                    projectiles,
                    pickups,
                })))
            }
            5 => Some(Packet::Bye { id: r.u8()? }),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ sockets

pub struct HostClient {
    pub addr: SocketAddr,
    pub id: u8,
    pub last_recv: f64,
    pub last_state_seq: u32,
    pub shot_ack: u16,
    pub echo_seq: u32,
}

pub struct HostNet {
    pub sock: UdpSocket,
    pub port: u16,
    pub clients: Vec<HostClient>,
    pub next_id: u8,
    pub snap_seq: u32,
}

impl HostNet {
    pub fn bind(port: u16) -> io::Result<HostNet> {
        let sock = UdpSocket::bind(("0.0.0.0", port))?;
        sock.set_nonblocking(true)?;
        Ok(HostNet { sock, port, clients: Vec::new(), next_id: 1, snap_seq: 0 })
    }

    pub fn recv_all(&mut self) -> Vec<(SocketAddr, Packet)> {
        let mut out = Vec::new();
        let mut buf = [0u8; 2048];
        while let Ok((n, addr)) = self.sock.recv_from(&mut buf) {
            if let Some(p) = Packet::decode(&buf[..n]) {
                out.push((addr, p));
            }
        }
        out
    }

    pub fn send_to(&self, addr: SocketAddr, p: &Packet) {
        let _ = self.sock.send_to(&p.encode(), addr);
    }
}

pub struct ClientNet {
    pub sock: UdpSocket,
    pub server: SocketAddr,
    pub my_id: Option<u8>,
    pub state_seq: u32,
    pub last_recv: f64,
    pub last_snap_seq: u32,
    pub snaps: VecDeque<(f64, Snapshot)>, // recv time, snapshot
    pub pending_shots: Vec<ShotEv>,
    pub next_shot_id: u16,
    pub sent_times: VecDeque<(u32, f64)>,
    pub rtt: f32,
    pub hello_t: f64,
    pub started: f64,
}

impl ClientNet {
    pub fn connect(addr_str: &str, now: f64) -> io::Result<ClientNet> {
        let with_port = if addr_str.contains(':') {
            addr_str.to_string()
        } else {
            format!("{}:{}", addr_str, DEFAULT_PORT)
        };
        let server = with_port
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address"))?;
        let sock = UdpSocket::bind("0.0.0.0:0")?;
        sock.set_nonblocking(true)?;
        Ok(ClientNet {
            sock,
            server,
            my_id: None,
            state_seq: 0,
            last_recv: now,
            last_snap_seq: 0,
            snaps: VecDeque::new(),
            pending_shots: Vec::new(),
            next_shot_id: 1,
            sent_times: VecDeque::new(),
            rtt: 0.0,
            hello_t: -10.0,
            started: now,
        })
    }

    pub fn recv_all(&mut self) -> Vec<Packet> {
        let mut out = Vec::new();
        let mut buf = [0u8; 2048];
        while let Ok((n, addr)) = self.sock.recv_from(&mut buf) {
            if addr != self.server {
                continue;
            }
            if let Some(p) = Packet::decode(&buf[..n]) {
                out.push(p);
            }
        }
        out
    }

    pub fn send(&self, p: &Packet) {
        let _ = self.sock.send_to(&p.encode(), self.server);
    }

    pub fn queue_shot(&mut self, origin: Vec3, dir: Vec3) {
        if self.pending_shots.len() >= 12 {
            self.pending_shots.remove(0);
        }
        let id = self.next_shot_id;
        self.next_shot_id = self.next_shot_id.wrapping_add(1);
        if self.next_shot_id == 0 {
            self.next_shot_id = 1;
        }
        self.pending_shots.push(ShotEv { id, origin, dir });
    }

    pub fn ack_shots(&mut self, ack: u16) {
        if ack == 0 {
            return;
        }
        // ids increase monotonically; drop everything at-or-before `ack`
        // (wrapping comparison keeps this correct across u16 wrap)
        self.pending_shots.retain(|s| s.id.wrapping_sub(ack).wrapping_sub(1) < 0x8000);
    }
}
