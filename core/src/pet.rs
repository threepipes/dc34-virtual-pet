//! One creature's life: needs, mess, illness and how it ends.
//!
//! Every derived value is a function of the pet's age plus a handful of anchors
//! recorded when the player last acted. Nothing is integrated tick by tick, so
//! feeding at 08:00 and checking back after a five-hour blackout gives exactly
//! the same result as watching the whole time. That property is the reason the
//! badge can sleep, lose power and resume without the simulation drifting.

use crate::time::{awake_ms, is_asleep};

// -- Lifetime -----------------------------------------------------------------

/// A generation lasts this much cumulative uptime.
///
/// TEMPORARY: `docs/spec.md` says 24 h. The teen and adult stages have been cut
/// from 8 h each to the child stage's 6 h so that a whole loop -- hatch, four
/// evolutions, generation change -- can be watched end to end, which drags the
/// lifespan down with them. Put all three back together.
pub const LIFESPAN_MS: u64 = 20 * 60 * 60 * 1000;

const STAGE_BABY_MS: u64 = 10 * 60 * 1000;
const STAGE_CHILD_MS: u64 = 2 * 60 * 60 * 1000;
const STAGE_TEEN_MS: u64 = 8 * 60 * 60 * 1000;
/// TEMPORARY: 16 h in the spec. See [`LIFESPAN_MS`].
const STAGE_ADULT_MS: u64 = 14 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Egg,
    Baby,
    Child,
    Teen,
    Adult,
}

impl Stage {
    /// The stage a pet of this age is in.
    pub fn at(age_ms: u64) -> Self {
        match age_ms {
            a if a < STAGE_BABY_MS => Stage::Egg,
            a if a < STAGE_CHILD_MS => Stage::Baby,
            a if a < STAGE_TEEN_MS => Stage::Child,
            a if a < STAGE_ADULT_MS => Stage::Teen,
            _ => Stage::Adult,
        }
    }

    /// Age at which this stage begins. Used to place the evolution animation.
    pub fn starts_at(self) -> u64 {
        match self {
            Stage::Egg => 0,
            Stage::Baby => STAGE_BABY_MS,
            Stage::Child => STAGE_CHILD_MS,
            Stage::Teen => STAGE_TEEN_MS,
            Stage::Adult => STAGE_ADULT_MS,
        }
    }

}

/// Stage spans as (start, end, poop speed), for integrating the poop clock.
/// Young pets make a mess twice as fast as grown ones; eggs make none, which is
/// why the table starts at the hatch.
const POOP_SEGMENTS: [(u64, u64, u64); 3] = [
    (STAGE_BABY_MS, STAGE_TEEN_MS, 2),
    (STAGE_TEEN_MS, STAGE_ADULT_MS, 1),
    (STAGE_ADULT_MS, LIFESPAN_MS, 1),
];

// -- Needs --------------------------------------------------------------------

/// Both meters run 0..=4, as four pips on the status bar.
pub const METER_MAX: u8 = 4;

/// Awake time per pip lost. `docs/spec.md` asks for 25-50 min; mood is the
/// slower of the two so that a pet left alone gets hungry before it gets sad.
const HUNGER_PERIOD_MS: u64 = 30 * 60 * 1000;
const MOOD_PERIOD_MS: u64 = 45 * 60 * 1000;

/// Each point of inherited discipline slows both meters by this much. A small
/// nudge on purpose: the reward for raising a pet well should be felt, not
/// large enough to make later generations play themselves.
const INHERIT_BONUS_MS: u64 = 3 * 60 * 1000;

/// Awake time per dropping, at adult speed.
const POOP_PERIOD_MS: u64 = 60 * 60 * 1000;
/// The screen has room for this many. Beyond it they stop piling up.
pub const POOP_MAX: u8 = 4;
/// Living in this much of its own mess makes a pet ill.
const SICK_POOP: u8 = 3;

