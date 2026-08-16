//! The game's rules, with nothing Xous-shaped in them.
//!
//! Drawing, input and the Xous API live one level up in `dc34-virtual-pet`.
//! Keeping them out of here is what makes `cargo test` work on the host, and
//! what makes "leave it alone for three days and see what happens" a test that
//! runs in microseconds.
//!
//! The layering is:
//!
//! * [`time`] -- uptime and the pet's awake clock. Pure functions.
//! * [`pet`] -- one creature's needs, mess, illness and death.
//! * [`Game`] -- the lineage: which generation, and what it inherited.
//! * [`GameState`] -- the host clock adapter. The only part that knows about
//!   milliseconds handed in by someone else.

#![cfg_attr(not(test), no_std)]

pub mod pet;
pub mod save;
pub mod time;

pub use pet::{
    ActionResult, Outcome, Pet, Refusal, Stage, CARE_MISS_LIMIT, DISCIPLINE_MAX, LIFESPAN_MS,
    METER_MAX, POOP_MAX,
};
pub use save::{SAVE_LEN};
pub use time::{game_time, is_asleep, GameTime, TimeOfDay, UPTIME_MS_PER_GAME_DAY};

/// Debug accelerator. At 1 the game runs in real time, which puts a full
/// lifespan a day away; at 60 it takes 24 minutes, which is short enough to
/// watch a generation end on real hardware.
pub const TIME_SCALE: u64 = 60;

/// The lineage. Pets come and go; the generation counter and what it passes on
/// are what survive them.
#[derive(Debug, Clone)]
pub struct Game {
    /// Cumulative uptime, the only clock this game believes in.
    uptime_ms: u64,
    /// Uptime at which the current pet's egg was laid.
    gen_start_ms: u64,
    generation: u32,
    /// Discipline handed down by the previous pet.
    inherited: u8,
    pet: Pet,
}

impl Default for Game {
    fn default() -> Self { Self::new() }
}

impl Game {
    pub fn new() -> Self {
        Self { uptime_ms: 0, gen_start_ms: 0, generation: 1, inherited: 0, pet: Pet::new(0) }
    }

    /// Move the simulation up to `uptime_ms`. Never runs backwards, so a host
    /// clock that resets does not un-raise the pet.
    pub fn advance_to(&mut self, uptime_ms: u64) {
        self.uptime_ms = uptime_ms.max(self.uptime_ms);
        self.pet.advance_to(self.uptime_ms - self.gen_start_ms);
    }

    pub fn uptime_ms(&self) -> u64 { self.uptime_ms }
    pub fn gen_start_ms(&self) -> u64 { self.gen_start_ms }
    pub fn generation(&self) -> u32 { self.generation }
    pub fn inherited(&self) -> u8 { self.inherited }

    /// Rebuild from a save file. Only [`save`] should be calling this.
    pub fn restore(uptime_ms: u64, gen_start_ms: u64, generation: u32, inherited: u8, pet: Pet) -> Self {
        Self { uptime_ms, gen_start_ms, generation, inherited, pet }
    }

    /// Serialise for the host to store.
    pub fn to_bytes(&self) -> [u8; SAVE_LEN] { save::to_bytes(self) }

    /// Read back what [`Game::to_bytes`] wrote. `None` if it is not a save file
    /// this build understands.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> { save::from_bytes(bytes) }
    pub fn pet(&self) -> &Pet { &self.pet }
    pub fn pet_mut(&mut self) -> &mut Pet { &mut self.pet }

    /// Everything a screen needs, in one read.
    pub fn snapshot(&self) -> Snapshot {
        let p = &self.pet;
        Snapshot {
            generation: self.generation,
            time: game_time(p.age_ms()),
            part_of_day: TimeOfDay::at(game_time(p.age_ms()).hour),
            age_ms: p.age_ms(),
            stage: p.stage(),
            hunger: p.hunger(),
            mood: p.mood(),
            poop: p.poop(),
            sick: p.sick(),
            asleep: p.asleep(),
            alert: p.alert(),
            weight: p.weight(),
            discipline: p.discipline(),
            care_miss: p.care_miss(),
            outcome: p.outcome(),
        }
    }

