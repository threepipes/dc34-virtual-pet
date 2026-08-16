//! A game that plugs into the DC34 badge firmware.
//!
//! The host (`dc34-vault`) knows only [`BadgeGame`] and [`GameAction`]: it owns
//! the screen, the buttons and the clock, and hands all three to whatever game
//! it was built against. Swapping in a different game is a one-line change to
//! the host's `Cargo.toml`.
//!
//! The game itself is still the PoC screen -- a clock and two boxes. The raising
//! mechanics come later; what is being proven here is the plumbing.

use core::fmt::Write;

use blitstr2::GlyphStyle;
use dc34_virtual_pet_core::GameState;
use ux_api::minigfx::*;
use ux_api::service::api::Gid;
use ux_api::service::gfx::Gfx;

// -- Buttons ------------------------------------------------------------------
// The three front buttons, in physical left-to-right order. These codes are the
// same in hosted mode and on hardware, so there is no per-target branching here.

/// Left button: move the cursor.
const KEY_SELECT: char = '←';
/// Center button: act on the current selection.
const KEY_OK: char = '🔥';
/// Right button: back out.
const KEY_CANCEL: char = '→';

// -- Layout -------------------------------------------------------------------

const SCREEN: isize = 128;
const ITEMS: [&'static str; 2] = ["hello, world!", "end"];

/// Index of the item that leaves the game.
const ITEM_END: usize = 1;

/// Vertical extent of each menu box.
const BOX_TOP: [isize; 2] = [34, 66];
const BOX_HEIGHT: isize = 26;
const BOX_MARGIN_X: isize = 8;
const BOX_RADIUS: isize = 4;

/// How long the "Hello, world!" acknowledgement stays up.
const MESSAGE_MS: u64 = 2000;

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

pub struct VirtualPet {
    state: GameState,
    /// `None` means nothing is selected -- the state Cancel returns you to.
    cursor: Option<usize>,
    /// Text to show at the bottom, and the deadline after which it disappears.
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
        Self { state: GameState::new(), cursor: Some(0), message: None, now_ms: 0 }
    }
}

impl BadgeGame for VirtualPet {
    fn start(&mut self, now_ms: u64) {
        self.state.start(now_ms);
        self.cursor = Some(0);
        self.message = None;
        self.now_ms = now_ms;
    }

    fn tick(&mut self, now_ms: u64) {
        self.state.tick(now_ms);
        self.now_ms = now_ms;
        // Expire the acknowledgement message.
        if let Some((_, deadline)) = self.message {
            if now_ms >= deadline {
                self.message = None;
            }
        }
    }

    fn key(&mut self, k: char) -> GameAction {
        match k {
            KEY_SELECT => {
                self.cursor = Some(match self.cursor {
                    Some(i) => (i + 1) % ITEMS.len(),
                    None => 0,
                });
            }
            // Cancel backs out one step at a time: first it drops the selection,
            // then -- with nothing left to back out of -- it hands the badge back.
            // Without that second step "end" would be the only way out, which is a
            // bad place to be if the menu ever fails to draw.
            KEY_CANCEL => match self.cursor {
                Some(_) => self.cursor = None,
                None => return GameAction::Exit,
            },
            KEY_OK => match self.cursor {
                Some(ITEM_END) => return GameAction::Exit,
                Some(_) => {
                    self.message = Some(("Hello, world!".to_string(), self.now_ms + MESSAGE_MS));
                }
                None => {}
            },
            // The jog wheel is deliberately unused in this design.
            _ => {}
        }
        GameAction::Continue
    }

    /// Paint the whole screen. Cheap enough at 128x128 that partial redraws would
    /// be premature; if flicker shows up, switch to repainting only on changes.
    ///
    /// No flush here: the host flushes once per frame for every mode, and doing it
    /// twice would push the whole framebuffer over IPC for nothing.
    fn draw(&self, gfx: &Gfx) {
        gfx.clear().ok();

        // -- clock ------------------------------------------------------------
        let t = self.state.game_time();
        let mut clock = TextView::new(
            Gid::dummy(),
            TextBounds::BoundingBox(Rectangle::new_coords(0, 2, SCREEN - 1, 20)),
        );
        clock.draw_border = false;
        clock.style = GlyphStyle::Bold;
        write!(clock, "Day {}  {:02}:{:02}", t.day, t.hour, t.minute).ok();
        gfx.draw_textview(&mut clock).ok();

        // -- menu boxes -------------------------------------------------------
        for (i, label) in ITEMS.iter().enumerate() {
            let selected = self.cursor == Some(i);
            let top = BOX_TOP[i];

            // A thicker stroke marks the selection. Inverting the fill would be the
            // other option, but that also inverts the text and reads as "pressed".
            let style =
                DrawStyle::new(PixelColor::Light, PixelColor::Dark, if selected { 2 } else { 1 });
            let border = Rectangle::new_coords_with_style(
                BOX_MARGIN_X,
                top,
                SCREEN - 1 - BOX_MARGIN_X,
                top + BOX_HEIGHT,
                style,
            );
            gfx.draw_rounded_rectangle(RoundedRectangle::new(border, BOX_RADIUS)).ok();

            let mut tv = TextView::new(
                Gid::dummy(),
                TextBounds::BoundingBox(Rectangle::new_coords(
                    BOX_MARGIN_X + 5,
                    top + 4,
                    SCREEN - 1 - BOX_MARGIN_X - 5,
                    top + BOX_HEIGHT - 3,
                )),
            );
            tv.draw_border = false;
            tv.style = GlyphStyle::Regular;
            write!(tv, "{}{}", if selected { "> " } else { "  " }, label).ok();
            gfx.draw_textview(&mut tv).ok();
        }

        // -- bottom line: message, or the button legend -----------------------
        let mut footer = TextView::new(
            Gid::dummy(),
            TextBounds::BoundingBox(Rectangle::new_coords(0, SCREEN - 18, SCREEN - 1, SCREEN - 1)),
        );
        footer.draw_border = false;
        match &self.message {
            Some((text, _)) => {
                footer.style = GlyphStyle::Bold;
                write!(footer, "{}", text).ok();
            }
            None => {
                footer.style = GlyphStyle::Small;
                write!(footer, "< sel   ^ ok   > cancel").ok();
            }
        }
        gfx.draw_textview(&mut footer).ok();
    }
}
