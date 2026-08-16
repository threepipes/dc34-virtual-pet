//! Rendering. Every screen in `docs/UI.md` is drawn here, out of primitives.
//!
//! The creature is built from circles and lines rather than a sprite sheet. That
//! is not the end state -- `docs/UI.md` budgets six 96x96 bitmaps -- but it keeps
//! the game playable and the expressions legible while the mechanics are what is
//! being tuned.

use core::fmt::Write;

use blitstr2::GlyphStyle;
use dc34_virtual_pet_core::{Snapshot, Stage, METER_MAX, POOP_MAX};
use ux_api::minigfx::*;
use ux_api::service::api::Gid;
use ux_api::service::gfx::Gfx;

// -- Layout -------------------------------------------------------------------

pub const SCREEN: isize = 128;
/// Generation, day and the two meters.
pub const STATUS_H: isize = 16;
/// Top of the six-icon bar. 18 px rather than 16 so a glyph plus its margin
/// fits without the row being clipped.
pub const MENU_TOP: isize = SCREEN - 18;
/// The creature's 96 px square, between the two bars.
pub const FIELD_TOP: isize = STATUS_H;
pub const FIELD_BOTTOM: isize = MENU_TOP;

const CENTER_X: isize = SCREEN / 2;
const CENTER_Y: isize = (FIELD_TOP + FIELD_BOTTOM) / 2;

const INK: PixelColor = PixelColor::Dark;
const PAPER: PixelColor = PixelColor::Light;

fn stroke(width: isize) -> DrawStyle { DrawStyle::new(PAPER, INK, width) }
fn filled() -> DrawStyle { DrawStyle::new(INK, INK, 1) }

/// How the creature should look. Kept separate from the game state so that the
/// transient screens (evolution, farewell) can ask for a face directly.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Normal,
    Happy,
    Troubled,
    Sick,
    Asleep,
    Dead,
}

// -- Text ---------------------------------------------------------------------

/// Draw one line of text in `bounds`. Everything on screen goes through here, so
/// the `Gid::dummy()` and border defaults are stated once.
pub fn text(gfx: &Gfx, bounds: Rectangle, style: GlyphStyle, invert: bool, s: &str) {
    let mut tv = TextView::new(Gid::dummy(), TextBounds::BoundingBox(bounds));
    tv.draw_border = false;
    tv.style = style;
    tv.invert = invert;
    // The default 4 px margin costs 8 px of a 21 px menu cell, which is more
    // than the cell can spare.
    tv.margin = Point::new(1, 1);
    write!(tv, "{}", s).ok();
    gfx.draw_textview(&mut tv).ok();
}

/// A line of text spanning the width of the screen, `top` pixels down.
pub fn line(gfx: &Gfx, top: isize, height: isize, style: GlyphStyle, s: &str) {
    text(gfx, Rectangle::new_coords(2, top, SCREEN - 3, top + height), style, false, s);
}

/// Same, but with room to wrap. Full-width glyphs advance 17 px whatever the
/// style asks for, so seven of them is all a line holds; anything longer needs
/// this rather than a taller guess at where to break it.
pub fn wrapped(gfx: &Gfx, top: isize, bottom: isize, style: GlyphStyle, s: &str) {
    text(gfx, Rectangle::new_coords(2, top, SCREEN - 3, bottom), style, false, s);
}

// -- Common furniture ---------------------------------------------------------

/// Generation and age on the left, mood and fullness on the right.
pub fn status_bar(gfx: &Gfx, s: &Snapshot) {
    let mut label = String::new();
    write!(label, "G{} D{}", s.generation, s.time.day).ok();
    text(gfx, Rectangle::new_coords(2, 1, 56, STATUS_H - 1), GlyphStyle::Small, false, &label);

    meter(gfx, 62, s.mood);
    meter(gfx, 96, s.hunger);
}

/// Four pips, filled up to `level`.
fn meter(gfx: &Gfx, left: isize, level: u8) {
    for i in 0..METER_MAX as isize {
        let x = left + i * 8;
        let pip = Rectangle::new_coords_with_style(
            x,
            4,
            x + 5,
            11,
            if i < level as isize { filled() } else { stroke(1) },
        );
        gfx.draw_rectangle(pip).ok();
    }
}

/// The six-icon bar. `cursor` is `None` when nothing is selected.
pub fn menu_bar(gfx: &Gfx, labels: &[&str], cursor: Option<usize>) {
    let cell = SCREEN / labels.len() as isize;
    for (i, label) in labels.iter().enumerate() {
        let x = i as isize * cell;
        let bounds = Rectangle::new_coords(x, MENU_TOP, x + cell - 1, SCREEN - 1);
        text(gfx, bounds, GlyphStyle::Regular, cursor == Some(i), label);
    }
}

/// A framed list, used by both submenus. Sized to the number of entries.
pub fn list(gfx: &Gfx, items: &[&str], cursor: usize) {
    const ROW: isize = 18;
    let height = items.len() as isize * ROW + 8;
    let top = CENTER_Y - height / 2;
    let frame = Rectangle::new_coords_with_style(14, top, SCREEN - 15, top + height, stroke(1));
    gfx.draw_rounded_rectangle(RoundedRectangle::new(frame, 4)).ok();

    for (i, item) in items.iter().enumerate() {
        let y = top + 4 + i as isize * ROW;
        let mut row = String::new();
        write!(row, "{}{}", if i == cursor { "> " } else { "  " }, item).ok();
        text(gfx, Rectangle::new_coords(18, y, SCREEN - 19, y + ROW), GlyphStyle::Regular, false, &row);
    }
}

