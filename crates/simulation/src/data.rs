//! A unit's numbers: one shared description, and the overlays that correct it.
//!
//! `docs/spec/simulation/architecture.md` is the contract, and this mirrors the
//! build's layering rather than inventing one. A description is shared by every
//! instance of a type and never written. A correction is an entry in an
//! overlay, addressed by an index and tagged with what wrote it, so it can be
//! taken away again without anyone recomputing a base. The build keeps two such
//! overlays per unit, one on the unit and one per skill, and aggregates active
//! buffs separately; MCFR records all three, which is why they are three here.
//!
//! [`Stats`] is the build's `FightProperty`: a derived number, computed from
//! the description and the overlays and recomputed when one of them changes.
//! Nothing in the fight reads a description directly.
//!
//! **A rate composes by summing within its channel and multiplying once**,
//! which `tests/modifier/composition.mcscript` measured against the game and
//! `docs/rules/officer_effects.md` records. Across channels a number composes
//! as its own property reads them: `DamageProperty.CalculateDamage` and
//! `MoveSpeedProperty.Refresh` sum every channel's values and enhancements and
//! multiply their remainders, which the tower-loss fights measured on a unit
//! whose damage and speed a buff corrects. A number whose property has not been
//! read is still refused when two channels correct it.

use crate::{
    Error, Result,
    rules::{AttackPath, UnitConfig},
};

/// One, in the Q32.32 fixed point a rate is stored in.
const ONE: i128 = 1 << 32;

/// Millimetres to Q32.32 metres, exact for any whole number of millimetres a
/// description quantizes to.
fn space_to_q32(millimetres: i64) -> i64 {
    i64::try_from(
        i128::from(millimetres) * ONE / i128::from(crate::rules::SPACE_UNITS_PER_METER_SCALE),
    )
    .expect("a quantized speed fits Q32.32")
}

/// Which of a unit's numbers an overlay entry corrects.
///
/// The build addresses these by the `MechDataChange*` and `SkillDataChange*`
/// enums, whose numeric indices are not read yet. These are the ones MCFR
/// records and this simulator resolves; the rest arrive with the mechanism
/// that needs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Index {
    MoveSpeed,
    MaxLife,
    AttackDamage,
    AttackInterval,
    AttackRange,
    /// The rate on the damage a unit takes: `PerformHitTargetEffect` reads a
    /// buff's `amplifyDamageRate` and scales each hit by it. It corrects no
    /// number of the description, only what reaches the unit.
    AmplifyDamage,
}

impl Index {
    /// Whether this number's property composes every channel as one
    /// aggregate: values and enhancements summed, remainders multiplied.
    /// `DamageProperty.CalculateDamage` does for damage, adding the buff's
    /// `GetDamageChangeAddRate` to the skill's rate and multiplying the two
    /// reduce rates; `MoveSpeedProperty.Refresh` does for speed, over the
    /// unit's `DataSet` and the buffs'; a hit's amplification has one source.
    const fn composes_across_channels(self) -> bool {
        matches!(
            self,
            Self::AttackDamage | Self::MoveSpeed | Self::AmplifyDamage
        )
    }

    const fn name(self) -> &'static str {
        match self {
            Self::MoveSpeed => "move speed",
            Self::MaxLife => "max life",
            Self::AttackDamage => "attack damage",
            Self::AttackInterval => "attack interval",
            Self::AttackRange => "attack range",
            Self::AmplifyDamage => "damage taken",
        }
    }
}

/// Which overlay an entry lives in.
///
/// A buff and a data change that produce the same number stay distinguishable,
/// which is the whole reason the build keeps them apart and MCFR records them
/// apart.
#[allow(
    dead_code,
    reason = "the writers arrive with the first mechanism; the reader, the \
              refusal and the invariants are tested now"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Channel {
    /// The unit's own `DataSet`.
    Unit,
    /// The skill's `DataSet`.
    Skill,
    /// The `BuffManager`'s aggregate over active buffs.
    Buff,
}

