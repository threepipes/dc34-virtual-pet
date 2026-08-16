//! Where the pet lives between power cycles.
//!
//! The badge is meant to be unplugged, so a game that only exists in RAM starts
//! over every time. This is the PDDB side of that: one key holding one
//! fixed-size blob.
//!
//! Writes go through `sync()` on purpose. PDDB caches otherwise, and a cached
//! write is exactly the one lost to a yanked cable -- which is the case this
//! whole file exists to survive.

use dc34_virtual_pet_core::{Game, SAVE_LEN};

/// Shared with the rest of the badge's data, under this app's own key.
const DICT: &str = "dc34.game";
const KEY: &str = "virtual-pet";

/// Read the saved game, if there is one this build understands.
///
/// Anything that goes wrong -- no key, short read, a format from another
/// version -- reads as "no save", and the caller starts a new lineage. Losing a
/// pet is bad; refusing to start is worse.
pub fn load() -> Option<Game> {
    let pddb = pddb::Pddb::new();
    pddb.is_mounted_blocking();

    // `create_key: false`, so a first run does not leave an empty key behind.
    let mut key = pddb.get(DICT, KEY, None, true, false, Some(SAVE_LEN), None::<fn()>).ok()?;
    let mut buf = [0u8; SAVE_LEN];
    let read = std::io::Read::read(&mut key, &mut buf).ok()?;
    if read < SAVE_LEN {
        return None;
    }
    Game::from_bytes(&buf)
}

/// Write the game out, and push it to flash.
///
/// Returns whether it landed. The caller shows that to the player rather than
/// claiming a save that did not happen.
pub fn save(game: &Game) -> bool {
    let pddb = pddb::Pddb::new();
    let bytes = game.to_bytes();
    let ok = pddb
        .get(DICT, KEY, None, true, true, Some(SAVE_LEN), None::<fn()>)
        .and_then(|mut key| std::io::Write::write_all(&mut key, &bytes))
        .is_ok();
    ok && pddb.sync().is_ok()
}