/// How long the pet calls before an unanswered call becomes a care miss.
/// `docs/spec.md` §1: the 25-minute judgement granularity.
const ALERT_GRACE_MS: u64 = 25 * 60 * 1000;

/// Care misses that kill. Reaching this is a game over, not a generation change.
pub const CARE_MISS_LIMIT: u8 = 5;

const WEIGHT_START: u16 = 5;
const WEIGHT_MIN: u16 = 5;
const WEIGHT_MAX: u16 = 99;

pub const DISCIPLINE_MAX: u8 = 4;

/// How a generation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Lived the full 24 h. The next generation inherits.
    Lifespan,
    /// Died of neglect. The lineage resets.
    CareFailure,
}

/// Why the pet ignored a menu choice. The wording is the host's business; this
/// only says which of them applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Still an egg.
    Unhatched,
    /// Asleep, and not to be woken.
    Asleep,
    /// The meter is already full.
    Full,
    /// Nothing wrong with it.
    Healthy,
    /// Nothing to do -- no mess, nobody calling.
    Nothing,
}

pub type ActionResult = Result<(), Refusal>;

#[derive(Debug, Clone)]
pub struct Pet {
    /// Age this pet has been advanced to, capped at its lifespan.
    age_ms: u64,

    /// Meters are stored as "level at anchor" plus the awake time the anchor was
    /// taken, and decay is worked out on read. `*_misses` counts the care misses
    /// already credited against the current anchor, so re-reading is idempotent.
    hunger_at_anchor: u8,
    hunger_anchor_ms: u64,
    hunger_misses: u8,
    mood_at_anchor: u8,
    mood_anchor_ms: u64,
    mood_misses: u8,

    /// Droppings produced before the last sweep, in poop-clock milliseconds.
    poop_cleaned_ms: u64,

    /// Awake time at which the pet fell ill, if it is ill.
    sick_since_ms: Option<u64>,
    sick_misses: u8,

    /// A scolding only lands once per call, so it cannot be spammed for points.
    scolded_this_alert: bool,

    care_miss: u8,
    weight: u16,
    discipline: u8,

    /// Widened by inherited discipline.
    hunger_period_ms: u64,
    mood_period_ms: u64,

    outcome: Option<Outcome>,
}

impl Pet {
    /// A fresh egg. `inherited` is the previous generation's discipline.
    pub fn new(inherited: u8) -> Self {
        // Needs only start running once the egg hatches.
        let hatch = awake_ms(STAGE_BABY_MS);
        let bonus = inherited.min(DISCIPLINE_MAX) as u64 * INHERIT_BONUS_MS;
        Self {
            age_ms: 0,
            hunger_at_anchor: METER_MAX,
            hunger_anchor_ms: hatch,
            hunger_misses: 0,
            mood_at_anchor: METER_MAX,
            mood_anchor_ms: hatch,
            mood_misses: 0,
            poop_cleaned_ms: 0,
            sick_since_ms: None,
            sick_misses: 0,
            scolded_this_alert: false,
            care_miss: 0,
            weight: WEIGHT_START,
            discipline: 0,
            hunger_period_ms: HUNGER_PERIOD_MS + bonus,
            mood_period_ms: MOOD_PERIOD_MS + bonus,
            outcome: None,
        }
    }

    // -- Persistence ----------------------------------------------------------

    /// Everything this pet keeps that is not derived. The decay periods are
    /// left out on purpose: they follow from the inherited discipline the
    /// constructor is given, so storing them again would let the two disagree.
    pub fn save_state(&self) -> crate::save::PetState {
        crate::save::PetState {
            age_ms: self.age_ms,
            hunger_at_anchor: self.hunger_at_anchor,
            hunger_anchor_ms: self.hunger_anchor_ms,
            hunger_misses: self.hunger_misses,
            mood_at_anchor: self.mood_at_anchor,
            mood_anchor_ms: self.mood_anchor_ms,
            mood_misses: self.mood_misses,
            poop_cleaned_ms: self.poop_cleaned_ms,
            sick_since_ms: self.sick_since_ms,
            sick_misses: self.sick_misses,
            scolded_this_alert: self.scolded_this_alert,
            care_miss: self.care_miss,
            weight: self.weight,
            discipline: self.discipline,
            outcome: self.outcome,
        }
    }