/// A correction, in the two shapes the build stores.
///
/// The build keeps them in two different classes, named after their own
/// arithmetic: `DataSet.floatDatas` is a `List<AdditiveDataFloat>` whose
/// `Refresh` sums its entries, and `DataSet.floatRateDatas` is a
/// `List<MultiplicativeDataFloat>` whose `Refresh` keeps two accumulators —
/// one reset to zero and summed, one reset to one and multiplied — with each
/// entry routed to one of them by its sign. A rate's two halves are therefore
/// not symmetric: enhancements add, impairments compound.
///
/// Both halves of a rate are stored non-negative, as MCFR stores them: the
/// native reduce getter answers the remaining multiplier and the recording
/// keeps `1 − it`, so a 30% slow is `reduce` of 0.3.
#[allow(
    dead_code,
    reason = "a mechanism writes these; the neutral case and the refusal are \
              tested now"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Correction {
    /// Q32.32 raw. `add` enhances and `reduce` impairs; one entry commonly
    /// carries one of them, and a recording's aggregate carries both.
    Rate { add: i64, reduce: i64 },
    /// The number's own units, signed. A value has one accumulator because
    /// the build gives it one: `AdditiveDataFloat` sums and clamps.
    Value(i64),
}

impl Correction {
    /// Whether this correction changes nothing.
    const fn neutral(self) -> bool {
        match self {
            Self::Rate { add, reduce } => add == 0 && reduce == 0,
            Self::Value(value) => value == 0,
        }
    }
}

/// One entry of an overlay: what it corrects, and what put it there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) index: Index,
    /// The module that wrote it, which is how it is taken away again.
    pub(crate) source: &'static str,
    pub(crate) correction: Correction,
}

/// One overlay over the shared description.
///
/// An overlay is a set of tagged entries and never a running total: adding a
/// correction and removing it restores the exact previous number, because the
/// number is recomputed from the entries rather than adjusted in place.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Overlay {
    entries: Vec<Entry>,
}

#[allow(
    dead_code,
    reason = "a mechanism writes and withdraws; both are tested now"
)]
impl Overlay {
    pub(crate) fn write(&mut self, entry: Entry) {
        self.entries.push(entry);
    }

    /// Takes away everything one module wrote.
    pub(crate) fn withdraw(&mut self, source: &str) {
        self.entries.retain(|entry| entry.source != source);
    }

    fn corrections(&self, index: Index) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(move |entry| entry.index == index)
    }

    /// What this overlay's `DataSet` holds for one number: the values summed
    /// (`AdditiveDataFloat`), the enhancements summed and the impairments
    /// compounded into one (`MultiplicativeDataFloat`), or nothing when no
    /// entry changes it. A recording stores exactly this aggregate.
    fn aggregate(&self, index: Index) -> Option<Aggregate> {
        let mut aggregate = Aggregate::default();
        let mut touched = false;
        for entry in self.corrections(index) {
            match entry.correction {
                correction if correction.neutral() => {}
                Correction::Value(add) => {
                    aggregate.value += i128::from(add);
                    touched = true;
                }
                Correction::Rate { add, reduce } => {
                    aggregate.enhance += i128::from(add);
                    if reduce != 0 {
                        aggregate.remaining =
                            aggregate.remaining * (ONE - i128::from(reduce)) / ONE;
                    }
                    touched = true;
                }
            }
        }
        touched.then_some(aggregate)
    }
}

/// One number's aggregate in one overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Aggregate {
    /// Σ value, in the number's own units.
    value: i128,
    /// Σ enhance, Q32.32.
    enhance: i128,
    /// Π (1 − impair), Q32.32.
    remaining: i128,
}

impl Default for Aggregate {
    fn default() -> Self {
        Self {
            value: 0,
            enhance: 0,
            remaining: ONE,
        }
    }
}

impl Aggregate {
    /// The rate half as a recording stores it: the enhancements' sum, and one
    /// less the compounded remainder.
    fn rate(self) -> Result<mechcore_mcfr::RateModifier> {
        Ok(mechcore_mcfr::RateModifier {
            add: i64::try_from(self.enhance)
                .map_err(|_| Error::new("an enhancement is outside the signed range"))?,
            reduce: i64::try_from(ONE - self.remaining)
                .map_err(|_| Error::new("an impairment is outside the signed range"))?,
        })
    }
}

/// The three overlays a unit carries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Overlays {
    unit: Overlay,
    skill: Overlay,
    buff: Overlay,
}

