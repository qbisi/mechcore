//! `FightEffectSystem`: each effect provider of a unit handed the unit as
//! the fight starts, as the unit's effects are activated, as they are
//! deactivated, and as its technologies are switched off and on.
//!
//! The build gives every unit a copy of its side's `FightEffectMananger` for
//! its type (`TeamFightEffectManager.CreateMechUnitEffectMananger`), and
//! `FightEffectMananger.ActiveEffect`, `DeactiveEffect`, `DisableEffect` and
//! `EnableEffect` hand the unit to each provider in it alike. Here each of
//! those is one function handing the unit to every provider of
//! [`EffectProvider::ALL`], and what each provider does with it is one arm
//! of a match: a provider whose system holds its units adds, removes or
//! switches the unit there, which passes over a unit it has no source of,
//! and every other provider's source is read from the unit's placement where
//! the fight acts. A new provider is listed in [`EffectProvider::ALL`] and
//! given an arm in each.

use super::*;
use crate::modifier::EffectProvider;

impl Simulation {
    /// `FightEffectSystem.OnEnterFight`: every unit the fight starts with on
    /// the ground handed to its providers, then each system's own start.
    /// A unit travelling in is handed over as it arrives
    /// ([`Self::active_effect`]).
    pub(in crate::fight) fn enter_effects_fight(&mut self) {
        for provider in EffectProvider::ALL {
            match provider {
                EffectProvider::InterceptMissile => self.activate_interceptions(),
                EffectProvider::StealthTech => self.enter_stealth_fight(),
                EffectProvider::MechGroup => self.start_groups(),
                EffectProvider::ReactiveArmor => self.enter_reactive_armor_fight(),
                EffectProvider::WreckageRecovery => self.enter_wreckage_fight(),
                EffectProvider::Burrow => self.enter_burrow_fight(),
                EffectProvider::Repair => self.enter_repair_fight(),
                // `SiegeModeEffectSystem.OnEnterFight` digs its units in
                // after each skill drew its first interval
                // (`Simulation::enter_siege_fight`).
                EffectProvider::SiegeMode
                | EffectProvider::LifeSteal
                | EffectProvider::AutoRecovery
                | EffectProvider::EnergyShield
                | EffectProvider::SweepSkillIntensify
                | EffectProvider::ArmorStrengthen
                | EffectProvider::SearchTargetSpecific
                | EffectProvider::AirAttack
                | EffectProvider::SecondaryDamageIntensify
                | EffectProvider::Buff
                | EffectProvider::SupportUnit
                | EffectProvider::DeadEffect
                | EffectProvider::MoveAbilitySummon
                | EffectProvider::ExtraSkill
                | EffectProvider::DeadLine
                | EffectProvider::MoveAbilityAttackIntensify
                | EffectProvider::MoveAbilityRangeItem
                | EffectProvider::AdvancedEnergyShield
                | EffectProvider::FireIntensify
                | EffectProvider::RvoRadiusChange
                | EffectProvider::ClearRangeItem
                | EffectProvider::KillExplosion => {}
            }
        }
    }

    /// `FightEffectSystem.ActiveEffect`: the unit handed to each of its
    /// providers as it lands, joins or rises again.
    pub(in crate::fight) fn active_effect(&mut self, unit: u64) -> Result<()> {
        for provider in EffectProvider::ALL {
            match provider {
                EffectProvider::InterceptMissile => self.activate_interception(unit),
                EffectProvider::StealthTech => self.add_stealth_unit(unit),
                EffectProvider::MechGroup => self.add_group_unit(unit),
                EffectProvider::SiegeMode => self.add_siege_unit(unit)?,
                EffectProvider::ReactiveArmor => self.activate_reactive_armor(unit),
                EffectProvider::WreckageRecovery => self.add_wreckage_unit(unit),
                EffectProvider::Burrow => self.add_burrow_unit(unit),
                EffectProvider::Repair => self.add_repair_unit(unit),
                // The rest are read from the unit where the fight acts, which
                // passes over a unit still travelling or rising.
                EffectProvider::LifeSteal
                | EffectProvider::AutoRecovery
                | EffectProvider::EnergyShield
                | EffectProvider::SweepSkillIntensify
                | EffectProvider::ArmorStrengthen
                | EffectProvider::SearchTargetSpecific
                | EffectProvider::AirAttack
                | EffectProvider::SecondaryDamageIntensify
                | EffectProvider::Buff
                | EffectProvider::SupportUnit
                | EffectProvider::DeadEffect
                | EffectProvider::MoveAbilitySummon
                | EffectProvider::ExtraSkill
                | EffectProvider::DeadLine
                | EffectProvider::MoveAbilityAttackIntensify
                | EffectProvider::MoveAbilityRangeItem
                | EffectProvider::AdvancedEnergyShield
                | EffectProvider::FireIntensify
                | EffectProvider::RvoRadiusChange
                | EffectProvider::ClearRangeItem
                | EffectProvider::KillExplosion => {}
            }
        }
        Ok(())
    }

