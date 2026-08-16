//! The two clocks the game runs on.
//!
//! * **uptime** -- cumulative milliseconds the badge has been powered. Never wall
//!   clock time; see `docs/spec.md` for why the RTC cannot give us one.
//! * **awake time** -- uptime with the pet's nights subtracted. Needs (hunger,
//!   mood, poop) run on this one, so leaving the badge on overnight does not
//!   starve the pet while it sleeps.
//!
//! Both are pure functions of uptime. Nothing here accumulates, which is what
//! keeps the simulation independent of how often `tick()` happens to be called.

/// Uptime that makes one in-game day. `docs/spec.md`: cumulative 2 h.
pub const UPTIME_MS_PER_GAME_DAY: u64 = 2 * 60 * 60 * 1000;

/// The pet sleeps from in-game 00:00 until this hour. Four in-game hours is ten
/// minutes of uptime per hour: enough for the night to be a thing that happens,
/// short enough not to eat a noticeable share of a 24-hour lifespan.
pub const WAKE_HOUR: u64 = 4;

/// In-game time at which an egg is laid. Starting the day at midnight would put
/// a newborn straight into the sleep screen; 08:00 gives it a full day first.
pub const START_HOUR: u64 = 8;

const MS_PER_GAME_HOUR: u64 = UPTIME_MS_PER_GAME_DAY / 24;
const SLEEP_MS: u64 = WAKE_HOUR * MS_PER_GAME_HOUR;
const AWAKE_MS_PER_DAY: u64 = UPTIME_MS_PER_GAME_DAY - SLEEP_MS;
const START_OFFSET_MS: u64 = START_HOUR * MS_PER_GAME_HOUR;

/// A point on the in-game calendar. Days count from 1, and reset with each
/// generation -- `day` is the pet's age, not a running total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameTime {
    pub day: u64,
    pub hour: u64,
    pub minute: u64,
}

/// Roughly where in the day the pet is. The screen has no room for a clock, and
/// a player mostly wants to know how long is left before bed rather than the
/// hour -- `Night` is the warning that sleep is coming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeOfDay {
    Morning,
    Noon,
    Evening,
    Night,
}

impl TimeOfDay {
    pub fn at(hour: u64) -> Self {
        match hour {
            6..=10 => TimeOfDay::Morning,
            11..=16 => TimeOfDay::Noon,
            17..=20 => TimeOfDay::Evening,
            // 21:00 onwards, through the small hours until the pet wakes.
            _ => TimeOfDay::Night,
        }
    }
}

/// In-game time at a given age.
pub fn game_time(age_ms: u64) -> GameTime {
    let t = age_ms.saturating_add(START_OFFSET_MS);
    let day = t / UPTIME_MS_PER_GAME_DAY + 1;
    let into_day = t % UPTIME_MS_PER_GAME_DAY;
    let minutes = into_day * 24 * 60 / UPTIME_MS_PER_GAME_DAY;
    GameTime { day, hour: minutes / 60, minute: minutes % 60 }
}

/// Whether the pet is asleep at a given age.
pub fn is_asleep(age_ms: u64) -> bool {
    age_ms.saturating_add(START_OFFSET_MS) % UPTIME_MS_PER_GAME_DAY < SLEEP_MS
}

/// Awake milliseconds between birth and `age_ms`.
pub fn awake_ms(age_ms: u64) -> u64 {
    /// Awake time since in-game midnight of day 1, which is `START_OFFSET_MS`
    /// before birth. The caller subtracts that head start back off.
    fn since_midnight(t: u64) -> u64 {
        let days = t / UPTIME_MS_PER_GAME_DAY;
        let into_day = t % UPTIME_MS_PER_GAME_DAY;
        days * AWAKE_MS_PER_DAY + into_day.saturating_sub(SLEEP_MS)
    }
    since_midnight(age_ms.saturating_add(START_OFFSET_MS)) - since_midnight(START_OFFSET_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 60 * 60 * 1000;
    const MIN: u64 = 60 * 1000;

    #[test]
    fn a_newborn_wakes_at_eight() {
        assert_eq!(game_time(0), GameTime { day: 1, hour: 8, minute: 0 });
        assert!(!is_asleep(0));
    }

    #[test]
    fn the_calendar_advances_with_uptime() {
        // A quarter of an in-game day is six hours.
        assert_eq!(game_time(UPTIME_MS_PER_GAME_DAY / 4), GameTime { day: 1, hour: 14, minute: 0 });
        // 08:00 + 16 h rolls over to day 2.
        assert_eq!(game_time(UPTIME_MS_PER_GAME_DAY * 2 / 3), GameTime { day: 2, hour: 0, minute: 0 });
        assert_eq!(game_time(UPTIME_MS_PER_GAME_DAY), GameTime { day: 2, hour: 8, minute: 0 });
    }

    #[test]
    fn nights_are_the_first_sixth_of_each_day() {
        let midnight = UPTIME_MS_PER_GAME_DAY * 2 / 3;
        let night = UPTIME_MS_PER_GAME_DAY * WAKE_HOUR / 24;
        assert!(!is_asleep(midnight - 1));
        assert!(is_asleep(midnight));
        assert!(is_asleep(midnight + night - 1));
        // 04:00 -- awake again.
        assert!(!is_asleep(midnight + night));
    }

    #[test]
    fn awake_time_stalls_overnight() {
        let midnight = UPTIME_MS_PER_GAME_DAY * 2 / 3; // 80 min of uptime
        assert_eq!(awake_ms(0), 0);
        assert_eq!(awake_ms(10 * MIN), 10 * MIN);
        assert_eq!(awake_ms(midnight), midnight);
        // Half the night passes, and the awake clock has not moved.
        assert_eq!(awake_ms(midnight + 10 * MIN), midnight);
        // 04:00: it starts again from where it stopped.
        assert_eq!(awake_ms(midnight + 20 * MIN), midnight);
        assert_eq!(awake_ms(midnight + 30 * MIN), midnight + 10 * MIN);
    }

    #[test]
    fn awake_time_loses_a_sixth_of_every_day() {
        // Over a full lifespan: 24 h of uptime, minus 4 h of in-game night per
        // 2 h of uptime -- 20 minutes a day, 10 per hour of uptime.
        assert_eq!(awake_ms(24 * HOUR), 24 * HOUR * 5 / 6);
    }

    #[test]
    fn awake_time_never_goes_backwards() {
        let mut prev = 0;
        for age in (0..26 * HOUR).step_by(MIN as usize) {
            let now = awake_ms(age);
            assert!(now >= prev, "awake_ms went backwards at {age}");
            prev = now;
        }
    }

    #[test]
    fn the_day_is_divided_into_four() {
        assert_eq!(TimeOfDay::at(8), TimeOfDay::Morning);
        assert_eq!(TimeOfDay::at(12), TimeOfDay::Noon);
        assert_eq!(TimeOfDay::at(18), TimeOfDay::Evening);
        // Night has to cover both sides of midnight, including the hours the
        // pet is actually asleep.
        assert_eq!(TimeOfDay::at(22), TimeOfDay::Night);
        assert_eq!(TimeOfDay::at(2), TimeOfDay::Night);
        assert_eq!(TimeOfDay::at(5), TimeOfDay::Night);
        // The pet wakes at 04:00, so morning starts a little after it is up.
        assert_eq!(TimeOfDay::at(WAKE_HOUR), TimeOfDay::Night);
    }

    #[test]
    fn the_top_of_the_range_does_not_panic() {
        let t = game_time(u64::MAX);
        assert!(t.hour < 24 && t.minute < 60);
        awake_ms(u64::MAX);
    }
}