impl Overlays {
    #[allow(dead_code, reason = "a mechanism reaches for a channel to write it")]
    pub(crate) const fn channel(&mut self, channel: Channel) -> &mut Overlay {
        match channel {
            Channel::Unit => &mut self.unit,
            Channel::Skill => &mut self.skill,
            Channel::Buff => &mut self.buff,
        }
    }

    /// The description's number, with every overlay that touches it applied.
    ///
    /// The build's shape, which `docs/spec/simulation/architecture.md` derives
    /// from `DataSet`'s two aggregation classes:
    ///
    /// ```text
    /// (base + Σ value) × (1 + Σ enhance) × Π (1 − impair)
    /// ```
    ///
    /// Values sum because `AdditiveDataFloat.Refresh` sums them.
    /// Enhancements sum and impairments compound because
    /// `MultiplicativeDataFloat.Refresh` keeps one accumulator reset to zero
    /// and one reset to one, and routes each entry by its sign. The whole
    /// thing truncates toward zero once, at the end, because the build casts
    /// an `FPoint` to `Int32` there and not before.
    ///
    /// An impairment is therefore not a negative enhancement: two of `0.11`
    /// leave `0.89 × 0.89`, not `1 − 0.22`.
    ///
    /// # Errors
    ///
    /// Returns an error when one number is corrected in more than one channel
    /// and its property's composition has not been read: `AttackIntervalProperty`
    /// reads a skill's and a buff's aggregates by its own arithmetic, which is
    /// not measured, so an interval corrected twice is refused rather than
    /// assumed to compose like damage.
    fn resolve(&self, index: Index, base: i64) -> Result<i64> {
        self.resolve_scaled(index, base, |value| value)
    }

    /// [`Overlays::resolve`] for a base held in other units than its values:
    /// `value_scale` carries a summed value into the base's units.
    fn resolve_scaled(
        &self,
        index: Index,
        base: i64,
        value_scale: impl Fn(i128) -> i128,
    ) -> Result<i64> {
        let mut corrected: Option<&'static str> = None;
        let mut total = Aggregate::default();
        for (channel, overlay) in [
            ("unit", &self.unit),
            ("skill", &self.skill),
            ("buff", &self.buff),
        ] {
            let Some(aggregate) = overlay.aggregate(index) else {
                continue;
            };
            if let Some(first) = corrected
                && !index.composes_across_channels()
            {
                return Err(Error::new(format!(
                    "{} is corrected in the {first} channel and the {channel} channel \
                     at once, and what its property does with two channels' \
                     aggregates is not read: see the unresolved questions in \
                     docs/spec/simulation/architecture.md",
                    index.name()
                )));
            }
            corrected = Some(channel);
            total.value += aggregate.value;
            total.enhance += aggregate.enhance;
            total.remaining = total.remaining * aggregate.remaining / ONE;
        }
        if corrected.is_none() {
            return Ok(base);
        }
        // The rates meet first, as one FPoint factor, and the number is
        // multiplied by it once: `CalculateDamage` multiplies its summed
        // enhancement by the reduce rates before it reaches the damage.
        let factor = (ONE + total.enhance) * total.remaining / ONE;
        let scaled = (i128::from(base) + value_scale(total.value)) * factor / ONE;
        i64::try_from(scaled).map_err(|_| {
            Error::new(format!(
                "{} resolved outside the range a number can hold",
                index.name()
            ))
        })
    }
}

/// A unit's derived numbers, which is what the fight reads.
///
/// This is the build's `FightProperty` layer: each number is computed from the
/// description and the overlays, and [`Stats::refresh`] recomputes them when an
/// overlay changes. A cache that is dropped and recomputed at any moment gives
/// the same number, so caching cannot change a result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stats {
    pub(crate) overlays: Overlays,
    /// The unit's level, which is its `IMechLevelData` rating: base life and
    /// base damage are the description's times it, before any overlay.
    level: i64,
    /// Q32.32 metres a second: `MoveSpeedProperty` keeps the speed as an
    /// `FPoint`, and a rate on it lands between two millimetres.
    move_speed_q32: i64,
    max_life: i64,
    attack_damage: i64,
    attack_interval: u64,
    attack_range: i64,
}

