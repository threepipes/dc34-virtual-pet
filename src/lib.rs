//! A game that plugs into the DC34 badge firmware.
//!
//! The host (`dc34-vault`) knows only [`BadgeGame`] and [`GameAction`]: it owns
//! the screen, the buttons and the clock, and hands all three to whatever game
//! it was built against. Swapping in a different game is a one-line change to
//! the host's `Cargo.toml`.
//!
//! The rules live in `dc34-virtual-pet-core`, which knows nothing about Xous.
//! What is here is the part that cannot be tested off-device: which screen is
//! up, which button does what, and how it all looks.

mod draw;
mod minigame;

use blitstr2::GlyphStyle;
use dc34_virtual_pet_core::{GameState, Outcome, Refusal, Snapshot, Stage};
use draw::Face;
use minigame::MiniGame;
use ux_api::service::gfx::Gfx;

// -- Buttons ------------------------------------------------------------------
// The three front buttons, in physical left-to-right order, plus the jog press.
// These codes are the same in hosted mode and on hardware, so there is no
// per-target branching here.

/// Left button: move the cursor.
const KEY_SELECT: char = '←';
/// Center button: act on the current selection.
const KEY_OK: char = '🔥';
/// Right button: back out.
const KEY_CANCEL: char = '→';
/// Jog press. The only way back to the badge menu: it sits away from the three
/// front buttons, so it is hard to hit by accident, and it matches what the
/// official app uses the key for.
const KEY_MENU: char = '∴';

/// How long a reaction stays on screen.
const MESSAGE_MS: u64 = 1500;

// -- On-screen text -----------------------------------------------------------
// Everything the player reads is ASCII, not the Japanese of docs/UI.md. The ja
// font is compiled into the image, but blitstr2's `english_rules` for baosec
// falls back to zh only and never consults it (libs/blitstr2/src/style_macros.rs
// ~152), so kana come out as replacement glyphs. Selecting the Japanese rules
// does not help either: `style_glyph` matches the locale against "jp" while
// locales sets LANG to "ja", so `lang-ja` still lands in the English rules.
// Fixing either would mean patching xous-core, which this project keeps stock.

/// The main screen's icon bar, in cursor order.
const MENU: [&str; 5] = ["FD", "PL", "CL", "MD", "SC"];
/// What each icon means, shown while it is selected.
const MENU_NAMES: [&str; 5] = ["FEED", "PLAY", "CLEAN", "MEDICINE", "SCOLD"];

const MENU_FEED: usize = 0;
const MENU_PLAY: usize = 1;
const MENU_CLEAN: usize = 2;
const MENU_MEDICINE: usize = 3;
const MENU_SCOLD: usize = 4;

const FEED_ITEMS: [&str; 2] = ["MEAL", "SNACK"];

/// What the host should do after handing a key to the game.
pub enum GameAction {
    /// ゲームを続ける
    Continue,
    /// ゲームを抜けて呼び出し元のモードへ戻る
    Exit,
}

/// The contract between the badge firmware and a game.
pub trait BadgeGame {
    /// 起動時に呼ばれる。時間の基準を取る用
    fn start(&mut self, now_ms: u64);
    /// 定期的に呼ばれる。状態を進める
    fn tick(&mut self, now_ms: u64);
    /// キー入力。使わなかったキーは無視してよい
    fn key(&mut self, k: char) -> GameAction;
    /// 画面全体を描く。ホストが直後に flush するので、ここでは不要
    fn draw(&self, gfx: &Gfx);
}

/// Build the game this crate provides. The host calls this and then only ever
/// talks to the trait, so it never names a concrete game type.
pub fn new_game() -> Box<dyn BadgeGame> { Box::new(VirtualPet::new()) }

/// Which screen is up. `docs/UI.md` §4 is the transition diagram this follows.
enum Screen {
    Main,
    Feed(usize),
    Status,
    Play(MiniGame),
    /// A stage was reached. Blocks until acknowledged so it cannot be missed.
    Evolved,
    /// The pet's life ended, one way or the other.
    Ended(Outcome),
}

pub struct VirtualPet {
    state: GameState,
    screen: Screen,
    /// Position on the main screen's icon bar. Kept out of `Screen` so that a
    /// trip through the status screen comes back to the same icon.
    cursor: usize,
    /// Stage as of the last frame, to notice evolution.
    last_stage: Stage,
    /// Text to show at the bottom of the field, and when it disappears.
    message: Option<(String, u64)>,
    /// Latest clock reading, so that `key()` can set a deadline without being
    /// handed the time itself. `tick()` runs often enough for this to be exact
    /// to within a frame.
    now_ms: u64,
}

impl Default for VirtualPet {
    fn default() -> Self { Self::new() }
}

