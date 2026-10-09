//! `AttackCountEffectLinker`: what an `IAttackCountEffect` does to the first
//! attacks of its unit's main skill while a condition holds.
//!
//! `MoveAbilityAttackIntensifyProvider.DoActive` hands its unit's main skill
//! a linker (`FightSkill.AddAttackCountEffectLinker`) and registers its
//! condition with the unit's `UndergroundMoveAbility`: the end of a
//! surfacing (`OnExitMoveEnd`) puts it in condition, the start of a burrow
//! (`OnEnterMoveBegin`) takes it out. `ConditionChange` counts the skill's
//! attacks from zero again either way, and taking it out takes its effect
//! away. `SkillAttackController.PerformAttack` begins with `TryEffect`,
//! which counts the attack and, in condition, writes the effect onto the
//! main skill while the count is within the trigger count, and takes it
//! away once past it: the blow that `PerformAttack` goes on to deal reads
//! it. The effect is a rate on the skill's damage
//! (`SkillDataChangeFloatRate.DamageRate`) and metres on its splash
//! (`SkillDataChangeFloat.SplashRangeValue`); the flag it sets on the
//! unit's `attackParam` (`IsMoveAbilityStrengthAttack`) is read only by the
//! client.

use super::*;
use crate::data::{Channel, Correction, Entry, Index};
use crate::modifier::MoveAbilityAttack;

/// What the linker writes its entries under, which is how it takes them
/// away again.
const SOURCE: &str = "AttackCountEffectLinker";

/// One main skill's `AttackCountEffectLinker`.
#[derive(Debug, Clone, Copy)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a field of `AttackCountEffectLinker`"
)]
pub(in crate::fight) struct AttackCountLinker {
    source: MoveAbilityAttack,
    /// `isEnable`, cleared while its technology is switched off.
    enabled: bool,
    /// `isCondition`, false from the fight's start (`ResetData`).
    condition: bool,
    /// `hasEffect`: whether its entries are written.
    has_effect: bool,
    /// `attackCount`: the attacks since the condition last changed.
    attack_count: i32,
}

impl AttackCountLinker {
    /// The linker a unit's provider hands its main skill:
    /// `IsAvailableMoveAbility` lets it only on a unit with an
    /// `UndergroundMoveAbility`.
    pub(in crate::fight) fn of(
        rules: &crate::rules::UnitConfig,
        placement: &crate::layout::Placement,
    ) -> Option<Self> {
        let source = placement.effects.move_ability_attack?;
        rules.underground.as_ref()?;
        Some(Self {
            source,
            enabled: true,
            condition: false,
            has_effect: false,
            attack_count: 0,
        })
    }
}

impl Actor {
    /// `AttackCountEffectLinker.ConditionChange`, which
    /// `MoveAbilityAttackIntensifyProvider.EnterCondition` and
    /// `ExitCondition` reach through the main skill's
    /// `SetAttackCountEffectLinkerCondition`. Out of condition it is also
    /// what `ResetData` leaves, as `FightSkill.ExitFight` resets it: no
    /// count, no effect.
    pub(in crate::fight) fn attack_count_condition(&mut self, condition: bool) {
        let Some(linker) = self.skills.main.attack_count_linker.as_mut() else {
            return;
        };
        let remove = !condition && linker.has_effect;
        linker.attack_count = 0;
        linker.condition = condition;
        if remove {
            self.remove_attack_count_effect();
        }
    }

    /// `AttackCountEffectLinker.TryEffect`, at the start of
    /// `SkillAttackController.PerformAttack`.
    pub(in crate::fight) fn try_attack_count_effect(&mut self) {
        let Some(linker) = self.skills.main.attack_count_linker.as_mut() else {
            return;
        };
        linker.attack_count += 1;
        if !(linker.enabled && linker.condition) {
            return;
        }
        if linker.source.trigger_count < linker.attack_count {
            if linker.has_effect {
                self.remove_attack_count_effect();
            }
            return;
        }
        if linker.has_effect {
            return;
        }
        linker.has_effect = true;
        let source = linker.source;
        let splash = q32_mul(
            source.splash_range_q32,
            crate::rules::SPACE_UNITS_PER_METER_SCALE << 32,
        ) >> 32;
        let skill = self.stats.overlays.channel(Channel::Skill);
        skill.write(Entry {
            index: Index::AttackDamage,
            source: SOURCE,
            correction: super::tower::rate(source.damage_rate_q32),
        });
        skill.write(Entry {
            index: Index::SplashRange,
            source: SOURCE,
            correction: Correction::Value(splash),
        });
        self.stats
            .refresh(&self.rules)
            .expect("a rate on damage and metres on splash resolve");
    }

    /// `MoveAbilityAttackIntensifyProvider.DisableEffect` and `EnableEffect`,
    /// through the main skill's `SetAttackCountEffectLinkerEnable`: off, the
    /// effect is taken away if it is written; either way `isEnable` follows,
    /// and the count goes on (`TryEffect` counts before it asks).
    pub(in crate::fight) fn switch_attack_count(&mut self, on: bool) {
        let Some(linker) = self.skills.main.attack_count_linker.as_mut() else {
            return;
        };
        let remove = !on && linker.has_effect;
        linker.enabled = on;
        if remove {
            self.remove_attack_count_effect();
        }
    }

    /// `AttackCountEffectLinker.RemoveEffect`.
    fn remove_attack_count_effect(&mut self) {
        if let Some(linker) = self.skills.main.attack_count_linker.as_mut() {
            linker.has_effect = false;
        }
        self.stats.overlays.channel(Channel::Skill).withdraw(SOURCE);
        self.stats
            .refresh(&self.rules)
            .expect("the numbers resolved before the effect was written");
    }
}