impl Stats {
    /// The numbers a description alone gives, with no correction on them.
    ///
    /// # Errors
    ///
    /// Cannot fail while the overlays are empty; it answers a `Result` because
    /// [`Stats::refresh`] does.
    #[cfg(test)]
    pub(crate) fn of(rules: &UnitConfig) -> Result<Self> {
        Self::at_level(rules, 1)
    }

    /// The numbers a description gives at a level, with no correction on them.
    ///
    /// # Errors
    ///
    /// See [`Stats::of`].
    pub(crate) fn at_level(rules: &UnitConfig, level: i64) -> Result<Self> {
        let mut stats = Self {
            overlays: Overlays::default(),
            level,
            move_speed_q32: 0,
            max_life: 0,
            attack_damage: 0,
            attack_interval: 0,
            attack_range: 0,
        };
        stats.refresh(rules)?;
        Ok(stats)
    }

    /// The numbers a description gives once a loadout has corrected them.
    ///
    /// # Errors
    ///
    /// Returns whatever [`Overlays::resolve`] refuses, which is what makes a
    /// correction this build cannot compose a refusal rather than a number.
    pub(crate) fn corrected(
        rules: &UnitConfig,
        level: i64,
        written: &[(Channel, Entry)],
    ) -> Result<Self> {
        let mut stats = Self::at_level(rules, level)?;
        for (channel, entry) in written {
            stats.overlays.channel(*channel).write(entry.clone());
        }
        stats.refresh(rules)?;
        Ok(stats)
    }

    /// Recomputes every derived number from the description, the level and
    /// the overlays.
    ///
    /// The level is its own multiplier, not a correction: `FightMech`'s
    /// `GetBaseLife` and `GetBaseDamage` multiply the description by the
    /// level's rating and hand the product to the properties, which then
    /// apply the `DataSet`s. Build 2259's nine `attributeUpgradeDatas` rows
    /// rate life and damage at exactly their level, so the rating is the
    /// level.
    ///
    /// # Errors
    ///
    /// Returns an error when an overlay carries a correction that is not
    /// neutral; see [`Overlays::resolve`].
    pub(crate) fn refresh(&mut self, rules: &UnitConfig) -> Result<()> {
        let resolve = |index, base| self.overlays.resolve(index, base);
        // A value is whole millimetres; the speed is resolved in Q32.32.
        let metres = i128::from(crate::rules::SPACE_UNITS_PER_METER_SCALE);
        self.move_speed_q32 = self.overlays.resolve_scaled(
            Index::MoveSpeed,
            space_to_q32(rules.move_speed()),
            |value| value * ONE / metres,
        )?;
        self.max_life = resolve(Index::MaxLife, self.base(rules.max_life)?)?;
        self.attack_damage = resolve(Index::AttackDamage, self.base(rules.attack.base_damage)?)?;
        self.attack_interval = u64::try_from(resolve(
            Index::AttackInterval,
            i64::try_from(rules.attack.interval_time_units())
                .map_err(|_| Error::new("attack interval is outside the signed range"))?,
        )?)
        .map_err(|_| Error::new("attack interval resolved below zero"))?;
        self.attack_range = resolve(Index::AttackRange, rules.attack.range())?;
        Ok(())
    }

    /// A base number at this unit's level.
    fn base(&self, description: i64) -> Result<i64> {
        description
            .checked_mul(self.level)
            .ok_or_else(|| Error::new("a level-scaled base is outside the signed range"))
    }

    /// Q32.32 metres a second.
    pub(crate) const fn move_speed_q32(&self) -> i64 {
        self.move_speed_q32
    }

    pub(crate) const fn max_life(&self) -> i64 {
        self.max_life
    }

    pub(crate) const fn attack_damage(&self) -> i64 {
        self.attack_damage
    }