impl VirtualPet {
    pub fn new() -> Self {
        Self {
            state: GameState::new(),
            screen: Screen::Main,
            cursor: 0,
            last_stage: Stage::Egg,
            message: None,
            now_ms: 0,
        }
    }

    fn snapshot(&self) -> Snapshot { self.state.game().snapshot() }

    fn say(&mut self, text: &str) {
        self.message = Some((text.to_string(), self.now_ms + MESSAGE_MS));
    }

    /// Report an action the pet either did or declined to do.
    fn report(&mut self, result: Result<(), Refusal>, done: &str) {
        self.say(match result {
            Ok(()) => done,
            Err(Refusal::Unhatched) => "not hatched yet",
            Err(Refusal::Asleep) => "shh, sleeping",
            Err(Refusal::Full) => "already full",
            Err(Refusal::Healthy) => "feeling fine!",
            Err(Refusal::Nothing) => "nothing to do",
        });
    }

    /// Act on the main screen's icon bar.
    fn run_menu_item(&mut self, item: usize) {
        match item {
            MENU_FEED => self.screen = Screen::Feed(0),
            MENU_PLAY => {
                if self.snapshot().asleep {
                    self.say("shh, sleeping");
                } else {
                    self.screen = Screen::Play(MiniGame::new(self.now_ms));
                }
            }
            MENU_CLEAN => {
                let r = self.state.game_mut().pet_mut().clean();
                self.report(r, "all clean!");
            }
            MENU_MEDICINE => {
                let r = self.state.game_mut().pet_mut().medicate();
                self.report(r, "all better!");
            }
            MENU_SCOLD => {
                let r = self.state.game_mut().pet_mut().scold();
                self.report(r, "told off");
            }
            _ => {}
        }
    }

    /// A finished round of the guessing game. Winning the series is worth more.
    fn finish_minigame(&mut self, wins: u32) {
        let won = wins >= 3;
        self.state.game_mut().pet_mut().play_result(won).ok();
        self.say(if won { "that was fun!" } else { "not bad" });
        self.screen = Screen::Main;
    }

    /// The face for the pet's current state, most urgent first.
    fn face(&self, s: &Snapshot) -> Face {
        if s.outcome.is_some() {
            Face::Dead
        } else if s.asleep {
            Face::Asleep
        } else if s.sick {
            Face::Sick
        } else if s.alert {
            Face::Troubled
        } else if s.hunger >= 3 && s.mood >= 3 {
            Face::Happy
        } else {
            Face::Normal
        }
    }
}

impl BadgeGame for VirtualPet {
    fn start(&mut self, now_ms: u64) {
        // The clock is re-anchored but the pet is not: `GameState` banks the
        // uptime, so leaving the game and coming back costs it nothing.
        self.state.start(now_ms);
        self.now_ms = now_ms;
        self.screen = Screen::Main;
        self.cursor = 0;
        self.message = None;
        self.last_stage = self.snapshot().stage;
    }

    fn tick(&mut self, now_ms: u64) {
        self.state.tick(now_ms);
        self.now_ms = now_ms;

        if let Some((_, deadline)) = self.message {
            if now_ms >= deadline {
                self.message = None;
            }
        }

        // Growing up and dying interrupt whatever is on screen: both are one-off
        // events, and a submenu is not worth missing them for.
        let s = self.snapshot();
        if let Some(outcome) = s.outcome {
            if !matches!(self.screen, Screen::Ended(_)) {
                self.screen = Screen::Ended(outcome);
                self.message = None;
            }
        } else if s.stage != self.last_stage {
            self.screen = Screen::Evolved;
            self.message = None;
        }
        self.last_stage = s.stage;
    }