    /// Put a saved pet back. Call on a pet built with the matching `inherited`.
    pub fn restore(&mut self, s: &crate::save::PetState) {
        self.age_ms = s.age_ms.min(LIFESPAN_MS);
        self.hunger_at_anchor = s.hunger_at_anchor.min(METER_MAX);
        self.hunger_anchor_ms = s.hunger_anchor_ms;
        self.hunger_misses = s.hunger_misses;
        self.mood_at_anchor = s.mood_at_anchor.min(METER_MAX);
        self.mood_anchor_ms = s.mood_anchor_ms;
        self.mood_misses = s.mood_misses;
        self.poop_cleaned_ms = s.poop_cleaned_ms;
        self.sick_since_ms = s.sick_since_ms;
        self.sick_misses = s.sick_misses;
        self.scolded_this_alert = s.scolded_this_alert;
        self.care_miss = s.care_miss;
        self.weight = s.weight;
        self.discipline = s.discipline.min(DISCIPLINE_MAX);
        self.outcome = s.outcome;
    }

    // -- Derived state --------------------------------------------------------

    pub fn age_ms(&self) -> u64 { self.age_ms }
    pub fn stage(&self) -> Stage { Stage::at(self.age_ms) }
    pub fn asleep(&self) -> bool { is_asleep(self.age_ms) }
    pub fn weight(&self) -> u16 { self.weight }
    pub fn discipline(&self) -> u8 { self.discipline }
    pub fn care_miss(&self) -> u8 { self.care_miss }
    pub fn sick(&self) -> bool { self.sick_since_ms.is_some() }
    pub fn outcome(&self) -> Option<Outcome> { self.outcome }

    pub fn hunger(&self) -> u8 {
        // The anchor sits at the hatch, which is ahead of an egg's awake clock;
        // saturating keeps the meter full until then instead of underflowing.
        let elapsed = self.awake().saturating_sub(self.hunger_anchor_ms);
        meter(self.hunger_at_anchor, elapsed, self.hunger_period_ms)
    }

    pub fn mood(&self) -> u8 {
        let elapsed = self.awake().saturating_sub(self.mood_anchor_ms);
        meter(self.mood_at_anchor, elapsed, self.mood_period_ms)
    }

    pub fn poop(&self) -> u8 {
        let produced = self.poop_clock(self.age_ms).saturating_sub(self.poop_cleaned_ms);
        ((produced / POOP_PERIOD_MS).min(POOP_MAX as u64)) as u8
    }

    /// The pet is calling for help: a meter has bottomed out, or it is ill.
    /// Answering within [`ALERT_GRACE_MS`] avoids a care miss.
    pub fn alert(&self) -> bool {
        !self.asleep() && (self.hunger() == 0 || self.mood() == 0 || self.sick())
    }

    fn awake(&self) -> u64 { awake_ms(self.age_ms) }

    /// Poop-clock milliseconds at a given age: awake time, weighted by how fast
    /// the pet was producing at each stage.
    fn poop_clock(&self, age_ms: u64) -> u64 {
        let mut acc = 0;
        for (start, end, speed) in POOP_SEGMENTS {
            if age_ms <= start {
                break;
            }
            acc += speed * (awake_ms(age_ms.min(end)) - awake_ms(start));
        }
        acc
    }