    /// The laser's base damage is truncated after its ramp multiplier, before
    /// the dynamic damage rates. `DamageProperty.CalculateBaseDamage` and
    /// `CalculateDamage` are separate stages in build 2259.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "preserve the existing native laser ramp's float truncation"
    )]
    pub(crate) fn laser_damage(&self, rules: &UnitConfig, attack_count: usize) -> i64 {
        let AttackPath::Laser { damage_multipliers } = &rules.attack.path else {
            unreachable!("laser damage requires the laser attack path")
        };
        let base = self
            .base(rules.attack.base_damage)
            .expect("the layout verified the level-scaled base");
        let multiplier = damage_multipliers[attack_count.min(damage_multipliers.len() - 1)];
        let ramped = (base as f64 * multiplier).trunc() as i64;
        self.overlays
            .resolve(Index::AttackDamage, ramped)
            .expect("the layout verified the damage corrections")
    }

    /// The unit's own `DataSet` as a recording stores it: life's rate, move
    /// speed's value in whole metres, and move speed's rate, each the
    /// aggregate its field keeps.
    ///
    /// # Errors
    ///
    /// Returns an error for a correction the unit `DataSet` has no field for.
    pub(crate) fn unit_dynamic_modifiers(&self) -> Result<mechcore_mcfr::UnitDynamicModifierSet> {
        let unit = &self.overlays.unit;
        let mut set = mechcore_mcfr::UnitDynamicModifierSet::default();
        if let Some(life) = unit.aggregate(Index::MaxLife) {
            if life.value != 0 {
                return Err(Error::new("the unit DataSet has no field for a life value"));
            }
            set.life_rate = life.rate()?;
        }
        if let Some(speed) = unit.aggregate(Index::MoveSpeed) {
            // `DataSet.intDatas`: whole metres, which is how the officer's
            // integer reaches the recording (3 for one Advanced Power System,
            // 6 for two, in `tests/modifier/`).
            let metres = i128::from(crate::rules::SPACE_UNITS_PER_METER_SCALE);
            if speed.value % metres != 0 {
                return Err(Error::new(
                    "a unit's move-speed value is a whole number of metres",
                ));
            }
            set.move_speed_value = i32::try_from(speed.value / metres)
                .map_err(|_| Error::new("a unit's move-speed value is outside i32"))?;
            set.move_speed_change_rate = speed.rate()?;
        }
        for index in [
            Index::AttackDamage,
            Index::AttackInterval,
            Index::AttackRange,
            Index::AmplifyDamage,
        ] {
            if unit.aggregate(index).is_some() {
                return Err(Error::new(format!(
                    "the unit DataSet has no field for {}",
                    index.name()
                )));
            }
        }
        Ok(set)
    }

    /// The skill's `DataSet` as a recording stores it, one entry per skill
    /// slot, or none when the skill channel holds nothing.
    ///
    /// Each number the skill channel corrects lands in the field the build
    /// keeps its aggregate in: damage's rate, the attack range's value and
    /// rate, the attack interval's value and rate. A value is the number's own
    /// units here and Q32.32 metres or seconds in the recording. Level ratings
    /// are in the base channel and never appear here.
    ///
    /// # Errors
    ///
    /// Returns an error for a correction the skill `DataSet` has no field
    /// for, rather than dropping it from what the recording is compared with.
    pub(crate) fn skill_dynamic_modifiers(
        &self,
        slots: usize,
    ) -> Result<Vec<mechcore_mcfr::SkillNumericModifierState>> {
        let skill = &self.overlays.skill;
        let mut set = mechcore_mcfr::SkillDynamicModifierSet::default();
        let q32 = |value: i128, units_per_one: i128| {
            i64::try_from(value * ONE / units_per_one)
                .map_err(|_| Error::new("a skill value is outside the signed range"))
        };
        if let Some(damage) = skill.aggregate(Index::AttackDamage) {
            if damage.value != 0 {
                return Err(Error::new(
                    "the skill DataSet has no field for a damage value",
                ));
            }
            set.damage_rate = damage.rate()?;
        }
        if let Some(range) = skill.aggregate(Index::AttackRange) {
            set.attack_range_value = q32(
                range.value,
                i128::from(crate::rules::SPACE_UNITS_PER_METER_SCALE),
            )?;
            set.attack_range_rate = range.rate()?;
        }
        if let Some(interval) = skill.aggregate(Index::AttackInterval) {
            set.attack_interval_value = q32(
                interval.value,
                i128::from(crate::rules::TIME_UNITS_PER_SECOND_SCALE),
            )?;
            set.attack_interval_rate = interval.rate()?;
        }
        for index in [Index::MoveSpeed, Index::MaxLife, Index::AmplifyDamage] {
            if skill.aggregate(index).is_some() {
                return Err(Error::new(format!(
                    "the skill DataSet has no field for {}",
                    index.name()
                )));
            }
        }
        if set.is_zero() {
            return Ok(Vec::new());
        }
        Ok((0..slots)
            .map(|slot| mechcore_mcfr::SkillNumericModifierState {
                skill_slot: u16::try_from(slot).expect("validated skill count fits u16"),
                modifiers: set,
            })
            .collect())
    }

    /// The buffs' aggregate as a recording stores it: move speed's rate,
    /// damage's rate and the rate on damage taken.
    ///
    /// # Errors
    ///
    /// Returns an error for a buff correction the recording has no field
    /// for, rather than dropping it from what the recording is compared with.
    pub(crate) fn buff_modifiers(&self) -> Result<mechcore_mcfr::BuffModifierSet> {
        let buff = &self.overlays.buff;
        let mut set = mechcore_mcfr::BuffModifierSet::default();
        for (index, field) in [
            (Index::MoveSpeed, &mut set.move_speed_rate),
            (Index::AttackDamage, &mut set.damage_rate),
            (Index::AmplifyDamage, &mut set.amplify_damage_rate),
        ] {
            if let Some(aggregate) = buff.aggregate(index) {
                if aggregate.value != 0 {
                    return Err(Error::new(format!(
                        "a buff's {} value is not a field this build records",
                        index.name()
                    )));
                }
                *field = aggregate.rate()?;
            }
        }
        for index in [Index::MaxLife, Index::AttackInterval, Index::AttackRange] {
            if buff.aggregate(index).is_some() {
                return Err(Error::new(format!(
                    "no buff here corrects {}",
                    index.name()
                )));
            }
        }
        Ok(set)
    }

    /// What one hit of `amount` takes off this unit: the amount scaled by
    /// the rate on damage taken and truncated once, as `PerformHitTargetEffect`
    /// scales it by `GetBuffAmplifyDamageAddRate`.
    ///
    /// # Errors
    ///
    /// Returns an error when the scaled hit leaves the signed range.
    pub(crate) fn damage_taken(&self, amount: i64) -> Result<i64> {
        self.overlays.resolve(Index::AmplifyDamage, amount)
    }

    pub(crate) const fn attack_interval(&self) -> u64 {
        self.attack_interval
    }

    pub(crate) const fn attack_range(&self) -> i64 {
        self.attack_range
    }
}

