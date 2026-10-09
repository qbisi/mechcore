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
                EffectProvider::FlyTech => self.enter_fly_fight(),
                EffectProvider::IgnoreBuff => self.enter_ignore_buff_fight(),
                EffectProvider::SkillSearchTarget => self.enter_life_priority_fight(),
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
                | EffectProvider::KillExplosion
                | EffectProvider::AdditionalDamage => {}
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
                EffectProvider::FlyTech => self.add_fly_unit(unit),
                EffectProvider::IgnoreBuff => self.add_ignore_buff_unit(unit),
                EffectProvider::SkillSearchTarget => self.switch_life_priority(unit, true),
                EffectProvider::Buff => self.activate_buff_cycles(unit),
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
                | EffectProvider::KillExplosion
                | EffectProvider::AdditionalDamage => {}
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
                EffectProvider::FlyTech => self.remove_fly_unit(unit),
                EffectProvider::IgnoreBuff => self.switch_ignore_buff(unit, false),
                EffectProvider::SkillSearchTarget => self.switch_life_priority(unit, false),
                EffectProvider::Buff => self.deactivate_buff_cycles(unit),
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
                | EffectProvider::KillExplosion
                | EffectProvider::AdditionalDamage => {}
            }
        }
        Ok(())
    }

    /// One provider's `DisableEffect` or `EnableEffect` on a unit, beside
    /// what its technologies wrote onto its numbers, which every provider
    /// takes away and writes again alike.
    ///
    /// - An armour's provider takes its reduction away, which is the unit's
    ///   numbers.
    /// - A lifesteal's and a second damage's providers take their hit effect
    ///   away, which the fight asks of the unit at each hit.
    /// - A search technology's `SearchTargetSpecificProvider.DoDisable` takes
    ///   its ranges and score offsets away and turns its unit's selector back
    ///   to `Normal`, which is the selector it turned to with no offsets, and
    ///   `DoEnable` writes them and turns it to `DistanceIntensify` again,
    ///   which reads them as they now stand.
    /// - An extra weapon's provider disables its skills
    ///   (`ExtraSkillProvider.DisableSkill`), which the layout refuses for the
    ///   shapes it does not fight switched off.
    /// - A buff added once onto its own unit is cleared
    ///   (`BuffManager.ClearSelfResourceBuffByDisableTech`), and every buff
    ///   controller of the unit stops, its listener taken off, and starts
    ///   again at no time (`BuffEffectProvider.DoDisableCycle`,
    ///   `DoEnableCycle`).
    /// - A stealth technology's unit is shown and counts as triggered
    ///   (`StealthTechSystem.DisableStealthTech`).
    /// - A dead-line technology's pre-hit effect is taken off its unit's
    ///   skills (`DeadLineEffectProvider.DisableEffect`), and does nothing
    ///   while the technologies are off (`PerformPreHitEffect`).
    /// - A grouping technology's unit leaves its side's groups
    ///   (`MechGrounpEffectProvider.DisableEffect`,
    ///   `TeamMechGroupManager.RemoveMech`) and is handed back (`AddMech`).
    /// - A barrier technology's shield is disabled and deactivated, its
    ///   energy recorded, and given back that energy and activated again
    ///   (`AdvancedEnergyShieldProvider.DisableEffect`, `EnableEffect`).
    /// - A reactive armor's rate leaves its unit, its count kept, and comes
    ///   back while the count lasts (`ReactiveArmorSystem.DisableReactiveArmor`,
    ///   `EnableReactiveArmor`).
    /// - A siege-mode technology's unit leaves its trench on the system's
    ///   next update, as if no enemy had stood in range for its whole
    ///   duration (`SiegeModeEffectProvider.DisableEffect`,
    ///   `SiegeModeEffectSystem.EndSiegeMode`), and does not dig in again
    ///   (`EnableEffect` is `SingleEffectProvider`'s alone).
    /// - A fire technology's hit leaves no fire while the technologies are
    ///   off (`FireIntensifyEffectProvider.PerformHitEffect` returns on
    ///   `isTechnologyDisabled` for a source that `CanDisable`).
    /// - A wreckage-recovery technology's hit effect is taken off its unit's
    ///   skills (`WreckageRecoveryEffectProvider.DisableEffect`,
    ///   `SkillManager.RemoveHitEffect`) and handed back (`EnableEffect`);
    ///   what its unit struck before still heals it as it dies.
    /// - A loose-formation technology's agent keeps its own inner radius
    ///   from the others of its team and stops switching
    ///   (`MotionController.DisableRVOChangeRadius`), and switches again from
    ///   its next update (`EnableRVOChangeRadius`).
    /// - A fire-extinguisher technology's unit clears nothing
    ///   (`TeamClearRangeItemManager.DisableMech`), and clears again from the
    ///   manager's next clearing (`EnableMech`).
    /// - A dead effect is taken off the unit's `DeadEffectSystem` controller
    ///   and handed back (`DeadEffectProvider.DisableEffect`,
    ///   `EnableEffect`): a unit that dies with its technologies off leaves
    ///   no acid, summons nothing and does not rise.
    /// - A kill-explosion technology's hit effect is taken off its unit's
    ///   skills (`KillExplosionEffectProvider.DisableEffect`,
    ///   `SkillManager.RemoveHitEffect`) and handed back (`EnableEffect`);
    ///   a hit while the technologies are off sets nothing off
    ///   (`PerformHitEffect` returns on `isTechnologyDisabled`).
    /// - A repair technology's controller is disabled, its clocks standing
    ///   still, and enabled again (`AutoRecoveryEffectProvider.DisableEffect`,
    ///   `EnableEffect`, `AutoRecoverySystem.DisableMech`, `EnableMech`).
    /// - A unit's own shield is disabled, its energy's share of its maximum
    ///   recorded, and enabled again at that share
    ///   (`EnergyShieldController.Disable`, `Enable`).
    /// - A sweep technology's changes leave its unit's sweep skill, which is
    ///   reset to its own, and are applied again
    ///   (`SweepSkillIntensifyEffectProvider.TryApply`).
    /// - An air attack technology's switch stays: its provider's
    ///   `DisableEffect` and `EnableEffect` do nothing.
    /// - An interception technology's interceptors are disabled and enabled,
    ///   each letting its target go and returning to idle
    ///   (`InterceptEffectBase.DoDisable`, `DoEnable`).
    /// - A stronger surfacing's linker takes its effect away and holds none
    ///   while off, its count going on
    ///   (`FightSkill.SetAttackCountEffectLinkerEnable`).
    /// - A sand fog's action leaves its unit's move ability and is put back
    ///   (`MoveAbilityRangeItemSystem.RemoveMech`, `AddMech`).
    /// - A support technology's production line counts on and makes nothing
    ///   (`SupportUnitSystem.Disable`, `Enable`).
    /// - A surfacing line's action leaves its unit's move ability and is
    ///   put back (`MoveAbilitySummonSystem`).
    /// - A burrowing technology's unit comes up, its buff removed, and its
    ///   manager passes over it (`BurrowSystem.Deactive`) until it is
    ///   switched on (`Active`).
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
            // enabled with it (`FightSkill.IsBelongExtraSkill`), each skill
            // of a grouped row its own, and so is each of a row's skills
            // that joined the main skill's group
            // (`FightSkillFactory.PrepareGroupedSkill` only adds them to it).
            EffectProvider::ExtraSkill => {
                let actor = self
                    .actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable");
                let switched = &actor.placement.effects.technology_disable.technologies;
                let mut support_skills = Vec::new();
                for extra in &mut actor.skills.extras {
                    if switched.contains(&extra.rules.technology) {
                        if extra.skill.kind == SkillKind::Support {
                            support_skills.push(extra.rules.technology);
                        }
                        extra.skill.disabled = !on;
                        for sibling in extra.skill.siblings_mut() {
                            sibling.disabled = !on;
                        }
                    }
                }
                let main = &mut actor.skills.main;
                for slot in 1..main.group_size() {
                    if main
                        .joined(slot)
                        .is_some_and(|joined| switched.contains(&joined.technology))
                    {
                        main.sibling_mut(slot).disabled = !on;
                    }
                }
                for technology in support_skills {
                    self.switch_support_skill_line(actor_id, technology, on);
                }
            }
            EffectProvider::StealthTech => self.switch_stealth(actor_id, on),
            EffectProvider::MechGroup => self.switch_group_unit(actor_id, on),
            EffectProvider::AdvancedEnergyShield => self.switch_carried_shield(actor_id, on),
            EffectProvider::RvoRadiusChange => self.switch_rvo_radius_change(actor_id, on),
            EffectProvider::Repair => self.switch_repair(actor_id, on),
            EffectProvider::FlyTech => self.switch_fly(actor_id, on),
            EffectProvider::IgnoreBuff => self.switch_ignore_buff(actor_id, on),
            EffectProvider::SkillSearchTarget => self.switch_life_priority(actor_id, on),
            EffectProvider::AutoRecovery => self.switch_auto_recovery(actor_id, on),
            EffectProvider::EnergyShield => self.switch_energy_shield(actor_id, on),
            EffectProvider::SweepSkillIntensify => self.switch_sweep(actor_id, on),
            EffectProvider::InterceptMissile => self.switch_unit_interception(actor_id, on),
            EffectProvider::MoveAbilityRangeItem => self.switch_sand_fog(actor_id, on),
            EffectProvider::SupportUnit => self.switch_production(actor_id, on),
            EffectProvider::MoveAbilitySummon => self.switch_surfacing_line(actor_id, on),
            EffectProvider::MoveAbilityAttackIntensify => self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .switch_attack_count(on),
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
            | EffectProvider::DeadEffect
            | EffectProvider::DeadLine
            | EffectProvider::FireIntensify
            | EffectProvider::ClearRangeItem
            | EffectProvider::KillExplosion
            // `AdditionalDamageProvider.DoDisableEffect` takes its hit effect
            // off the skills (`SkillManager.RemoveHitEffect`), and a hit while
            // the technologies are off takes nothing besides.
            | EffectProvider::AdditionalDamage => {}
        }
        Ok(())
    }
}
