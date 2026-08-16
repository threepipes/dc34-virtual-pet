//! Game state and time arithmetic, with nothing Xous-shaped in it.
//!
//! Drawing, input and the Xous API live one level up in `dc34-virtual-pet`. Keeping
//! them out of here is what makes `cargo test` work on the host.

#![cfg_attr(not(test), no_std)]

/// Real time that maps to one in-game day, per the design doc.
pub const REAL_MS_PER_GAME_DAY: u64 = 2 * 60 * 60 * 1000;

/// Debug accelerator for the clock. At 1 the clock runs in real time, which makes
/// visual confirmation take hours; at 60 a full in-game day passes in two minutes.
pub const TIME_SCALE: u64 = 60;

/// A point on the in-game calendar. Days count from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameTime {
    pub day: u64,
    pub hour: u64,
    pub minute: u64,
}

/// Convert elapsed milliseconds into in-game time.
pub fn game_time(elapsed_ms: u64) -> GameTime {
    let scaled = elapsed_ms.saturating_mul(TIME_SCALE);
    let day = scaled / REAL_MS_PER_GAME_DAY + 1;
    let into_day = scaled % REAL_MS_PER_GAME_DAY;
    let minutes = into_day * 24 * 60 / REAL_MS_PER_GAME_DAY;
    GameTime { day, hour: minutes / 60, minute: minutes % 60 }
}

/// Everything the game remembers between frames. Today that is only the clock;
/// the pet itself lands here when the raising mechanics are implemented.
#[derive(Debug, Clone, Copy, Default)]
pub struct GameState {
    /// Host clock reading at the moment the game was entered.
    start_ms: u64,
    /// Most recent host clock reading handed to us.
    now_ms: u64,
}

impl GameState {
    pub fn new() -> Self { Self::default() }

    /// Take the time reference. Called every time the game is entered, so the
    /// clock restarts at day 1, 00:00.
    pub fn start(&mut self, now_ms: u64) {
        self.start_ms = now_ms;
        self.now_ms = now_ms;
    }

    /// Advance to a new host clock reading. The clock is monotonic from the
    /// caller's point of view, so a reading older than `start_ms` is clamped.
    pub fn tick(&mut self, now_ms: u64) { self.now_ms = now_ms.max(self.start_ms); }

    /// Milliseconds since the game was entered.
    pub fn elapsed_ms(&self) -> u64 { self.now_ms.saturating_sub(self.start_ms) }

    /// Current in-game time.
    pub fn game_time(&self) -> GameTime { game_time(self.elapsed_ms()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real milliseconds that produce `game_minutes` of in-game time. Only exact
    /// for multiples of 3 at the current scale (5000 ms per game minute / 60),
    /// so the callers below stick to those.
    fn real_ms(game_minutes: u64) -> u64 { game_minutes * REAL_MS_PER_GAME_DAY / (24 * 60) / TIME_SCALE }

    #[test]
    fn starts_at_day_one_midnight() {
        assert_eq!(game_time(0), GameTime { day: 1, hour: 0, minute: 0 });
    }

    #[test]
    fn minutes_advance() {
        assert_eq!(game_time(real_ms(3)), GameTime { day: 1, hour: 0, minute: 3 });
        assert_eq!(game_time(real_ms(90)), GameTime { day: 1, hour: 1, minute: 30 });
        assert_eq!(game_time(real_ms(1_437)), GameTime { day: 1, hour: 23, minute: 57 });
    }

    #[test]
    fn day_rolls_over() {
        let one_day = REAL_MS_PER_GAME_DAY / TIME_SCALE;
        assert_eq!(game_time(one_day - 1), GameTime { day: 1, hour: 23, minute: 59 });
        assert_eq!(game_time(one_day), GameTime { day: 2, hour: 0, minute: 0 });
        assert_eq!(game_time(3 * one_day + one_day / 2), GameTime { day: 4, hour: 12, minute: 0 });
    }

    #[test]
    fn no_overflow_at_the_top_of_the_range() {
        // `saturating_mul` keeps this from panicking in a debug build.
        let t = game_time(u64::MAX);
        assert!(t.hour < 24 && t.minute < 60);
    }

    #[test]
    fn state_is_relative_to_start() {
        let one_day = REAL_MS_PER_GAME_DAY / TIME_SCALE;
        let mut state = GameState::new();
        state.start(1_000_000);
        assert_eq!(state.elapsed_ms(), 0);
        assert_eq!(state.game_time(), GameTime { day: 1, hour: 0, minute: 0 });

        state.tick(1_000_000 + one_day);
        assert_eq!(state.game_time(), GameTime { day: 2, hour: 0, minute: 0 });
    }

    #[test]
    fn state_clamps_readings_before_start() {
        let mut state = GameState::new();
        state.start(500);
        state.tick(100);
        assert_eq!(state.elapsed_ms(), 0);
    }
}