#[cfg(test)]
mod tests {
    use super::{Channel, Correction, Entry, Index, Stats};
    use crate::rules::SimulationConfig;

    fn marksman() -> crate::rules::UnitConfig {
        SimulationConfig::load()
            .unwrap()
            .units
            .get("marksman")
            .unwrap()
            .clone()
    }

    /// With nothing written, every derived number is the description's own.
    /// This is what keeps the recordings identical while the layer is empty.
    #[test]
    fn an_empty_overlay_answers_the_description() {
        let rules = marksman();
        let stats = Stats::of(&rules).unwrap();
        assert_eq!(
            stats.move_speed_q32(),
            super::space_to_q32(rules.move_speed())
        );
        assert_eq!(stats.max_life(), rules.max_life);
        assert_eq!(stats.attack_damage(), rules.attack.base_damage);
        assert_eq!(stats.attack_interval(), rules.attack.interval_time_units());
        assert_eq!(stats.attack_range(), rules.attack.range());
    }

    /// A level multiplies the description's life and damage, and nothing
    /// else, before any overlay: `tests/level/`'s recordings read 3244 and
    /// 4658 at level 2, 4866 and 6987 at level 3, and move speed, interval and
    /// range unchanged.
    #[test]
    fn a_level_multiplies_base_life_and_damage() {
        let rules = marksman();
        for (level, life, damage) in [(2, 3244, 4658), (3, 4866, 6987)] {
            let stats = Stats::at_level(&rules, level).unwrap();
            assert_eq!((stats.max_life(), stats.attack_damage()), (life, damage));
            assert_eq!(
                stats.move_speed_q32(),
                super::space_to_q32(rules.move_speed())
            );
            assert_eq!(stats.attack_interval(), rules.attack.interval_time_units());
            assert_eq!(stats.attack_range(), rules.attack.range());
        }
    }

