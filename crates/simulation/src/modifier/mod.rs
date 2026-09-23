//! The `Modifier` step: what a unit's officers, technologies and equipment
//! write onto it before the fight begins.
//!
//! The build applies them at deployment rather than inside the fight —
//! `TechnologySystem.AddTechnologyEffect` takes a `PlayerController` and is
//! called from `MAP_AddUnit` — so they are applied as the fight is built, into
//! the overlays [`crate::data`] resolves. [`effects`] is what every table
//! writes; [`officers`], [`technologies`] and [`equipment`] read their own
//! table, and [`targets`] says which units a row's targeting category
//! reaches.

mod effects;
mod equipment;
mod officers;
mod targets;
mod technologies;

pub(crate) use equipment::EquipmentEffects;
pub(crate) use officers::OfficerEffects;
pub(crate) use technologies::TechnologyEffects;
