//! The `Modifier` step: what a unit's officers and technologies write onto it
//! before the fight begins.
//!
//! The build applies them at deployment rather than inside the fight —
//! `TechnologySystem.AddTechnologyEffect` takes a `PlayerController` and is
//! called from `MAP_AddUnit` — so they are applied as the fight is built, into
//! the overlays [`crate::data`] resolves. [`effects`] is what both tables
//! write; [`officers`] and [`technologies`] read their own table and say which
//! units a row reaches.

mod effects;
mod officers;
mod technologies;

pub(crate) use officers::OfficerEffects;
pub(crate) use technologies::TechnologyEffects;
