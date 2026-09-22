//! The `Modifier` step: a unit's level, officers and technologies
//! before the fight begins.
//!
//! The build applies them at deployment rather than inside the fight —
//! `TechnologySystem.AddTechnologyEffect` takes a `PlayerController` and is
//! called from `MAP_AddUnit` — so they are applied as the fight is built, into
//! the overlays [`crate::data`] resolves. [`effects`] is what both tables
//! write; [`officers`] and [`technologies`] read their own table and say which
//! units a row reaches. [`levels`] supplies `IMechLevelData` ratings in the
//! base channel, before either dynamic overlay is applied.

mod effects;
mod levels;
mod officers;
mod technologies;

pub(crate) use levels::LevelEffects;
pub(crate) use officers::OfficerEffects;
pub(crate) use technologies::TechnologyEffects;