    /// `FightEffectSystem.DeactiveEffect` of a unit that died: each of its
    /// providers lets it go.
    pub(in crate::fight) fn deactive_effect(&mut self, unit: u64) -> Result<()> {
        for provider in EffectProvider::ALL {
            match provider {
                EffectProvider::MechGroup => self.remove_group_unit(unit),
                EffectProvider::SiegeMode => self.remove_siege_unit(unit)?,
                EffectProvider::WreckageRecovery => self.remove_wreckage_unit(unit),
                EffectProvider::Burrow => self.remove_burrow_unit(unit),
                EffectProvider::Repair => self.remove_repair_unit(unit),
                // Every dead unit's interceptors are deactivated together,
                // after the summons the deaths make
                // (`Simulation::deactivate_dead_interceptions`).
                EffectProvider::InterceptMissile
                // A dead unit's stealth and reactive armor end with it.
                | EffectProvider::StealthTech
                | EffectProvider::ReactiveArmor
                | EffectProvider::LifeSteal
                | EffectProvider::AutoRecovery
                | EffectProvider::EnergyShield
                | EffectProvider::SweepSkillIntensify
                | EffectProvider::ArmorStrengthen
                | EffectProvider::SearchTargetSpecific
                | EffectProvider::AirAttack
                | EffectProvider::SecondaryDamageIntensify
                | EffectProvider::Buff
                | EffectProvider::SupportUnit
                | EffectProvider::DeadEffect
                | EffectProvider::MoveAbilitySummon
                | EffectProvider::ExtraSkill
                | EffectProvider::DeadLine
                | EffectProvider::MoveAbilityAttackIntensify
                | EffectProvider::MoveAbilityRangeItem
                | EffectProvider::AdvancedEnergyShield
                | EffectProvider::FireIntensify
                | EffectProvider::RvoRadiusChange
                | EffectProvider::ClearRangeItem
                | EffectProvider::KillExplosion => {}
            }
        }
        Ok(())
    }

    /// One provider's `DisableEffect` or `EnableEffect` on a unit, beside
    /// what its technologies wrote onto its numbers, which every provider
    /// takes away and writes again alike. The layout refuses a provider whose
    /// own the fight does not mirror ([`EffectProvider::disable_read`]).
    pub(in crate::fight) fn switch_provider(
        &mut self,
        actor_id: u64,
        provider: EffectProvider,
        on: bool,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        match provider {
            // `ExtraSkillProvider.DisableSkill` and `EnableSkill`: every
            // extra skill of a technology that switches is disabled and
            // enabled with it (`FightSkill.IsBelongExtraSkill`).
            EffectProvider::ExtraSkill => {
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                let switched = &actor.placement.effects.technology_disable.technologies;
                for extra in &mut actor.skills.extras {
                    if switched.contains(&extra.rules.technology) {
                        extra.skill.disabled = !on;
                    }
                }
            }
            EffectProvider::StealthTech => self.switch_stealth(actor_id, on),
            EffectProvider::MechGroup => self.switch_group_unit(actor_id, on),
            EffectProvider::AdvancedEnergyShield => self.switch_carried_shield(actor_id, on),
            EffectProvider::RvoRadiusChange => self.switch_rvo_radius_change(actor_id, on),
            EffectProvider::Repair => self.switch_repair(actor_id, on),
            EffectProvider::AutoRecovery => self.switch_auto_recovery(actor_id, on),
            EffectProvider::EnergyShield => self.switch_energy_shield(actor_id, on),
            EffectProvider::SweepSkillIntensify => self.switch_sweep(actor_id, on),
            EffectProvider::InterceptMissile => self.switch_unit_interception(actor_id, on),
            EffectProvider::Buff => self.switch_buff_cycles(actor_id, on),
            EffectProvider::ReactiveArmor => self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .switch_reactive_armor(on),
            EffectProvider::SiegeMode => {
                if !on {
                    self.end_siege_mode(actor_id);
                }
            }
            EffectProvider::Burrow => return self.switch_burrow(actor_id, on, events),
            // The rest take away what the fight asks of the unit where it
            // acts, its technologies disabled: a lifesteal's and a second
            // damage's hit effect, a search's ranges, offsets and selector,
            // an armour's reduction among its numbers, and a buff its unit
            // added itself, which `BuffManager` clears as the disabling buff
            // enters. What the layout refuses switched does not reach here.
            // `AirAttackEffectProvider.DisableEffect` and `EnableEffect` are
            // `NormalEffectProvider`'s, which do nothing: the skills it turned
            // onto or off aircraft stay so.
            EffectProvider::AirAttack
            | EffectProvider::WreckageRecovery
            | EffectProvider::LifeSteal
            | EffectProvider::ArmorStrengthen
            | EffectProvider::SearchTargetSpecific
            | EffectProvider::SecondaryDamageIntensify
            | EffectProvider::SupportUnit
            | EffectProvider::DeadEffect
            | EffectProvider::MoveAbilitySummon
            | EffectProvider::DeadLine
            | EffectProvider::MoveAbilityAttackIntensify
            | EffectProvider::MoveAbilityRangeItem
            | EffectProvider::FireIntensify
            | EffectProvider::ClearRangeItem
            | EffectProvider::KillExplosion => {}
        }
        Ok(())
    }
}