    /// The level is its own multiplier, applied before the `DataSet`s: a
    /// level-2 Marksman under Advanced Offensive Tactics shoots 6055, which is
    /// `4658 × 1.3` truncated once. Were the level a rate among the officer's,
    /// it would be `2329 × (1 + 1 + 0.3)`, 5356; the recording is not.
    #[test]
    fn a_level_is_applied_before_the_overlays() {
        let rules = marksman();
        let officer = Entry {
            index: Index::AttackDamage,
            source: "Modifier",
            correction: Correction::Rate {
                add: THIRTY_PERCENT,
                reduce: 0,
            },
        };
        let stats = Stats::corrected(&rules, 2, &[(Channel::Skill, officer)]).unwrap();
        assert_eq!(stats.attack_damage(), 6055);
    }

    /// Advanced Offensive Tactics' `+0.3`, in the Q32.32 raw the build stores
    /// and `config/officer_effects.yaml` carries.
    const THIRTY_PERCENT: i64 = 1_288_490_188;

    /// The capture, replayed against this layer.
    ///
    /// `tests/modifier/composition.mcscript` recorded one Marksman shooting
    /// one Rhino under no officer, one and two, and the game's own damage was
    /// 2329, 3027 and 3726. Two officers of one kind reach the recording as a
    /// single `+0.6`, so they sum and multiply once rather than compounding —
    /// compounding would be 3935, which the recording is not.
    #[test]
    fn a_rate_sums_within_its_channel_and_multiplies_once() {
        let rules = marksman();
        let mut stats = Stats::of(&rules).unwrap();
        assert_eq!(
            rules.attack.base_damage, 2329,
            "the Marksman the capture shot with"
        );

        let officer = Entry {
            index: Index::AttackDamage,
            source: "Modifier",
            correction: Correction::Rate {
                add: THIRTY_PERCENT,
                reduce: 0,
            },
        };
        stats
            .overlays
            .channel(Channel::Skill)
            .write(officer.clone());
        stats.refresh(&rules).unwrap();
        assert_eq!(stats.attack_damage(), 3027);

        stats.overlays.channel(Channel::Skill).write(officer);
        stats.refresh(&rules).unwrap();
        assert_eq!(stats.attack_damage(), 3726);
    }

    /// A correction that changes nothing needs no composition rule at all.
    #[test]
    fn a_neutral_correction_leaves_the_description_alone() {
        let rules = marksman();
        let mut stats = Stats::of(&rules).unwrap();
        stats.overlays.channel(Channel::Buff).write(Entry {
            index: Index::MoveSpeed,
            source: "Modifier",
            correction: Correction::Rate { add: 0, reduce: 0 },
        });
        stats.refresh(&rules).unwrap();
        assert_eq!(
            stats.move_speed_q32(),
            super::space_to_q32(rules.move_speed())
        );
    }

    /// A value is added to the description in its own units, before any rate
    /// multiplies it, and two values add.
    #[test]
    fn values_add_to_the_description_before_a_rate_multiplies_it() {
        let rules = marksman();
        let mut stats = Stats::of(&rules).unwrap();
        let base = rules.attack.range();
        for _ in 0..2 {
            stats.overlays.channel(Channel::Skill).write(Entry {
                index: Index::AttackRange,
                source: "Modifier",
                correction: Correction::Value(10_000),
            });
        }
        stats.refresh(&rules).unwrap();
        assert_eq!(stats.attack_range(), base + 20_000, "ten metres, twice");

        stats.overlays.channel(Channel::Skill).write(Entry {
            index: Index::AttackRange,
            source: "Modifier",
            correction: Correction::Rate {
                add: THIRTY_PERCENT,
                reduce: 0,
            },
        });
        stats.refresh(&rules).unwrap();
        let expected = i64::try_from(
            (i128::from(base + 20_000) * (i128::from(THIRTY_PERCENT) + (1 << 32))) >> 32,
        )
        .unwrap();
        assert_eq!(stats.attack_range(), expected, "the rate takes the sum");
    }