    /// The age at which the poop clock first reached `target`. Inverting it by
    /// bisection beats deriving a closed form: the clock is piecewise linear in
    /// both stage and night, and this is called at most once per illness.
    fn age_at_poop_clock(&self, target: u64) -> u64 {
        let (mut lo, mut hi) = (0u64, self.age_ms);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.poop_clock(mid) >= target {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        lo
    }

    // -- Time -----------------------------------------------------------------

    /// Advance to `age_ms` and settle anything that fell due on the way: illness
    /// from an unswept pen, care misses from unanswered calls, and death.
    ///
    /// Only ever moves forward, and does nothing once the generation has ended.
    pub fn advance_to(&mut self, age_ms: u64) {
        if self.outcome.is_some() {
            return;
        }
        self.age_ms = age_ms.max(self.age_ms).min(LIFESPAN_MS);

        if self.sick_since_ms.is_none() && self.poop() >= SICK_POOP {
            // Date the illness from the dropping that caused it, not from
            // whenever we happened to look, so a long blackout still charges the
            // player for the neglect that happened during it.
            let target = self.poop_cleaned_ms + SICK_POOP as u64 * POOP_PERIOD_MS;
            self.sick_since_ms = Some(awake_ms(self.age_at_poop_clock(target)));
            self.sick_misses = 0;
        }

        // A pet that ran out of care during this step died back when the last
        // miss landed, not at whatever age we happened to look. Rewinding is
        // what lets a badge left running for hours still report the right age on
        // the game-over screen.
        if self.total_misses_at(self.age_ms) >= CARE_MISS_LIMIT {
            self.age_ms = self.age_of_fatal_miss();
            self.outcome = Some(Outcome::CareFailure);
            if self.sick_since_ms.is_some_and(|since| since > self.awake()) {
                self.sick_since_ms = None; // it died before falling ill
            }
        }

        self.settle_care_misses();

        if self.outcome.is_none() && self.age_ms >= LIFESPAN_MS {
            self.outcome = Some(Outcome::Lifespan);
        }
    }

    /// Awake time at which each meter bottoms out, given the current anchors.
    fn hunger_zero_ms(&self) -> u64 {
        self.hunger_anchor_ms + self.hunger_at_anchor as u64 * self.hunger_period_ms
    }

    fn mood_zero_ms(&self) -> u64 {
        self.mood_anchor_ms + self.mood_at_anchor as u64 * self.mood_period_ms
    }

    /// Total care misses the pet would have at `age_ms`, counting the ones not
    /// yet charged. Monotonic in age, which is what makes the bisection below
    /// legitimate.
    fn total_misses_at(&self, age_ms: u64) -> u8 {
        let awake = awake_ms(age_ms);
        let hunger = overdue(awake, self.hunger_zero_ms()).saturating_sub(self.hunger_misses);
        let mood = overdue(awake, self.mood_zero_ms()).saturating_sub(self.mood_misses);
        let sick = self
            .sick_since_ms
            .map_or(0, |since| overdue(awake, since))
            .saturating_sub(self.sick_misses);
        self.care_miss.saturating_add(hunger).saturating_add(mood).saturating_add(sick)
    }

    /// Earliest age at which the care-miss count reaches the fatal limit.
    fn age_of_fatal_miss(&self) -> u64 {
        let (mut lo, mut hi) = (0u64, self.age_ms);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.total_misses_at(mid) >= CARE_MISS_LIMIT {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        lo
    }

    /// Charge one care miss per grace period each call has gone unanswered.
    ///
    /// Each source counts from its own start, and remembers how many it has
    /// already been charged for, so calling this twice charges nothing twice.
    fn settle_care_misses(&mut self) {
        let awake = self.awake();
        self.care_miss = self.total_misses_at(self.age_ms);
        self.hunger_misses = overdue(awake, self.hunger_zero_ms());
        self.mood_misses = overdue(awake, self.mood_zero_ms());
        self.sick_misses = self.sick_since_ms.map_or(0, |since| overdue(awake, since));

        if !self.alert() {
            self.scolded_this_alert = false;
        }
    }

    // -- Actions --------------------------------------------------------------

    /// Actions the player takes are refused wholesale while the pet is an egg or
    /// asleep, so each one only has to state its own reason for saying no.
    fn available(&self) -> ActionResult {
        match self.stage() {
            Stage::Egg => Err(Refusal::Unhatched),
            _ if self.asleep() => Err(Refusal::Asleep),
            _ => Ok(()),
        }
    }

    /// A meal: one pip of hunger, one gram.
    pub fn feed_meal(&mut self) -> ActionResult {
        self.available()?;
        if self.hunger() >= METER_MAX {
            return Err(Refusal::Full);
        }
        self.set_hunger(self.hunger() + 1);
        self.add_weight(1);
        Ok(())
    }

    /// A snack: one pip of mood, two grams. The lazy way to cheer a pet up, and
    /// priced accordingly.
    pub fn feed_snack(&mut self) -> ActionResult {
        self.available()?;
        if self.mood() >= METER_MAX {
            return Err(Refusal::Full);
        }
        self.set_mood(self.mood() + 1);
        self.add_weight(2);
        Ok(())
    }

    pub fn clean(&mut self) -> ActionResult {
        self.available()?;
        if self.poop() == 0 {
            return Err(Refusal::Nothing);
        }
        // Sweep away whole droppings only. Carrying the fraction over means a
        // tidy player cannot delay the next one by sweeping early.
        let produced = self.poop_clock(self.age_ms) - self.poop_cleaned_ms;
        self.poop_cleaned_ms += produced / POOP_PERIOD_MS * POOP_PERIOD_MS;
        Ok(())
    }

    pub fn medicate(&mut self) -> ActionResult {
        self.available()?;
        if self.sick_since_ms.is_none() {
            return Err(Refusal::Healthy);
        }
        self.sick_since_ms = None;
        self.sick_misses = 0;
        Ok(())
    }

    /// Scolding only means anything while the pet is calling, and only counts
    /// once per call.
    pub fn scold(&mut self) -> ActionResult {
        self.available()?;
        if !self.alert() || self.scolded_this_alert {
            return Err(Refusal::Nothing);
        }
        self.scolded_this_alert = true;
        self.discipline = (self.discipline + 1).min(DISCIPLINE_MAX);
        Ok(())
    }

    /// Result of a round of the guessing game: winning cheers the pet up more,
    /// and either way it burns a gram.
    pub fn play_result(&mut self, won: bool) -> ActionResult {
        self.available()?;
        self.set_mood(self.mood() + if won { 2 } else { 1 });
        self.weight = self.weight.saturating_sub(1).max(WEIGHT_MIN);
        Ok(())
    }

    /// Re-anchor a meter at `level`. Clearing the credited miss count is what
    /// makes answering a call stop the bleeding.
    fn set_hunger(&mut self, level: u8) {
        self.hunger_at_anchor = level.min(METER_MAX);
        self.hunger_anchor_ms = self.awake();
        self.hunger_misses = 0;
    }

    fn set_mood(&mut self, level: u8) {
        self.mood_at_anchor = level.min(METER_MAX);
        self.mood_anchor_ms = self.awake();
        self.mood_misses = 0;
    }

    fn add_weight(&mut self, grams: u16) { self.weight = (self.weight + grams).min(WEIGHT_MAX); }
}

/// A meter's current level: one pip per elapsed period, floored at zero.
fn meter(at_anchor: u8, elapsed_ms: u64, period_ms: u64) -> u8 {
    let lost = (elapsed_ms / period_ms).min(METER_MAX as u64) as u8;
    at_anchor.saturating_sub(lost)
}

/// Grace periods elapsed since `since`. Zero until the first one is up.
fn overdue(awake_ms: u64, since_ms: u64) -> u8 {
    (awake_ms.saturating_sub(since_ms) / ALERT_GRACE_MS).min(u8::MAX as u64) as u8
}
