//! The save file: a fixed-size, self-describing byte blob.
//!
//! Deliberately hand-rolled rather than a serde derive. The crate has no
//! dependencies and is `no_std` without `alloc`, and the state is a couple of
//! dozen scalars -- a format written out field by field is both smaller than the
//! machinery to avoid writing it, and easier to keep compatible by hand.
//!
//! A blob that fails to parse is treated as no save at all, so a corrupt or
//! older file costs the player their pet but never wedges the game.

use crate::pet::{Outcome, Pet};
use crate::Game;

/// Bumped whenever the layout below changes. Old versions are not migrated:
/// they are discarded, which for a 24-hour pet is a fair trade against carrying
/// migration code for every past shape.
const VERSION: u8 = 1;

/// Size of a save blob. Fixed, so the store can size its allocation exactly.
pub const SAVE_LEN: usize = 73;

/// Write the game out. The buffer is always fully written.
pub fn to_bytes(game: &Game) -> [u8; SAVE_LEN] {
    let mut out = [0u8; SAVE_LEN];
    let mut w = Writer { buf: &mut out, at: 0 };

    w.u8(VERSION);
    w.u64(game.uptime_ms());
    w.u64(game.gen_start_ms());
    w.u32(game.generation());
    w.u8(game.inherited());

    let p = game.pet();
    let s = p.save_state();
    w.u64(s.age_ms);
    w.u8(s.hunger_at_anchor);
    w.u64(s.hunger_anchor_ms);
    w.u8(s.hunger_misses);
    w.u8(s.mood_at_anchor);
    w.u64(s.mood_anchor_ms);
    w.u8(s.mood_misses);
    w.u64(s.poop_cleaned_ms);
    // `None` is the absent marker; a real illness always has a timestamp.
    w.u64(s.sick_since_ms.map_or(u64::MAX, |t| t));
    w.u8(s.sick_misses);
    w.u8(s.scolded_this_alert as u8);
    w.u8(s.care_miss);
    w.u16(s.weight);
    w.u8(s.discipline);
    w.u8(match s.outcome {
        None => 0,
        Some(Outcome::Lifespan) => 1,
        Some(Outcome::CareFailure) => 2,
    });

    debug_assert_eq!(w.at, SAVE_LEN, "SAVE_LEN does not match what to_bytes writes");
    out
}

/// Read a game back. `None` for anything this build does not recognise.
pub fn from_bytes(bytes: &[u8]) -> Option<Game> {
    if bytes.len() < SAVE_LEN {
        return None;
    }
    let mut r = Reader { buf: bytes, at: 0 };
    if r.u8() != VERSION {
        return None;
    }

    let uptime_ms = r.u64();
    let gen_start_ms = r.u64();
    let generation = r.u32();
    let inherited = r.u8();

    let state = PetState {
        age_ms: r.u64(),
        hunger_at_anchor: r.u8(),
        hunger_anchor_ms: r.u64(),
        hunger_misses: r.u8(),
        mood_at_anchor: r.u8(),
        mood_anchor_ms: r.u64(),
        mood_misses: r.u8(),
        poop_cleaned_ms: r.u64(),
        sick_since_ms: match r.u64() {
            u64::MAX => None,
            t => Some(t),
        },
        sick_misses: r.u8(),
        scolded_this_alert: r.u8() != 0,
        care_miss: r.u8(),
        weight: r.u16(),
        discipline: r.u8(),
        outcome: match r.u8() {
            1 => Some(Outcome::Lifespan),
            2 => Some(Outcome::CareFailure),
            _ => None,
        },
    };

    // Rebuilding through the constructor is what keeps the derived fields --
    // the decay periods widened by inherited discipline -- consistent with
    // `inherited`, rather than trusting a second copy of them on disk.
    let mut pet = Pet::new(inherited);
    pet.restore(&state);

    // Nonsense that would leave the simulation in an impossible place: a pet
    // older than the lineage it belongs to.
    if gen_start_ms > uptime_ms {
        return None;
    }
    Some(Game::restore(uptime_ms, gen_start_ms, generation.max(1), inherited, pet))
}

/// Everything a [`Pet`] keeps that is not derived. Lives here rather than in
/// `pet` so that the field list and the byte layout stay next to each other.
pub struct PetState {
    pub age_ms: u64,
    pub hunger_at_anchor: u8,
    pub hunger_anchor_ms: u64,
    pub hunger_misses: u8,
    pub mood_at_anchor: u8,
    pub mood_anchor_ms: u64,
    pub mood_misses: u8,
    pub poop_cleaned_ms: u64,
    pub sick_since_ms: Option<u64>,
    pub sick_misses: u8,
    pub scolded_this_alert: bool,
    pub care_miss: u8,
    pub weight: u16,
    pub discipline: u8,
    pub outcome: Option<Outcome>,
}

struct Writer<'a> {
    buf: &'a mut [u8],
    at: usize,
}

impl Writer<'_> {
    fn u8(&mut self, v: u8) {
        self.buf[self.at] = v;
        self.at += 1;
    }

    fn u16(&mut self, v: u16) {
        self.buf[self.at..self.at + 2].copy_from_slice(&v.to_le_bytes());
        self.at += 2;
    }

    fn u32(&mut self, v: u32) {
        self.buf[self.at..self.at + 4].copy_from_slice(&v.to_le_bytes());
        self.at += 4;
    }

    fn u64(&mut self, v: u64) {
        self.buf[self.at..self.at + 8].copy_from_slice(&v.to_le_bytes());
        self.at += 8;
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> u8 {
        let v = self.buf[self.at];
        self.at += 1;
        v
    }

    fn u16(&mut self) -> u16 {
        let mut b = [0u8; 2];
        b.copy_from_slice(&self.buf[self.at..self.at + 2]);
        self.at += 2;
        u16::from_le_bytes(b)
    }

    fn u32(&mut self) -> u32 {
        let mut b = [0u8; 4];
        b.copy_from_slice(&self.buf[self.at..self.at + 4]);
        self.at += 4;
        u32::from_le_bytes(b)
    }

    fn u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&self.buf[self.at..self.at + 8]);
        self.at += 8;
        u64::from_le_bytes(b)
    }
}