    /// An impairment is not a negative enhancement: two of them compound,
    /// because the build keeps one accumulator reset to one and multiplies
    /// into it.
    #[test]
    fn impairments_compound_and_enhancements_sum() {
        let rules = marksman();
        let base = rules.attack.base_damage;
        let eleven_percent = 472_446_402_i64;
        let impair = Entry {
            index: Index::AttackDamage,
            source: "Modifier",
            correction: Correction::Rate {
                add: 0,
                reduce: eleven_percent,
            },
        };

        let mut once = Stats::of(&rules).unwrap();
        once.overlays.channel(Channel::Skill).write(impair.clone());
        once.refresh(&rules).unwrap();
        assert_eq!(once.attack_damage(), 2072, "2329 x 0.89");

        let mut twice = Stats::of(&rules).unwrap();
        for _ in 0..2 {
            twice.overlays.channel(Channel::Skill).write(impair.clone());
        }
        twice.refresh(&rules).unwrap();
        assert_eq!(twice.attack_damage(), 1844, "2329 x 0.89 x 0.89");
        let summed = i64::try_from(
            (i128::from(base) * ((1_i128 << 32) - 2 * i128::from(eleven_percent))) >> 32,
        )
        .unwrap();
        assert_eq!(summed, 1816, "and not 2329 x (1 - 0.22)");
    }

    /// An interval's property reads a skill's and a buff's aggregates by an
    /// arithmetic nobody has measured, so two channels on it are refused.
    #[test]
    fn two_channels_on_an_unread_property_are_refused() {
        let rules = marksman();
        let mut stats = Stats::of(&rules).unwrap();
        for channel in [Channel::Skill, Channel::Buff] {
            stats.overlays.channel(channel).write(Entry {
                index: Index::AttackInterval,
                source: "Modifier",
                correction: Correction::Rate {
                    add: THIRTY_PERCENT,
                    reduce: 0,
                },
            });
        }
        let refused = stats.refresh(&rules).unwrap_err().to_string();
        assert!(refused.contains("attack interval"), "{refused}");
        assert!(
            refused.contains("skill channel and the buff channel"),
            "{refused}"
        );
    }

    /// Damage composes across channels as it does within one: an officer's
    /// `+0.3` in the skill channel and a lost tower's `-0.9` in the buff
    /// channel meet as one factor, `1.3 × 0.1`, before the damage is
    /// multiplied, as `DamageProperty.CalculateDamage` computes it.
    #[test]
    fn damage_composes_across_channels_as_one_factor() {
        let rules = marksman();
        let mut stats = Stats::of(&rules).unwrap();
        stats.overlays.channel(Channel::Skill).write(Entry {
            index: Index::AttackDamage,
            source: "Modifier",
            correction: Correction::Rate {
                add: THIRTY_PERCENT,
                reduce: 0,
            },
        });
        stats.overlays.channel(Channel::Buff).write(Entry {
            index: Index::AttackDamage,
            source: "BuffSystem",
            correction: Correction::Rate {
                add: 0,
                reduce: 3_865_470_566,
            },
        });
        stats.refresh(&rules).unwrap();
        // 2329 × trunc_q32(1.3 × 0.1) = 302.77…
        assert_eq!(stats.attack_damage(), 302);
    }

    /// An overlay is a set of tagged entries and not a running total, so what
    /// one module wrote is exactly what taking it away removes.
    #[test]
    fn withdrawing_a_module_restores_the_number_it_found() {
        let rules = marksman();
        let mut stats = Stats::of(&rules).unwrap();
        let before = stats.clone();
        for channel in [Channel::Unit, Channel::Skill, Channel::Buff] {
            stats.overlays.channel(channel).write(Entry {
                index: Index::AttackRange,
                source: "CommanderSkillSystem",
                correction: Correction::Value(40),
            });
        }
        assert!(stats.refresh(&rules).is_err());
        for channel in [Channel::Unit, Channel::Skill, Channel::Buff] {
            stats
                .overlays
                .channel(channel)
                .withdraw("CommanderSkillSystem");
        }
        stats.refresh(&rules).unwrap();
        assert_eq!(stats, before);
    }
}
