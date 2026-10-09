//! `FlyTechEffectProvider`: a unit a technology takes into the air or brings
//! to the ground.
//!
//! A `FlyTech` turns its unit's domain: one whose type is on the ground
//! flies, one whose type flies lands (`FlyTechData.PreProcess`,
//! `GetUnitStateType`). Its provider sets the unit's `FightMech.isFly` as
//! its effects are activated, as the fight starts or as it lands, joins or
//! rises again (`DoActive`, `DoEnableOutOfFight`, `FightMech.SetTechFly`),
//! and sets it back as they are deactivated, as it dies (`DoDeactive`,
//! `DoDisableOutOfFight`). Every part of the fight that asks whether the
//! unit flies asks `FightMech.IsFly`: its search and the searches that find
//! it, its agent's layer and height (`RVOControllerFixed.RefreshMainLayer`),
//! its height where it stands (`FightMech.SetPosition`), what a splash or a
//! terrain reaches of it. Switched off with its technologies, it takes its
//! type's domain back once its row's `landingDuration` has passed
//! (`DisableEffect`, `DoDisableInFight`, a `GRTimerManager` timer whose
//! callback lands or flies it, `LandingCallBack`, `FlyUpCallBack`);
//! switched on again before then, it keeps the domain the technology gave it
//! and the timer is stopped (`EnableEffect`, `DoEnableInFight`,
//! `TryStopByObject`), and switched on after, it takes that domain at once.
//! `docs/rules/technology_effects.md` states the rule.

use super::*;

impl Actor {
    /// `IMechData.IsFly`: whether the unit's type flies.
    fn type_flies(&self) -> bool {
        self.rules.domain == UnitDomain::Air
    }

    /// `FightMech.SetTechFly`: the unit flies or stands on the ground.
    fn set_tech_fly(&mut self, fly: bool) {
        self.domain = if fly {
            UnitDomain::Air
        } else {
            UnitDomain::Ground
        };
    }
}

impl Simulation {
    /// `FlyTechEffectProvider.DoActive` of every unit the fight starts with
    /// on the ground (`FightEffectSystem.OnEnterFight`): a travelling unit
    /// keeps its type's domain until it lands ([`Self::add_fly_unit`]).
    pub(in crate::fight) fn enter_fly_fight(&mut self) {
        let held = self
            .actors
            .iter()
            .filter(|(_, actor)| !actor.travelling && actor.placement.effects.single.fly.is_some())
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        for unit in held {
            self.add_fly_unit(unit);
        }
    }

    /// `FlyTechEffectProvider.DoActive` (`DoEnableOutOfFight`): the unit
    /// takes the domain its technology gives it, at once.
    pub(in crate::fight) fn add_fly_unit(&mut self, unit: u64) {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        if actor.placement.effects.single.fly.is_some() {
            let fly = !actor.type_flies();
            actor.set_tech_fly(fly);
        }
    }

    /// `FlyTechEffectProvider.DoDeactive` (`DoDisableOutOfFight`), as the
    /// unit dies: it takes its type's domain back at once, and a timer of
    /// its switching is let go.
    pub(in crate::fight) fn remove_fly_unit(&mut self, unit: u64) {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        if actor.placement.effects.single.fly.is_some() {
            let fly = actor.type_flies();
            actor.set_tech_fly(fly);
            actor.fly_reverts_at = None;
        }
    }

    /// `FlyTechEffectProvider.EnableEffect` and `DisableEffect`. Each does
    /// nothing while the unit stands in the domain it would take
    /// (`mechData.IsFly() == IsFly()` for one, the opposite for the other).
    /// Off, a timer of the row's landing duration is started, its last one
    /// stopped, whose callback gives it its type's domain back
    /// (`DoDisableInFight`); on, it takes the technology's domain at once
    /// and the timer is stopped (`DoEnableInFight`, whose own timer's
    /// callback then finds nothing to do).
    pub(in crate::fight) fn switch_fly(&mut self, unit: u64, on: bool) {
        let step_now = self.step_now;
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        let Some(source) = actor.placement.effects.single.fly else {
            return;
        };
        let turned = actor.domain != actor.rules.domain;
        if on {
            if !turned {
                let fly = !actor.type_flies();
                actor.set_tech_fly(fly);
            }
            actor.fly_reverts_at = None;
        } else if turned {
            actor.fly_reverts_at = Some(step_now + math::seconds_q32_to_steps(source.landing_q32));
        }
    }

    /// The `GRTimerManager` timers due on this step: each unit whose
    /// technology was switched off lands or flies up
    /// (`LandingCallBack`, `FlyUpCallBack`), taking its type's domain back if
    /// it still holds the other.
    pub(in crate::fight) fn revert_fly_units(&mut self, step: u64) {
        for actor in self.actors.values_mut() {
            if actor.fly_reverts_at.is_some_and(|at| at <= step) {
                actor.fly_reverts_at = None;
                let fly = actor.type_flies();
                actor.set_tech_fly(fly);
            }
        }
    }
}
