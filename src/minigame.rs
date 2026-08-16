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
        use core::fmt::Write;

        // The title is eight full-width glyphs at 17 px of advance each, which
        // is wider than the screen. The box is two lines tall so that wordwrap
        // breaks it rather than dropping the tail off the edge.
        draw::wrapped(gfx, 2, 36, GlyphStyle::Bold, "あっちむいてホイ");
        draw::line(gfx, 38, 17, GlyphStyle::Regular, "ひだり？みぎ？");

        // The hands say which button does what, next to the pet that is about
        // to turn one way or the other. A written-out legend does not fit the
        // width, and this needs no reading.
        draw::icon(gfx, 8, PET_Y - 8, "👈");
        draw::icon(gfx, draw::SCREEN - 26, PET_Y - 8, "👉");

        // The pet leans the way it turned last round, so the reveal needs no
        // screen of its own.
        let lean = match self.last {
            Some((0, _)) => -12,
            Some((_, _)) => 12,
            None => 0,
        };
        let centre = Point::new(draw::SCREEN / 2 + lean, PET_Y);
        let style = DrawStyle::new(PixelColor::Light, PixelColor::Dark, 1);
        gfx.draw_circle(Circle::new_with_style(centre, 16, style)).ok();
        for side in [-1, 1] {
            let eye = Point::new(centre.x + side * 6, centre.y - 5);
            gfx.draw_circle(Circle::new_with_style(
                eye,
                2,
                DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1),
            ))
            .ok();
        }

        let mut count = String::new();
        write!(count, "{}/{}", (self.round + 1).min(ROUNDS), ROUNDS).ok();
        draw::text(
            gfx,
            Rectangle::new_coords(2, BOTTOM, 40, BOTTOM + 17),
            GlyphStyle::Small,
            false,
            &count,
        );

        // One line for both, because there is only room for one: the result
        // once there is a result, and until then the way out.
        let tail = match self.last {
            Some((_, true)) => "あたり！",
            Some((_, false)) => "はずれ",
            None => "o やめる",
        };
        draw::text(
            gfx,
            Rectangle::new_coords(44, BOTTOM, draw::SCREEN - 3, BOTTOM + 17),
            GlyphStyle::Regular,
            false,
            tail,
        );
    }
}

/// Vertical centre of the pet, and the top of the bottom line.
const PET_Y: isize = 76;
const BOTTOM: isize = 105;
