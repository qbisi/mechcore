//! Plays an MCFR recording back as a flat picture of the battlefield.
//!
//! A recording is read into a [`Timeline`], every object's life as tracks over
//! the ticks it lived, and written into one self-contained HTML page whose
//! script draws it: the map's towers, the constructions, the shields, each
//! unit as a top-down sprite drawn after the game's model of it, and the
//! projectiles between them. The page interpolates between ticks and animates
//! what the recording's events say happened, so a recording plays smoothly at
//! any speed and seeks to any moment.
//!
//! Nothing here decides anything about the fight: the page shows what the
//! recording holds, and a unit type the page has no sprite for is still drawn,
//! as a marked disc under its name.

mod page;
mod timeline;
mod track;

pub use page::page;
pub use timeline::{
    Building, Cue, Error, Field, Projectile, Ref, SCHEMA, Shield, Timeline, Unit, timeline,
};
pub use track::{Step, Track};
