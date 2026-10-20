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
//! switched on after the body has reverted, it takes the technology's domain
//! at once and replaces its timer with a callback to that same domain
//! (`EnableEffect`, `DoEnableInFight`, `TryStopByObject`). Deactivation does
//! not stop either callback, which can change even a dead unit's domain.
//! `docs/rules/technology_effects.md` states the rule.

use super::*;
use crate::data::Index;

impl Actor {
    /// `IMechData.IsFly`: whether the unit's type flies.
    fn type_flies(&self) -> bool {
        self.rules.domain == UnitDomain::Air
    }

    /// `FightMech.SetTechFly`: the unit physically flies or stands on the
    /// ground, independently of the source's unit-state data.
    fn set_tech_fly(&mut self, fly: bool) {
        let before = self.domain;
        self.domain = if fly {
            UnitDomain::Air
        } else {
            UnitDomain::Ground
        };
        if before == self.domain {
            return;
        }
        // `SetTechFly` refreshes the agent's layer and then calls
        // `SetPosition` with the current position, applying the new height now.
        self.placed_domain = self.domain;
    }

    /// `FightEffectMananger.RefreshOtherEffect`: adding or removing a
    /// `FlyTech` unit-state source changes conditional data immediately,
    /// even while the body's landing or flight timer is still pending.
    pub(in crate::fight) fn refresh_domain_effects(&mut self, on: bool, step: u64) {
        let domain = if on {
            match self.rules.domain {
                UnitDomain::Air => UnitDomain::Ground,
                UnitDomain::Ground => UnitDomain::Air,
            }
        } else {
            self.rules.domain
        };
        let before = self.modifier_domain;
        if before == domain {
            return;
        }
        self.modifier_domain = domain;
        if self.placement.effects.domain_corrections.is_empty() {
            return;
        }
        for (held_in, channel, entry) in self.placement.effects.domain_corrections.clone() {
            let overlay = self.stats.overlays.channel(channel);
            if held_in == before {
                overlay.remove(entry);
            } else if held_in == domain {
                overlay.write(entry);
            }
            if entry.index == Index::MaxLife {
                self.refresh_life_data()
                    .expect("the layout verified the officers' corrections");
            }
        }
        self.refresh_life_data()
            .expect("the layout verified the officers' corrections");
        self.refresh_common_skill_data(step)
            .expect("the layout verified the skills' corrections");
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
            actor.refresh_domain_effects(true, self.step_now);
            actor.set_tech_fly(fly);
        }
    }

    /// `FlyTechEffectProvider.DoDeactive` (`DoDisableOutOfFight`), as the
    /// unit dies: it takes its type's domain back at once. A callback already
    /// registered by enabling or disabling the technology remains scheduled.
    pub(in crate::fight) fn remove_fly_unit(&mut self, unit: u64) {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        if actor.placement.effects.single.fly.is_some() {
            let fly = actor.type_flies();
            actor.refresh_domain_effects(false, self.step_now);
            actor.set_tech_fly(fly);
        }
    }

    /// `FlyTechEffectProvider.EnableEffect` and `DisableEffect`. Each does
    /// nothing while the unit stands in the domain it would take
    /// (`mechData.IsFly() == IsFly()` for one, the opposite for the other).
    /// Off, a timer of the row's landing duration is started, its last one
    /// stopped, whose callback gives it its type's domain back
    /// (`DoDisableInFight`); on, it takes the technology's domain at once
    /// and replaces the timer with a callback to that same domain
    /// (`DoEnableInFight`). Usually that callback finds nothing to do, but
    /// deactivation can change the domain before it runs, even after death.
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
        if on != turned {
            let fly = if on {
                !actor.type_flies()
            } else {
                actor.type_flies()
            };
            if on {
                actor.set_tech_fly(fly);
            }
            let delay = math::seconds_q32_to_steps(source.landing_q32)
                .saturating_sub(u64::from(self.updating_timers));
            actor.fly_transition = Some((step_now + delay, fly));
            self.timers.stop(timers::Callback::Fly(unit));
            self.timers.start(timers::Callback::Fly(unit));
        }
    }

    /// The `GRTimerManager` timers due on this step: each unit whose
    /// technology was switched lands or flies up (`LandingCallBack`,
    /// `FlyUpCallBack`) if its current domain differs from the callback's.
    /// Neither callback checks whether the unit is alive.
    pub(in crate::fight) fn finish_fly_transition(&mut self, unit: u64, step: u64) -> bool {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("a timer's owner remains present");
        let Some((at, fly)) = actor.fly_transition else {
            return false;
        };
        if at > step {
            return true;
        }
        actor.fly_transition = None;
        actor.set_tech_fly(fly);
        false
    }
}