/// The button legend along the bottom, for screens without the icon bar.
pub fn legend(gfx: &Gfx, s: &str) {
    text(gfx, Rectangle::new_coords(0, MENU_TOP, SCREEN - 1, SCREEN - 1), GlyphStyle::Small, false, s);
}

// -- The creature -------------------------------------------------------------

/// Body radius by stage. Growing is most of what evolution looks like here.
fn body_radius(stage: Stage) -> isize {
    match stage {
        Stage::Egg | Stage::Baby => 14,
        Stage::Child => 19,
        Stage::Teen => 24,
        Stage::Adult => 28,
    }
}

pub fn creature(gfx: &Gfx, stage: Stage, face: Face) {
    if stage == Stage::Egg {
        egg(gfx);
        return;
    }
    let r = body_radius(stage);
    let c = Point::new(CENTER_X, CENTER_Y);
    gfx.draw_circle(Circle::new_with_style(c, r, stroke(1))).ok();

    let eye_y = CENTER_Y - r / 3;
    let eye_dx = r / 2;
    for side in [-1, 1] {
        let x = CENTER_X + side * eye_dx;
        match face {
            Face::Asleep => {
                gfx.draw_line(Line::new_with_style(
                    Point::new(x - 3, eye_y),
                    Point::new(x + 3, eye_y),
                    stroke(1),
                ))
                .ok();
            }
            Face::Dead => {
                gfx.draw_line(Line::new_with_style(
                    Point::new(x - 3, eye_y - 3),
                    Point::new(x + 3, eye_y + 3),
                    stroke(1),
                ))
                .ok();
                gfx.draw_line(Line::new_with_style(
                    Point::new(x - 3, eye_y + 3),
                    Point::new(x + 3, eye_y - 3),
                    stroke(1),
                ))
                .ok();
            }
            _ => {
                gfx.draw_circle(Circle::new_with_style(Point::new(x, eye_y), 2, filled())).ok();
            }
        }
    }

    // The mouth carries the mood: flat when content, open when calling, a thin
    // line when ill or asleep.
    let mouth_y = CENTER_Y + r / 3;
    match face {
        Face::Happy => {
            gfx.draw_line(Line::new_with_style(
                Point::new(CENTER_X - 5, mouth_y),
                Point::new(CENTER_X, mouth_y + 4),
                stroke(1),
            ))
            .ok();
            gfx.draw_line(Line::new_with_style(
                Point::new(CENTER_X, mouth_y + 4),
                Point::new(CENTER_X + 5, mouth_y),
                stroke(1),
            ))
            .ok();
        }
        Face::Troubled => {
            gfx.draw_circle(Circle::new_with_style(Point::new(CENTER_X, mouth_y + 1), 3, stroke(1)))
                .ok();
        }
        _ => {
            gfx.draw_line(Line::new_with_style(
                Point::new(CENTER_X - 4, mouth_y),
                Point::new(CENTER_X + 4, mouth_y),
                stroke(1),
            ))
            .ok();
        }
    }

}

/// Width and height of an icon's box. A glyph is 16 px and the box loses 2 px to
/// margins, but a box of exactly 18 draws nothing: wordwrap drops a word when
/// its width is `>=` the line, not `>`, so 16 in 16 counts as an overflow. The
/// slack is what makes the icon appear at all.
const ICON_BOX: isize = 22;

/// One 16 px glyph, placed. Text is the only way to get an emoji on screen, and
/// a `TextView` clears its own box, so these go where nothing else is drawn.
pub fn icon(gfx: &Gfx, left: isize, top: isize, s: &str) {
    let bounds = Rectangle::new_coords(left, top, left + ICON_BOX - 1, top + ICON_BOX - 3);
    text(gfx, bounds, GlyphStyle::Regular, false, s);
}

/// The row along the bottom of the field where problems collect: droppings on
/// the left, and on the right whatever the pet is calling about.
///
/// Illness and a bottomed-out meter used to look identical -- a troubled face
/// and a couple of exclamation marks -- so they get their own glyphs.
const TROUBLE_TOP: isize = FIELD_BOTTOM - (ICON_BOX - 2);

pub fn mess(gfx: &Gfx, count: u8) {
    for i in 0..count.min(POOP_MAX) as isize {
        icon(gfx, 4 + i * (ICON_BOX + 2), TROUBLE_TOP, "💩");
    }
}

/// Illness wins over a plain call: it is the one that needs medicine rather
/// than food, and showing both would not fit.
pub fn trouble(gfx: &Gfx, sick: bool, alert: bool) {
    let left = SCREEN - ICON_BOX - 2;
    if sick {
        icon(gfx, left, TROUBLE_TOP, "🤒");
    } else if alert {
        icon(gfx, left, TROUBLE_TOP, "❗");
    }
}

fn egg(gfx: &Gfx) {
    let c = Point::new(CENTER_X, CENTER_Y + 4);
    gfx.draw_circle(Circle::new_with_style(c, 18, stroke(1))).ok();
    gfx.draw_circle(Circle::new_with_style(Point::new(CENTER_X, CENTER_Y - 8), 13, stroke(1))).ok();
    // Wipe the seam where the two circles overlap, so it reads as one shape.
    gfx.draw_line(Line::new_with_style(
        Point::new(CENTER_X - 12, CENTER_Y - 4),
        Point::new(CENTER_X + 12, CENTER_Y - 4),
        DrawStyle::new(PAPER, PAPER, 3),
    ))
    .ok();
}
