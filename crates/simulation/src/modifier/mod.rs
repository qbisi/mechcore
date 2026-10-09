//! The `Modifier` step: what a unit's officers, technologies and equipment,
//! and its side's Energy Tower skills, write onto it before the fight begins.
//!
//! The build applies them at deployment rather than inside the fight —
//! `TechnologySystem.AddTechnologyEffect` takes a `PlayerController` and is
//! called from `MAP_AddUnit` — so they are applied as the fight is built, into
//! the overlays [`crate::data`] resolves. [`effects`] is what every table
//! writes; [`officers`], [`technologies`], [`equipment`] and
//! [`energy_tower`] read their own table, [`targets`] says which units a
//! row's targeting category reaches, and [`sources`] is what an equipment or
//! a technology hands its unit beyond its numbers.

mod buffs;
mod effects;
mod energy_tower;
mod equipment;
mod officers;
mod sources;
mod targets;
mod technologies;

pub(crate) use energy_tower::EnergyTowerSkillEffects;
pub(crate) use equipment::EquipmentEffects;
pub(crate) use officers::{ContraptionRates, OfficerEffects};
pub(crate) use sources::{
    AllCycle, Arrival, AutoRecovery, BuffReach, BuffSource, BuffTrigger, CarriedShield, DeadSummon,
    EnergyShield, LifeSteal, ProductionLine, StackCondition, Stealth, SweepIntensify,
    current as current_source,
};
pub(crate) use technologies::{
    ARMOR_SOURCE, MainSkill, SOURCE as TECHNOLOGY_SOURCE, SecondaryDamage, TechnologyEffects,
    UnitInterception,
};
