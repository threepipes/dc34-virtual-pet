//! The guessing game from `docs/UI.md` §3.5: five rounds of calling which way
//! the pet will turn.
//!
//! The randomness is a xorshift seeded off the host clock rather than the badge
//! TRNG. Guessing the next turn from the last one is not a threat worth a
//! syscall, and this keeps the crate free of Xous services it does not otherwise
//! need.

use blitstr2::GlyphStyle;
use ux_api::minigfx::*;
use ux_api::service::gfx::Gfx;

use crate::draw;

pub const ROUNDS: u32 = 5;

pub struct MiniGame {
    rng: u32,
    round: u32,
    wins: u32,
    /// 0 = left, 1 = right.
    choice: u32,
    /// The previous round's answer and whether it was called right, if any.
    last: Option<(u32, bool)>,
}

impl MiniGame {
    pub fn new(seed_ms: u64) -> Self {
        // Any non-zero seed will do; xorshift is stuck at zero.
        let seed = (seed_ms as u32) | 1;
        Self { rng: seed, round: 0, wins: 0, choice: 0, last: None }
    }

    fn next_bit(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng & 1
    }

    /// Call the round: point left (0) or right (1). One press is the whole
    /// interaction -- there is nothing to confirm, because there is nothing the
    /// player could have meant other than the side they pressed.
    ///
    /// Returns the number of rounds won once the series is over, and `None`
    /// while it is still going.
    pub fn answer(&mut self, side: u32) -> Option<u32> {
        self.choice = side & 1;
        let answer = self.next_bit();
        let right = answer == self.choice;
        if right {
            self.wins += 1;
        }
        self.last = Some((answer, right));
        self.round += 1;
        if self.round >= ROUNDS { Some(self.wins) } else { None }
    }

    pub fn draw(&self, gfx: &Gfx) {
        let mut header = String::new();
        use core::fmt::Write;
        write!(header, "PLAY  {}/{}", (self.round + 1).min(ROUNDS), ROUNDS).ok();
        draw::line(gfx, 2, 16, GlyphStyle::Bold, &header);

        // The pet leans the way it turned last round, so the reveal is visible
        // without a screen of its own.
        let lean = match self.last {
            Some((0, _)) => -14,
            Some((_, _)) => 14,
            None => 0,
        };
        let centre = Point::new(draw::SCREEN / 2 + lean, 56);
        let style = DrawStyle::new(PixelColor::Light, PixelColor::Dark, 1);
        gfx.draw_circle(Circle::new_with_style(centre, 18, style)).ok();
        for side in [-1, 1] {
            let eye = Point::new(centre.x + side * 6, centre.y - 5);
            gfx.draw_circle(Circle::new_with_style(
                eye,
                2,
                DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1),
            ))
            .ok();
        }

        if let Some((_, right)) = self.last {
            draw::line(gfx, 80, 16, GlyphStyle::Small, if right { "HIT!" } else { "miss" });
        }

        draw::line(gfx, draw::MENU_TOP - 18, 16, GlyphStyle::Regular, "LEFT       RIGHT");
        draw::legend(gfx, "< left   ^ stop   > right");
    }
}