    /// Lay the next egg. A pet that lived its full span passes its discipline
    /// on; one that died of neglect takes the lineage down with it.
    ///
    /// Does nothing while the current pet is still alive, so the host can wire
    /// it straight to the button on the farewell screen.
    pub fn next_generation(&mut self) {
        match self.pet.outcome() {
            Some(Outcome::Lifespan) => {
                self.generation += 1;
                self.inherited = self.pet.discipline();
            }
            Some(Outcome::CareFailure) => {
                self.generation = 1;
                self.inherited = 0;
            }
            None => return,
        }
        self.gen_start_ms = self.uptime_ms;
        self.pet = Pet::new(self.inherited);
    }
}

/// A consistent read of the whole game, for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub generation: u32,
    pub time: GameTime,
    pub part_of_day: TimeOfDay,
    pub age_ms: u64,
    pub stage: Stage,
    pub hunger: u8,
    pub mood: u8,
    pub poop: u8,
    pub sick: bool,
    pub asleep: bool,
    pub alert: bool,
    pub weight: u16,
    pub discipline: u8,
    pub care_miss: u8,
    pub outcome: Option<Outcome>,
}

/// Adapter between the host's millisecond clock and the game's uptime.
///
/// The host clock restarts whenever the badge does and has no idea the game
/// exists, so this banks the uptime accumulated so far every time the game is
/// entered. Leaving the game and coming back therefore costs the pet nothing,
/// which is the behaviour the design wants -- though it is still lost on a power
/// cycle until the save file lands.
#[derive(Debug, Clone)]
pub struct GameState {
    /// Uptime banked from earlier visits.
    banked_ms: u64,
    /// Host clock at the most recent [`GameState::start`].
    start_ms: u64,
    /// Most recent host clock reading.
    now_ms: u64,
    game: Game,
}

impl Default for GameState {
    fn default() -> Self { Self::new() }
}

impl GameState {
    pub fn new() -> Self {
        Self { banked_ms: 0, start_ms: 0, now_ms: 0, game: Game::new() }
    }

    /// Adopt a game read back from a save file, and bank its uptime so the
    /// clock carries on from there rather than from zero.
    pub fn resume(&mut self, game: Game) {
        self.banked_ms = game.uptime_ms();
        self.game = game;
    }

    /// Re-take the time reference. Called every time the game is entered; the
    /// pet keeps the age it already had.
    pub fn start(&mut self, now_ms: u64) {
        self.banked_ms = self.uptime_ms();
        self.start_ms = now_ms;
        self.now_ms = now_ms;
    }

    /// Advance to a new host clock reading. A reading older than the reference
    /// is clamped: from the game's side the clock only ever goes forward.
    pub fn tick(&mut self, now_ms: u64) {
        self.now_ms = now_ms.max(self.start_ms);
        let uptime = self.uptime_ms();
        self.game.advance_to(uptime);
    }

    /// Cumulative uptime, with [`TIME_SCALE`] applied.
    pub fn uptime_ms(&self) -> u64 {
        self.banked_ms + self.now_ms.saturating_sub(self.start_ms).saturating_mul(TIME_SCALE)
    }

    /// Host milliseconds since the game was last entered.
    pub fn elapsed_ms(&self) -> u64 { self.now_ms.saturating_sub(self.start_ms) }

    /// In-game time of the current pet.
    pub fn game_time(&self) -> GameTime { game_time(self.game.pet().age_ms()) }

    pub fn game(&self) -> &Game { &self.game }
    pub fn game_mut(&mut self) -> &mut Game { &mut self.game }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60 * 1000;
    const HOUR: u64 = 60 * MIN;

    /// Age at which the egg hatches.
    const HATCH: u64 = 10 * MIN;
    /// First night, in age terms: in-game 00:00 is 16 in-game hours after birth.
    const NIGHT_START: u64 = UPTIME_MS_PER_GAME_DAY * 2 / 3;
    const NIGHT_END: u64 = NIGHT_START + UPTIME_MS_PER_GAME_DAY * time::WAKE_HOUR / 24;

    /// A pet advanced straight to `age`, untouched.
    fn neglected(age: u64) -> Game {
        let mut g = Game::new();
        g.advance_to(age);
        g
    }