    fn key(&mut self, k: char) -> GameAction {
        // The jog press leaves the game, from any screen. Handled before
        // everything else so that there is always a way out, including while the
        // pet is asleep and ignoring the front buttons.
        if k == KEY_MENU {
            return GameAction::Exit;
        }

        let s = self.snapshot();

        // A sleeping pet takes no orders -- `docs/UI.md` §3.7.
        if s.asleep && s.outcome.is_none() && !matches!(self.screen, Screen::Ended(_)) {
            return GameAction::Continue;
        }

        match &mut self.screen {
            Screen::Main => match k {
                KEY_SELECT => self.cursor = (self.cursor + 1) % MENU.len(),
                KEY_OK => self.run_menu_item(self.cursor),
                // Nothing to back out of on the main screen, so Cancel is free
                // to open the status sheet. That is the only way in now that the
                // system submenu is gone.
                KEY_CANCEL => self.screen = Screen::Status,
                _ => {}
            },

            Screen::Feed(cursor) => match k {
                KEY_SELECT => *cursor = (*cursor + 1) % FEED_ITEMS.len(),
                KEY_OK => {
                    let pick = *cursor;
                    let pet = self.state.game_mut().pet_mut();
                    let (r, done) = if pick == 0 {
                        (pet.feed_meal(), "munch munch")
                    } else {
                        (pet.feed_snack(), "yum!")
                    };
                    self.report(r, done);
                    self.screen = Screen::Main;
                }
                KEY_CANCEL => self.screen = Screen::Main,
                _ => {}
            },

            Screen::Status => {
                if k == KEY_OK || k == KEY_CANCEL {
                    self.screen = Screen::Main;
                }
            }

            // The one screen where the outer buttons are neither select nor
            // cancel: pointing left and right is the whole game, so they are the
            // answer, and pressing one plays the round outright. That leaves the
            // center button free to be the way out.
            Screen::Play(game) => match k {
                KEY_SELECT | KEY_CANCEL => {
                    let side = if k == KEY_SELECT { 0 } else { 1 };
                    if let Some(wins) = game.answer(side) {
                        self.finish_minigame(wins);
                    }
                }
                KEY_OK => self.screen = Screen::Main,
                _ => {}
            },

            Screen::Evolved => {
                if k == KEY_OK {
                    self.screen = Screen::Main;
                }
            }

            Screen::Ended(_) => {
                if k == KEY_OK {
                    self.state.game_mut().next_generation();
                    self.last_stage = self.snapshot().stage;
                    self.screen = Screen::Main;
                }
            }
        }
        GameAction::Continue
    }

    /// Paint the whole screen. Cheap enough at 128x128 that partial redraws
    /// would be premature; if flicker shows up, switch to repainting only on
    /// changes.
    ///
    /// No flush here: the host flushes once per frame for every mode, and doing
    /// it twice would push the whole framebuffer over IPC for nothing.
    fn draw(&self, gfx: &Gfx) {
        gfx.clear().ok();
        let s = self.snapshot();
        let face = self.face(&s);

        match &self.screen {
            Screen::Ended(outcome) => {
                draw::creature(gfx, s.stage, Face::Dead);
                let (title, detail) = match outcome {
                    Outcome::Lifespan => ("A FULL LIFE", "traits passed on"),
                    Outcome::CareFailure => ("IT DIDN'T MAKE IT", "back to gen 1"),
                };
                draw::line(gfx, 2, 16, GlyphStyle::Bold, title);
                draw::line(gfx, 90, 16, GlyphStyle::Small, detail);
                draw::legend(gfx, "^ continue");
                return;
            }

            Screen::Evolved => {
                draw::creature(gfx, s.stage, Face::Happy);
                let title = if s.stage == Stage::Baby { "HATCHED!" } else { "IT GREW!" };
                draw::line(gfx, 2, 16, GlyphStyle::Bold, title);
                draw::legend(gfx, "^ continue");
                return;
            }

            Screen::Status => {
                draw::line(gfx, 2, 16, GlyphStyle::Bold, "STATUS");
                let rows = [
                    format!("gen        {}", s.generation),
                    format!("age        day {}", s.time.day),
                    format!("weight     {} g", s.weight),
                    format!("discipline {} / 4", s.discipline),
                    format!("misses     {}", s.care_miss),
                ];
                for (i, row) in rows.iter().enumerate() {
                    draw::line(gfx, 22 + i as isize * 17, 16, GlyphStyle::Small, row);
                }
                draw::legend(gfx, "^ or > to go back");
                return;
            }

            Screen::Play(game) => {
                game.draw(gfx);
                return;
            }

            Screen::Main | Screen::Feed(_) => {}
        }

        // -- the main screen, and the feed submenu drawn over it --------------
        draw::status_bar(gfx, &s);

        if s.asleep {
            // No menu bar while it sleeps: there is nothing to press.
            draw::creature(gfx, s.stage, Face::Asleep);
            draw::legend(gfx, "sleeping...");
            return;
        }

        draw::creature(gfx, s.stage, face);
        draw::mess(gfx, s.poop);

        match &self.screen {
            Screen::Feed(cursor) => {
                draw::list(gfx, &FEED_ITEMS, *cursor);
                draw::legend(gfx, "< sel   ^ ok   > back");
            }
            _ => {
                if s.stage == Stage::Egg {
                    // Nothing can be done for an egg, so it gets no icon bar.
                    draw::legend(gfx, "not long now...");
                } else {
                    draw::menu_bar(gfx, &MENU, Some(self.cursor));
                    // The icons are two letters; the selected one gets its name
                    // spelled out just above the bar.
                    draw::line(
                        gfx,
                        draw::MENU_TOP - 18,
                        16,
                        GlyphStyle::Small,
                        MENU_NAMES[self.cursor],
                    );
                }
            }
        }

        if let Some((text, _)) = &self.message {
            draw::line(gfx, draw::FIELD_TOP + 2, 16, GlyphStyle::Bold, text);
        }
    }
}