    /// The age at which the pet's awake clock reaches `target`. Needs run on
    /// awake time and nights are not short, so the two diverge quickly; tests
    /// that care about a meter have to say which clock they mean.
    fn age_at_awake(target: u64) -> u64 {
        let (mut lo, mut hi) = (0u64, 48 * HOUR);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if time::awake_ms(mid) >= target {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        lo
    }

    /// Advance minute by minute, running `each` at every step. Returns the game
    /// so the caller can look at how it ended.
    fn simulate(until: u64, mut each: impl FnMut(&mut Game)) -> Game {
        let mut g = Game::new();
        for age in (0..until).step_by(MIN as usize) {
            g.advance_to(age);
            each(&mut g);
        }
        g.advance_to(until);
        each(&mut g);
        g
    }

    /// A player who does nothing but sweep, so that only the meter under test
    /// can be the thing calling.
    fn swept(until: u64) -> Game {
        simulate(until, |g| {
            g.pet_mut().clean().ok();
        })
    }

    // -- Growing up -----------------------------------------------------------

    #[test]
    fn an_egg_hatches_after_ten_minutes() {
        assert_eq!(neglected(0).pet().stage(), Stage::Egg);
        assert_eq!(neglected(HATCH - 1).pet().stage(), Stage::Egg);
        assert_eq!(neglected(HATCH).pet().stage(), Stage::Baby);
    }

    #[test]
    fn the_stages_follow_the_spec() {
        // Straight off the age axis: a neglected pet never reaches adulthood.
        assert_eq!(Stage::at(2 * HOUR), Stage::Child);
        assert_eq!(Stage::at(8 * HOUR), Stage::Teen);
        assert_eq!(Stage::at(14 * HOUR), Stage::Adult);
        assert_eq!(Stage::at(LIFESPAN_MS), Stage::Adult);
        // Every stage after the first two runs the same length while the
        // shortened schedule is in place.
        assert_eq!(LIFESPAN_MS - 14 * HOUR, 14 * HOUR - 8 * HOUR);
        assert_eq!(14 * HOUR - 8 * HOUR, 8 * HOUR - 2 * HOUR);
    }

    #[test]
    fn an_egg_has_no_needs_yet() {
        let g = neglected(HATCH - 1);
        assert_eq!(g.pet().hunger(), METER_MAX);
        assert_eq!(g.pet().mood(), METER_MAX);
        assert_eq!(g.pet().poop(), 0);
        assert!(!g.pet().alert());
    }

    // -- Needs ----------------------------------------------------------------

    #[test]
    fn hunger_falls_one_pip_every_thirty_awake_minutes() {
        assert_eq!(neglected(HATCH).pet().hunger(), 4);
        assert_eq!(neglected(HATCH + 30 * MIN - 1).pet().hunger(), 4);
        assert_eq!(neglected(HATCH + 30 * MIN).pet().hunger(), 3);
        assert_eq!(neglected(HATCH + 60 * MIN).pet().hunger(), 2);
    }

    #[test]
    fn needs_hold_still_overnight() {
        let dusk = neglected(NIGHT_START).pet().hunger();
        // A whole night passes and the meter has not moved.
        assert_eq!(neglected(NIGHT_END - 1).pet().hunger(), dusk);
        // It resumes from where it stopped, not from the wall clock.
        assert_eq!(neglected(NIGHT_END + 30 * MIN).pet().hunger(), dusk - 1);
    }

    #[test]
    fn feeding_refills_one_pip_and_adds_a_gram() {
        let mut g = neglected(HATCH + 60 * MIN);
        assert_eq!(g.pet().hunger(), 2);
        let before = g.pet().weight();
        assert_eq!(g.pet_mut().feed_meal(), Ok(()));
        assert_eq!(g.pet().hunger(), 3);
        assert_eq!(g.pet().weight(), before + 1);
    }

    #[test]
    fn a_full_pet_turns_down_food() {
        let mut g = neglected(HATCH);
        assert_eq!(g.pet_mut().feed_meal(), Err(Refusal::Full));
        assert_eq!(g.pet_mut().feed_snack(), Err(Refusal::Full));
    }

    #[test]
    fn snacks_buy_mood_with_weight() {
        let mut g = neglected(HATCH + 50 * MIN);
        assert_eq!(g.pet().mood(), 3);
        let before = g.pet().weight();
        assert_eq!(g.pet_mut().feed_snack(), Ok(()));
        assert_eq!(g.pet().mood(), 4);
        assert_eq!(g.pet().weight(), before + 2);
    }

    #[test]
    fn playing_cheers_it_up_and_burns_a_gram() {
        let mut g = neglected(HATCH + 100 * MIN);
        let before = g.pet().mood();
        g.pet_mut().feed_meal().ok();
        let weight = g.pet().weight();
        assert_eq!(g.pet_mut().play_result(true), Ok(()));
        assert_eq!(g.pet().mood(), (before + 2).min(METER_MAX));
        assert_eq!(g.pet().weight(), weight - 1);
    }

    // -- Mess and illness -----------------------------------------------------

    #[test]
    fn the_young_make_a_mess_every_thirty_minutes() {
        assert_eq!(neglected(HATCH + 29 * MIN).pet().poop(), 0);
        assert_eq!(neglected(HATCH + 30 * MIN).pet().poop(), 1);
        assert_eq!(neglected(HATCH + 60 * MIN).pet().poop(), 2);
    }

    #[test]
    fn adults_make_a_mess_half_as_often() {
        // Timed between two droppings rather than from adulthood: how far along
        // the current one is at any given moment is not observable, so an
        // absolute deadline would be measuring the wrong thing.
        let mut g = simulate(16 * HOUR, attentive);
        assert_eq!(g.pet().stage(), Stage::Adult);

        let first = advance_until_poop(&mut g);
        g.pet_mut().clean().ok();
        let second = advance_until_poop(&mut g);

        let gap = time::awake_ms(second) - time::awake_ms(first);
        assert!(
            (59 * MIN..=61 * MIN).contains(&gap),
            "adult droppings came {} minutes apart, not 60",
            gap / MIN
        );
    }

    /// Run until the pen is dirty, feeding but not sweeping. Returns the age at
    /// which the dropping showed up, to the minute.
    fn advance_until_poop(g: &mut Game) -> u64 {
        let mut age = g.pet().age_ms();
        while g.pet().poop() == 0 {
            age += MIN;
            g.advance_to(age);
            let p = g.pet_mut();
            if p.hunger() == 0 {
                while p.feed_meal().is_ok() {}
            }
            if p.mood() == 0 {
                while p.feed_snack().is_ok() {}
            }
        }
        age
    }

    #[test]
    fn an_unswept_pen_stops_filling_at_the_edge_of_the_screen() {
        assert_eq!(neglected(6 * HOUR).pet().poop(), POOP_MAX);
    }

    #[test]
    fn sweeping_clears_the_pen() {
        let mut g = neglected(HATCH + 60 * MIN);
        assert_eq!(g.pet().poop(), 2);
        assert_eq!(g.pet_mut().clean(), Ok(()));
        assert_eq!(g.pet().poop(), 0);
        assert_eq!(g.pet_mut().clean(), Err(Refusal::Nothing));
    }

    #[test]
    fn living_in_filth_makes_it_ill() {
        let mut g = neglected(HATCH + 60 * MIN);
        assert!(!g.pet().sick());
        g.advance_to(age_at_awake(100 * MIN));
        assert_eq!(g.pet().poop(), 3);
        assert!(g.pet().sick());
        // Medicine cures it; sweeping alone does not.
        assert_eq!(g.pet_mut().clean(), Ok(()));
        assert!(g.pet().sick());
        assert_eq!(g.pet_mut().medicate(), Ok(()));
        assert!(!g.pet().sick());
        assert_eq!(g.pet_mut().medicate(), Err(Refusal::Healthy));
    }

    #[test]
    fn illness_is_dated_from_the_dropping_that_caused_it() {
        // Whether we watch it happen or come back hours later, the pet fell ill
        // at the same moment -- so it has been calling for the same length of
        // time, and owes the same number of care misses.
        let watched = simulate(6 * HOUR, |_| {});
        let mut ignored = Game::new();
        ignored.advance_to(6 * HOUR);
        assert_eq!(watched.snapshot(), ignored.snapshot());
    }

    // -- Care misses ----------------------------------------------------------

    #[test]
    fn an_unanswered_call_costs_one_miss_per_grace_period() {
        // Hunger bottoms out four 30-minute pips after the hatch, which is at
        // 10 awake minutes.
        // Swept throughout, so hunger is the only thing calling.
        let empty = 10 * MIN + 4 * 30 * MIN;
        let g = swept(age_at_awake(empty));
        assert_eq!(g.pet().hunger(), 0);
        assert!(g.pet().alert());
        assert_eq!(g.pet().care_miss(), 0, "the grace period has not run out yet");

        assert_eq!(swept(age_at_awake(empty + 25 * MIN)).pet().care_miss(), 1);
        assert_eq!(swept(age_at_awake(empty + 50 * MIN)).pet().care_miss(), 2);
    }

    #[test]
    fn answering_the_call_stops_the_bleeding() {
        let mut g = Game::new();
        g.advance_to(4 * HOUR);
        let misses = g.pet().care_miss();
        assert!(misses > 0, "four hours of neglect should have cost something");

        g.pet_mut().feed_meal().ok();
        g.pet_mut().feed_snack().ok();
        g.pet_mut().clean().ok();
        g.pet_mut().medicate().ok();
        assert!(!g.pet().alert());

        // An hour later the count has not moved: nothing is calling.
        g.advance_to(5 * HOUR);
        assert_eq!(g.pet().care_miss(), misses);
    }

    #[test]
    fn neglect_kills_and_the_lineage_resets() {
        let g = neglected(24 * HOUR);
        assert_eq!(g.pet().outcome(), Some(Outcome::CareFailure));
        assert!(g.pet().age_ms() < 24 * HOUR, "it died long before its time");

        let mut g = g;
        g.next_generation();
        assert_eq!(g.generation(), 1);
        assert_eq!(g.pet().stage(), Stage::Egg);
    }

    #[test]
    fn the_time_of_death_does_not_depend_on_when_we_look() {
        let watched = simulate(24 * HOUR, |_| {});
        let ignored = neglected(24 * HOUR);
        assert_eq!(watched.snapshot(), ignored.snapshot());
        assert_eq!(watched.pet().outcome(), Some(Outcome::CareFailure));
    }

    #[test]
    fn scolding_counts_once_per_call() {
        let mut g = Game::new();
        g.advance_to(HATCH + 2 * HOUR);
        assert!(g.pet().alert());
        assert_eq!(g.pet_mut().scold(), Ok(()));
        assert_eq!(g.pet().discipline(), 1);
        assert_eq!(g.pet_mut().scold(), Err(Refusal::Nothing), "no free points");
        assert_eq!(g.pet().discipline(), 1);
    }

    // -- A life, well and badly lived -----------------------------------------

    /// Answer everything the moment it comes up. Meals wait until the meter is
    /// actually empty -- a player who tops it up continuously would never see
    /// the pet call, and the call is what a scolding needs.
    fn attentive(g: &mut Game) {
        let p = g.pet_mut();
        p.clean().ok();
        p.medicate().ok();
        p.scold().ok();
        if p.hunger() == 0 {
            while p.feed_meal().is_ok() {}
        }
        if p.mood() == 0 {
            while p.feed_snack().is_ok() {}
        }
    }

    #[test]
    fn a_well_raised_pet_lives_its_full_span() {
        let g = simulate(25 * HOUR, attentive);
        assert_eq!(g.pet().outcome(), Some(Outcome::Lifespan));
        assert_eq!(g.pet().care_miss(), 0);
        assert_eq!(g.pet().age_ms(), LIFESPAN_MS);
    }

    #[test]
    fn a_full_span_passes_the_lineage_on() {
        let mut g = simulate(25 * HOUR, attentive);
        let discipline = g.pet().discipline();
        assert!(discipline > 0, "there was something to scold about");

        g.next_generation();
        assert_eq!(g.generation(), 2);
        assert_eq!(g.pet().stage(), Stage::Egg);

        // The inherited pet gets hungry more slowly than its parent did.
        let mut fresh = Game::new();
        let probe = g.uptime_ms() + HATCH + 30 * MIN;
        fresh.advance_to(HATCH + 30 * MIN);
        g.advance_to(probe);
        assert!(g.pet().hunger() > fresh.pet().hunger());
    }

    #[test]
    fn a_dead_pet_stops_ageing() {
        let mut g = neglected(24 * HOUR);
        let died_at = g.pet().age_ms();
        g.advance_to(48 * HOUR);
        assert_eq!(g.pet().age_ms(), died_at);
    }

    // -- Actions the pet will not take ----------------------------------------

    #[test]
    fn an_egg_and_a_sleeping_pet_take_no_orders() {
        let mut g = neglected(0);
        assert_eq!(g.pet_mut().feed_meal(), Err(Refusal::Unhatched));

        let mut g = Game::new();
        g.advance_to(NIGHT_START);
        assert!(g.pet().asleep());
        assert_eq!(g.pet_mut().feed_meal(), Err(Refusal::Asleep));
        assert!(!g.pet().alert(), "a sleeping pet is not calling for anything");
    }

    // -- The property the whole design rests on -------------------------------

    #[test]
    fn the_result_does_not_depend_on_how_often_we_tick() {
        for step in [MIN, 7 * MIN, 25 * MIN, 3 * HOUR] {
            let mut stepped = Game::new();
            let mut age = 0;
            while age < 20 * HOUR {
                age += step;
                stepped.advance_to(age);
            }
            let mut jumped = Game::new();
            jumped.advance_to(age);
            assert_eq!(
                stepped.snapshot(),
                jumped.snapshot(),
                "a {}-minute tick changed the outcome",
                step / MIN
            );
        }
    }

    // -- Saving ---------------------------------------------------------------

    #[test]
    fn a_saved_game_comes_back_the_same() {
        // A pet with something in every field: fed, played with, ill, scolded,
        // and carrying care misses.
        let mut g = simulate(5 * HOUR, |g| {
            let p = g.pet_mut();
            p.scold().ok();
            if p.hunger() == 0 {
                p.feed_meal().ok();
            }
        });
        g.pet_mut().feed_snack().ok();
        assert!(g.pet().care_miss() > 0 && g.pet().poop() > 0);

        let bytes = g.to_bytes();
        let back = Game::from_bytes(&bytes).expect("should read its own output");
        assert_eq!(g.snapshot(), back.snapshot());
        assert_eq!(g.uptime_ms(), back.uptime_ms());

        // And it carries on from where it left off, rather than from a guess.
        let mut a = g;
        let mut b = back;
        a.advance_to(a.uptime_ms() + 3 * HOUR);
        b.advance_to(b.uptime_ms() + 3 * HOUR);
        assert_eq!(a.snapshot(), b.snapshot());
    }

    #[test]
    fn a_saved_lineage_keeps_what_it_inherited() {
        let mut g = simulate(21 * HOUR, attentive);
        assert_eq!(g.pet().outcome(), Some(Outcome::Lifespan));
        g.next_generation();
        assert_eq!(g.generation(), 2);

        let back = Game::from_bytes(&g.to_bytes()).unwrap();
        assert_eq!(back.generation(), 2);

        // The inherited slow-down is not stored, it is rebuilt -- so this is
        // what proves it was rebuilt correctly.
        let probe = g.uptime_ms() + HATCH + 30 * MIN;
        let mut a = g;
        let mut b = back;
        a.advance_to(probe);
        b.advance_to(probe);
        assert_eq!(a.snapshot(), b.snapshot());
        assert!(a.pet().hunger() > neglected(HATCH + 30 * MIN).pet().hunger());
    }

    #[test]
    fn rubbish_is_not_a_save_file() {
        assert!(Game::from_bytes(&[]).is_none());
        assert!(Game::from_bytes(&[0u8; SAVE_LEN]).is_none(), "version 0 is not this format");
        let mut bytes = Game::new().to_bytes();
        assert!(Game::from_bytes(&bytes).is_some());
        // A pet older than the lineage it belongs to cannot have happened.
        bytes[9..17].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(Game::from_bytes(&bytes).is_none());
    }

    #[test]
    fn the_host_clock_banks_uptime_across_visits() {
        let mut s = GameState::new();
        s.start(1_000);
        s.tick(1_000 + 60_000); // one host minute
        let after_first = s.uptime_ms();
        assert_eq!(after_first, 60_000 * TIME_SCALE);

        // Leave the game and come back on a clock that has moved on.
        s.start(5_000_000);
        assert_eq!(s.uptime_ms(), after_first, "the pet did not age while away");
        s.tick(5_000_000 + 60_000);
        assert_eq!(s.uptime_ms(), after_first * 2);
    }
}
