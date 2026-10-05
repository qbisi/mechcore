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
//! which `tests/modifier/fights/officer-composition-*.yaml` pin from the game and
//! `docs/rules/officer_effects.md` records. Across channels a number composes
//! as its own property reads them: `DamageProperty.CalculateDamage` and
//! `MoveSpeedProperty.Refresh` sum every channel's values and enhancements and
//! multiply their remainders, which the tower-loss fights measured on a unit
//! whose damage and speed a buff corrects. A number whose property has not been
//! read is still refused when two channels correct it.

use crate::{
    Error, Result,
    rules::{AttackPath, AttackTargets, UnitConfig, UnitDomain},
};
use mechcore_mcfr::{Modifier, ModifierChannel, ModifierPart};

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

/// Time units to Q32.32 seconds, as a description's interval becomes the
/// `FPoint` `AttackIntervalProperty` holds.
fn time_to_q32(time_units: i64) -> i64 {
    i64::try_from(
        i128::from(time_units) * ONE / i128::from(crate::rules::TIME_UNITS_PER_SECOND_SCALE),
    )
    .expect("a quantized interval fits Q32.32")
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
    /// `SkillDataChangeFloat.SplashRangeValue`: `FightSkill.GetSplashRange`
    /// adds the skill `DataSet`'s value to the skill row's `splashRange`.
    SplashRange,
    /// The rate on the damage a unit takes: `PerformHitTargetEffect` reads a
    /// buff's `amplifyDamageRate` and scales each hit by it. It corrects no
    /// number of the description, only what reaches the unit.
    AmplifyDamage,
    /// `SkillDataChangeFloatRate.DamageRateByKillCount`: what each of the
    /// skill's kills adds to its damage's enhancement. It corrects no number
    /// on its own; `DamageProperty.CalculateDamage` multiplies its enhancement
    /// by the skill's `DamageCalculator.killCount`.
    DamagePerKill,
    /// `MechDataChangeFloat` 0, `gf_range_value`: the radius of the fire an
    /// extra weapon's hit leaves, which `ExtraSkillProvider.AddEffect` writes
    /// onto its unit and `GroundFireController.GetFireMech` reads back. Q32.32
    /// metres.
    GroundFireRange,
    /// `MechDataChangeFloat` 1, `gf_life_time_value`: how long that fire
    /// burns. Q32.32 seconds.
    GroundFireLifeTime,
    /// `MechDataChangeInt.ReduceDamageValue`, `reduce_damage_value`: what
    /// each hit on the unit loses, which `ArmorStrengthenEffectProvider`
    /// writes onto its unit and `PerformHitTargetEffect` reads back. Whole
    /// damage.
    ReduceDamage,
    /// `SkillDataChangeFloatRate.DamageReduceRateBase`: a rate on the skill's
    /// damage of its own, which `DamageProperty.CalculateDamage` multiplies
    /// in after the damage rates have met. An extra weapon technology's
    /// `allWeaponReduceDamageRate` writes it (`ExtraSkillProvider.EnableEffect`).
    DamageReduceRateBase,
    /// `SkillDataChangeFloat.AttackAirRangeAddValue` and
    /// `AttackGroundRangeAddValue`: what `AttackRangeAirProperty` and
    /// `AttackRangeGroundProperty` add to the skill's range, the one
    /// `FightSkill.GetAttackRange` answers while it locks a unit that flies
    /// and the other otherwise. Q32.32 metres in the build, millimetres here
    /// as the range is.
    RangeAgainst(UnitDomain),
    /// `SkillDataChangeInt.AttackRangeValueAir` and `AttackRangeValueGround`:
    /// whole metres, which a skill searching by `DistanceIntensify` counts
    /// off a candidate of that domain's distance
    /// (`SearchTargetController.SetTargetSelector`). Millimetres here.
    ScoreOffsetFor(UnitDomain),
    /// `SkillDataChangeFloat.DamageChangeRateAir` and `DamageChagneRateGround`:
    /// the rate `AirDamageProperty` and `GroundDamageProperty` hand
    /// `DamageProperty.CalculateDamage` as its extra enhancement, the one
    /// `DamageCalculator.GetAttackDamage` reads while the skill attacks a unit
    /// that flies and the other otherwise. A plain value, Q32.32.
    DamageRateAgainst(UnitDomain),
    /// `SkillDataChangeInt.AirAttackValue` and `GroundAttackValue`: what
    /// `FightSkill.IsAirAttack` and `IsGroundAttack` add to the row's flag,
    /// the skill attacking that domain while the sum is above zero. A whole
    /// number.
    AttackValueFor(UnitDomain),
    /// `SkillDataChangeFloat.ProjectileSpeedValue`: what the skill's
    /// projectiles' speed gains. Q32.32 metres a second in the build,
    /// millimetres a second here as the speed is.
    ProjectileSpeed,
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
            Self::SplashRange => "splash range",
            Self::AmplifyDamage => "damage taken",
            Self::DamagePerKill => "damage per kill",
            Self::GroundFireRange => "ground fire range",
            Self::GroundFireLifeTime => "ground fire life time",
            Self::ReduceDamage => "damage reduction",
            Self::DamageReduceRateBase => "damage reduce rate base",
            Self::RangeAgainst(UnitDomain::Air) => "attack range against an aerial unit",
            Self::RangeAgainst(UnitDomain::Ground) => "attack range against a ground unit",
            Self::ScoreOffsetFor(UnitDomain::Air) => "search offset for an aerial unit",
            Self::ScoreOffsetFor(UnitDomain::Ground) => "search offset for a ground unit",
            Self::DamageRateAgainst(UnitDomain::Air) => "damage rate against an aerial unit",
            Self::DamageRateAgainst(UnitDomain::Ground) => "damage rate against a ground unit",
            Self::AttackValueFor(UnitDomain::Air) => "air attack",
            Self::AttackValueFor(UnitDomain::Ground) => "ground attack",
            Self::ProjectileSpeed => "projectile speed",
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
    /// The number's own units, signed, and Q32.32 seconds for an interval. A
    /// value has one accumulator because the build gives it one:
    /// `AdditiveDataFloat` sums and clamps.
    Value(i64),
}

impl Correction {
    /// Whether this correction changes nothing.
    pub(crate) const fn neutral(self) -> bool {
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
    /// An overlay of these entries alone.
    pub(crate) fn of(entries: &[Entry]) -> Self {
        Self {
            entries: entries.to_vec(),
        }
    }

    pub(crate) fn write(&mut self, entry: Entry) {
        self.entries.push(entry);
    }

    /// The values it sums for one number.
    pub(crate) fn value(&self, index: Index) -> i64 {
        self.aggregate(index).map_or(0, |aggregate| {
            i64::try_from(aggregate.value).expect("the layout verified the skill's values")
        })
    }

    /// Takes away everything one module wrote.
    pub(crate) fn withdraw(&mut self, source: &str) {
        self.entries.retain(|entry| entry.source != source);
    }

    /// Takes away everything one module wrote, handing it back.
    pub(crate) fn take(&mut self, source: &str) -> Vec<Entry> {
        let (taken, kept) = std::mem::take(&mut self.entries)
            .into_iter()
            .partition(|entry| entry.source == source);
        self.entries = kept;
        taken
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
    fn rate(self) -> Result<(i64, i64)> {
        Ok((
            i64::try_from(self.enhance)
                .map_err(|_| Error::new("an enhancement is outside the signed range"))?,
            i64::try_from(ONE - self.remaining)
                .map_err(|_| Error::new("an impairment is outside the signed range"))?,
        ))
    }
}

/// Damage, its kill-count rate and its reduce rate base, which a skill's
/// `DataSet` keeps as rates alone, and the field each is recorded in.
const DAMAGE_RATES: [(Index, &str); 3] = [
    (Index::AttackDamage, "damage_rate"),
    (Index::DamagePerKill, "damage_rate_by_kill_count"),
    (Index::DamageReduceRateBase, "damage_reduce_rate_base"),
];

/// The skill numbers its `DataSet` keeps as values alone, the channel and
/// field each is recorded in, and how the overlay's units are written there:
/// Q32.32 metres for a range or a speed, whole metres for a search offset,
/// and as they are for a damage rate or a switch.
const SKILL_VALUES: [(Index, ModifierChannel, &str, Recorded); 9] = [
    (
        Index::RangeAgainst(UnitDomain::Air),
        ModifierChannel::SkillFloat,
        "attack_air_range_add_value",
        Recorded::Q32Metres,
    ),
    (
        Index::RangeAgainst(UnitDomain::Ground),
        ModifierChannel::SkillFloat,
        "attack_ground_range_add_value",
        Recorded::Q32Metres,
    ),
    (
        Index::ScoreOffsetFor(UnitDomain::Air),
        ModifierChannel::SkillInt,
        "attack_range_value_air",
        Recorded::WholeMetres,
    ),
    (
        Index::ScoreOffsetFor(UnitDomain::Ground),
        ModifierChannel::SkillInt,
        "attack_range_value_ground",
        Recorded::WholeMetres,
    ),
    (
        Index::DamageRateAgainst(UnitDomain::Air),
        ModifierChannel::SkillFloat,
        "damage_change_rate_air",
        Recorded::Raw,
    ),
    // The build's own spelling.
    (
        Index::DamageRateAgainst(UnitDomain::Ground),
        ModifierChannel::SkillFloat,
        "damage_chagne_rate_ground",
        Recorded::Raw,
    ),
    (
        Index::AttackValueFor(UnitDomain::Air),
        ModifierChannel::SkillInt,
        "air_attack_value",
        Recorded::Raw,
    ),
    (
        Index::AttackValueFor(UnitDomain::Ground),
        ModifierChannel::SkillInt,
        "ground_attack_value",
        Recorded::Raw,
    ),
    (
        Index::ProjectileSpeed,
        ModifierChannel::SkillFloat,
        "projectile_speed_value",
        Recorded::Q32Metres,
    ),
];

/// How a skill value held here is written in a recording.
#[derive(Clone, Copy)]
enum Recorded {
    /// Millimetres as Q32.32 metres, a `DataSet.floatDatas` distance.
    Q32Metres,
    /// Millimetres as whole metres, a `DataSet.intDatas` distance.
    WholeMetres,
    /// As it is.
    Raw,
}

/// What a skill overlay may carry that its `DataSet` has no field for.
fn refuse_unrecorded_skill_fields(skill: &Overlay) -> Result<()> {
    for index in [
        Index::MoveSpeed,
        Index::MaxLife,
        Index::AmplifyDamage,
        Index::GroundFireRange,
        Index::GroundFireLifeTime,
        Index::ReduceDamage,
    ] {
        if skill.aggregate(index).is_some() {
            return Err(Error::new(format!(
                "the skill DataSet has no field for {}",
                index.name()
            )));
        }
    }
    for (index, _) in DAMAGE_RATES {
        if let Some(damage) = skill.aggregate(index)
            && damage.value != 0
        {
            return Err(Error::new(format!(
                "the skill DataSet has no field for a value of {}",
                index.name()
            )));
        }
    }
    for (index, ..) in SKILL_VALUES {
        if let Some(value) = skill.aggregate(index)
            && value.rate()? != (0, 0)
        {
            return Err(Error::new(format!(
                "the skill DataSet has no field for a rate of {}",
                index.name()
            )));
        }
    }
    // `SkillDataChangeFloatRate` has no member for a splash.
    if let Some(splash) = skill.aggregate(Index::SplashRange)
        && splash.rate()? != (0, 0)
    {
        return Err(Error::new(
            "the skill DataSet has no field for a splash rate",
        ));
    }
    Ok(())
}

/// One skill's `DataSet` as a recording stores it, under its slot of
/// `FightMech.GetSkills()`.
fn skill_modifiers(skill: &Overlay, slot: usize, modifiers: &mut Vec<Modifier>) -> Result<()> {
    let q32 = |value: i128, units_per_one: i128| {
        i64::try_from(value * ONE / units_per_one)
            .map_err(|_| Error::new("a skill value is outside the signed range"))
    };
    refuse_unrecorded_skill_fields(skill)?;
    {
        let slot =
            Some(u16::try_from(slot).map_err(|_| Error::new("a skill slot is outside u16"))?);
        for (index, field) in DAMAGE_RATES {
            if let Some(rate) = skill.aggregate(index) {
                push_rate(
                    modifiers,
                    ModifierChannel::SkillFloatRate,
                    slot,
                    field,
                    rate,
                )?;
            }
        }
        if let Some(range) = skill.aggregate(Index::AttackRange) {
            push(
                modifiers,
                ModifierChannel::SkillFloat,
                slot,
                "attack_range_value",
                ModifierPart::Value,
                q32(
                    range.value,
                    i128::from(crate::rules::SPACE_UNITS_PER_METER_SCALE),
                )?,
            );
            push_rate(
                modifiers,
                ModifierChannel::SkillFloatRate,
                slot,
                "attack_range_rate",
                range,
            )?;
        }
        if let Some(splash) = skill.aggregate(Index::SplashRange) {
            push(
                modifiers,
                ModifierChannel::SkillFloat,
                slot,
                "splash_range_value",
                ModifierPart::Value,
                q32(
                    splash.value,
                    i128::from(crate::rules::SPACE_UNITS_PER_METER_SCALE),
                )?,
            );
        }
        for (index, channel, field, scale) in SKILL_VALUES {
            if let Some(value) = skill.aggregate(index) {
                let metres = i128::from(crate::rules::SPACE_UNITS_PER_METER_SCALE);
                let value = match scale {
                    Recorded::Q32Metres => q32(value.value, metres)?,
                    Recorded::WholeMetres => i64::try_from(value.value / metres)
                        .map_err(|_| Error::new("a skill's search offset is outside i64"))?,
                    Recorded::Raw => i64::try_from(value.value).map_err(|_| {
                        Error::new(format!("a skill's {} is outside i64", index.name()))
                    })?,
                };
                push(modifiers, channel, slot, field, ModifierPart::Value, value);
            }
        }
        if let Some(interval) = skill.aggregate(Index::AttackInterval) {
            push(
                modifiers,
                ModifierChannel::SkillFloat,
                slot,
                "attack_interval_value",
                ModifierPart::Value,
                i64::try_from(interval.value)
                    .map_err(|_| Error::new("an interval value is outside i64"))?,
            );
            push_rate(
                modifiers,
                ModifierChannel::SkillFloatRate,
                slot,
                "attack_interval_rate",
                interval,
            )?;
        }
    }
    Ok(())
}

/// Which domains a skill attacks, `FightSkill.IsAirAttack` and
/// `IsGroundAttack`: each the row's flag with the skill's `AirAttackValue` or
/// `GroundAttackValue` added, attacked while the sum is above zero. Arclight
/// with Anti-Aircraft Ammunition holds 1 and attacks aircraft; a Fang with
/// Grenade Launcher holds -1 and does not.
pub(crate) fn switched_targets(row: AttackTargets, skill: &Overlay) -> AttackTargets {
    let on = |flag: bool, domain| {
        i128::from(flag)
            + skill
                .aggregate(Index::AttackValueFor(domain))
                .map_or(0, |aggregate| aggregate.value)
            > 0
    };
    AttackTargets {
        ground: on(row.ground, UnitDomain::Ground),
        air: on(row.air, UnitDomain::Air),
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
    /// Whether the buffs' overlay holds an entry of `source` for `index`.
    pub(crate) fn buff_writes(&self, source: &str, index: Index) -> bool {
        self.buff
            .corrections(index)
            .any(|entry| entry.source == source)
    }

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
    pub(crate) fn resolve(&self, index: Index, base: i64) -> Result<i64> {
        self.resolve_scaled(index, base, |value| value, 0, ONE)
    }

    /// A damage with its skill's kills in it: `DamageProperty.CalculateDamage`
    /// adds the skill's `DamageRateByKillCount` enhancement times its
    /// `killCount` to the enhancements it sums, and reads no impairment of it.
    /// The factor the rates meet in is then multiplied by the skill's
    /// `DamageReduceRateBase`: a Melting Point with Energy Diffraction deals
    /// 0.17 of its beam's ramp, `trunc(20 × 0.17)` = 3 at its third blow.
    /// `DamageCalculator.GetNormalDamage`, what a recording reads as the
    /// unit's damage, leaves that rate out (`normal`): the same Melting
    /// Point's reads 1, its ramp's first step whole. It leaves out the extra
    /// enhancement too: a Wasp with Ground Specialization reads 202.
    ///
    /// Which of the skill's two damage properties answers is `against`'s:
    /// `AirDamageProperty` hands `CalculateDamage` the skill's
    /// `DamageChangeRateAir` as its extra enhancement and
    /// `GroundDamageProperty` its `DamageChagneRateGround`.
    fn resolve_damage(
        &self,
        base: i64,
        kills: i64,
        normal: bool,
        against: UnitDomain,
    ) -> Result<i64> {
        let per_kill = self
            .skill
            .aggregate(Index::DamagePerKill)
            .map_or(0, |aggregate| aggregate.enhance);
        let extra_add = self
            .skill
            .aggregate(Index::DamageRateAgainst(against))
            .filter(|_| !normal)
            .map_or(0, |aggregate| aggregate.value);
        let reduce_base = self
            .skill
            .aggregate(Index::DamageReduceRateBase)
            .filter(|_| !normal)
            .map_or(ONE, |aggregate| aggregate.remaining);
        self.resolve_scaled(
            Index::AttackDamage,
            base,
            |value| value,
            per_kill * i128::from(kills) + extra_add,
            reduce_base,
        )
    }

    /// [`Overlays::resolve`] with the enhancements alone: what a number
    /// comes to before anything reduces it.
    pub(crate) fn resolve_raised(&self, index: Index, base: i64) -> Result<i64> {
        let enhance = [&self.unit, &self.skill, &self.buff]
            .into_iter()
            .filter_map(|overlay| overlay.aggregate(index))
            .map(|aggregate| aggregate.enhance)
            .sum::<i128>();
        let scaled = i128::from(base) * (ONE + enhance) / ONE;
        i64::try_from(scaled).map_err(|_| {
            Error::new(format!(
                "{} raised is outside the signed range",
                index.name()
            ))
        })
    }

    /// [`Overlays::resolve`] for a base held in other units than its values:
    /// `value_scale` carries a summed value into the base's units.
    fn resolve_scaled(
        &self,
        index: Index,
        base: i64,
        value_scale: impl Fn(i128) -> i128,
        extra_enhance: i128,
        reduce_base: i128,
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
        if corrected.is_none() && extra_enhance == 0 && reduce_base == ONE {
            return Ok(base);
        }
        total.enhance += extra_enhance;
        // The rates meet first, as one FPoint factor, and the number is
        // multiplied by it once: `CalculateDamage` multiplies its summed
        // enhancement by the reduce rates, and that by the reduce rate base,
        // before it reaches the damage.
        let factor = (ONE + total.enhance) * total.remaining / ONE * reduce_base / ONE;
        let scaled = (i128::from(base) + value_scale(total.value)) * factor / ONE;
        i64::try_from(scaled).map_err(|_| {
            Error::new(format!(
                "{} resolved outside the range a number can hold",
                index.name()
            ))
        })
    }
}

/// The rate on what a formation gains, `MechTeam.expAddRate` and
/// `expReduceRate`, Q32.32.
///
/// It is not a unit's number: an officer's `expChangeRate` lands on the
/// unit's card, in its `DataSet`'s `UnitDataChangeFloatRate.ExpChangeRate`,
/// and `CardElement.RefreshExpRate` hands the card's two getters to its
/// formation through `MechTeam.ChangeExpRate`. The card's `DataSet` is a
/// `MultiplicativeDataFloat` like the unit's, so enhancements sum and
/// impairments compound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExperienceRate {
    /// Σ enhance.
    pub(crate) add: i64,
    /// Π (1 − impair): the remaining multiplier, which is what the reduce
    /// getter answers and `MechTeam` keeps.
    pub(crate) remaining: i64,
}

impl Default for ExperienceRate {
    /// `MechTeam`'s constructor: no enhancement, and `FPoint.One` remaining.
    fn default() -> Self {
        Self {
            add: 0,
            remaining: 1 << 32,
        }
    }
}

impl ExperienceRate {
    /// The rate with one more card entry routed into it by its sign.
    #[must_use]
    pub(crate) fn with(self, rate: i64) -> Self {
        match rate {
            0 => self,
            add if add > 0 => Self {
                add: self.add.saturating_add(add),
                ..self
            },
            reduce => Self {
                remaining: i64::try_from(
                    i128::from(self.remaining) * (ONE + i128::from(reduce)) / ONE,
                )
                .unwrap_or(0),
                ..self
            },
        }
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
    /// Its skills' `DamageCalculator.killCount`: every target it hit that
    /// died while it lived.
    kills: i64,
    /// Q32.32 metres a second: `MoveSpeedProperty` keeps the speed as an
    /// `FPoint`, and a rate on it lands between two millimetres.
    move_speed_q32: i64,
    max_life: i64,
    /// What the main skill deals a ground unit and an aerial one, by its
    /// `GroundDamageProperty` and `AirDamageProperty`.
    attack_damage: i64,
    attack_damage_air: i64,
    /// Q32.32 seconds: `AttackIntervalProperty` keeps the interval as an
    /// `FPoint`, and a rate on it lands between two time units.
    attack_interval_q32: i64,
    attack_range: i64,
    /// Q32.32 metres: `AttackRangeProperty` keeps the range as an `FPoint`,
    /// and a rate on it lands between two millimetres.
    attack_range_q32: i64,
    splash_radius: i64,
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
            kills: 0,
            move_speed_q32: 0,
            max_life: 0,
            attack_damage: 0,
            attack_damage_air: 0,
            attack_interval_q32: 0,
            attack_range: 0,
            attack_range_q32: 0,
            splash_radius: 0,
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
    /// apply the `DataSet`s. The build's nine `attributeUpgradeDatas` rows
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
            0,
            ONE,
        )?;
        self.max_life = resolve(Index::MaxLife, self.base(rules.max_life)?)?;
        let base_damage = self.base(rules.attack.base_damage)?;
        self.attack_damage =
            self.overlays
                .resolve_damage(base_damage, self.kills, false, UnitDomain::Ground)?;
        self.attack_damage_air =
            self.overlays
                .resolve_damage(base_damage, self.kills, false, UnitDomain::Air)?;
        // A value is Q32.32 seconds already, and so is the interval.
        self.attack_interval_q32 = self.overlays.resolve(
            Index::AttackInterval,
            time_to_q32(
                i64::try_from(rules.attack.interval_time_units())
                    .map_err(|_| Error::new("attack interval is outside the signed range"))?,
            ),
        )?;
        if self.attack_interval_q32 < 0 {
            return Err(Error::new("attack interval resolved below zero"));
        }
        // `AttackRangeProperty` reads no correction of a melee skill's range,
        // neither its `DataSet`'s values and rates nor a buff's: a Sandworm
        // with Anti-Aerial records the technology's 20 metres and reaches 60.
        if rules.attack.melee {
            self.attack_range = rules.attack.range();
            self.attack_range_q32 = space_to_q32(rules.attack.range());
        } else {
            self.attack_range = resolve(Index::AttackRange, rules.attack.range())?;
            self.attack_range_q32 = self.overlays.resolve_scaled(
                Index::AttackRange,
                space_to_q32(rules.attack.range()),
                |value| value * ONE / metres,
                0,
                ONE,
            )?;
        }
        self.splash_radius = resolve(Index::SplashRange, rules.attack.splash_radius())?;
        Ok(())
    }

    /// `DamageCalculator.AddKillCount` on each of its skills: one more kill,
    /// and the damage again.
    ///
    /// # Errors
    ///
    /// See [`Stats::refresh`].
    pub(crate) fn add_kill(&mut self, rules: &UnitConfig) -> Result<()> {
        self.kills = self.kills.saturating_add(1);
        self.refresh(rules)
    }

    /// `DamageCalculator.Clear`, as the fight ends: no kills, and the damage
    /// again.
    ///
    /// # Errors
    ///
    /// See [`Stats::refresh`].
    pub(crate) fn clear_kills(&mut self, rules: &UnitConfig) -> Result<()> {
        if self.kills == 0 {
            return Ok(());
        }
        self.kills = 0;
        self.refresh(rules)
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

    /// `DamageCalculator.GetAttackDamage`: the main skill's damage on a
    /// unit of `against`'s domain.
    pub(crate) const fn attack_damage_against(&self, against: UnitDomain) -> i64 {
        match against {
            UnitDomain::Ground => self.attack_damage,
            UnitDomain::Air => self.attack_damage_air,
        }
    }

    /// `DamageCalculator.GetNormalDamage`: the damage without the skill's
    /// `DamageReduceRateBase` and either property's extra enhancement, which
    /// a recording reads.
    pub(crate) fn normal_damage(&self, rules: &UnitConfig) -> i64 {
        self.base(rules.attack.base_damage)
            .and_then(|base| {
                self.overlays
                    .resolve_damage(base, self.kills, true, UnitDomain::Ground)
            })
            .expect("the layout verified the damage corrections")
    }

    /// Another skill's damage from its own base, as this unit's corrections
    /// leave it: `DamageProperty.CalculateDamage` over a skill whose `DataSet`
    /// holds the same corrections as the main skill's, and the buffs', with
    /// none of the main skill's kills.
    ///
    /// # Errors
    ///
    /// Returns an error when the damage leaves the signed range.
    pub(crate) fn damage_from(&self, base: i64, against: UnitDomain) -> Result<i64> {
        self.overlays.resolve_damage(base, 0, false, against)
    }

    /// Another skill's damage from its own base, with its own skill
    /// corrections in place of the main skill's, and the unit's and the
    /// buffs': `DamageProperty.CalculateDamage` over an extra skill whose
    /// `DataSet` holds what reaches it alone.
    ///
    /// # Errors
    ///
    /// Returns an error when the damage leaves the signed range.
    pub(crate) fn damage_with(
        &self,
        base: i64,
        skill: &[Entry],
        against: UnitDomain,
    ) -> Result<i64> {
        let overlays = Overlays {
            skill: Overlay::of(skill),
            ..self.overlays.clone()
        };
        overlays.resolve_damage(base, 0, false, against)
    }

    /// The laser's base damage is truncated after its ramp multiplier, before
    /// the dynamic damage rates. `DamageProperty.CalculateBaseDamage` and
    /// `CalculateDamage` are separate stages in the build.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "preserve the existing native laser ramp's float truncation"
    )]
    pub(crate) fn laser_damage(
        &self,
        rules: &UnitConfig,
        attack_count: usize,
        against: UnitDomain,
    ) -> i64 {
        let AttackPath::Laser { damage_multipliers } = &rules.attack.path else {
            unreachable!("laser damage requires the laser attack path")
        };
        self.ramped_laser_damage(
            rules.attack.base_damage,
            damage_multipliers,
            (1.0, attack_count),
            (self.kills, against),
            false,
        )
    }

    /// [`Self::laser_damage`] as `DamageCalculator.GetNormalDamage` answers
    /// it, without the skill's `DamageReduceRateBase`.
    pub(crate) fn laser_normal_damage(&self, rules: &UnitConfig, attack_count: usize) -> i64 {
        let AttackPath::Laser { damage_multipliers } = &rules.attack.path else {
            unreachable!("laser damage requires the laser attack path")
        };
        self.ramped_laser_damage(
            rules.attack.base_damage,
            damage_multipliers,
            (1.0, attack_count),
            (self.kills, UnitDomain::Ground),
            true,
        )
    }

    /// A beam's blow from the unit's base damage at its level, times the
    /// skill's damage rate and its ramp's multiplier for the blow
    /// (`FightLaserSkill.CalculateDamageRate`), truncated, then corrected by
    /// the rates that reach the skill, its damage property's the one for what
    /// it attacks. A skill other than the main one counts its own kills, of
    /// which no correction here reads any.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "preserve the existing native laser ramp's float truncation"
    )]
    pub(crate) fn ramped_laser_damage(
        &self,
        base_damage: i64,
        damage_multipliers: &[f64],
        (damage_rate, attack_count): (f64, usize),
        (kills, against): (i64, UnitDomain),
        normal: bool,
    ) -> i64 {
        let base = self
            .base(base_damage)
            .expect("the layout verified the level-scaled base");
        let multiplier = damage_multipliers[attack_count.min(damage_multipliers.len() - 1)];
        let ramped = (base as f64 * damage_rate * multiplier).trunc() as i64;
        self.overlays
            .resolve_damage(ramped, kills, normal, against)
            .expect("the layout verified the damage corrections")
    }

    /// Every correction on the unit, its skills and its buffs, as a recording
    /// stores it: the sparse list of native fields, the skill's repeated for
    /// each of `slots` skill slots.
    ///
    /// Each number a channel corrects lands in the field the build keeps its
    /// aggregate in. A value is the number's own units here and, in the
    /// recording, Q32.32 metres or seconds for a skill float and whole metres
    /// for the unit's move-speed int. Level ratings are in the base channel
    /// and never appear here.
    ///
    /// # Errors
    ///
    /// Returns an error for a correction the build has no field for, rather
    /// than dropping it from what the recording is compared with.
    pub(crate) fn modifiers(
        &self,
        slots: &[usize],
        own: &[(usize, &[Entry])],
    ) -> Result<Vec<Modifier>> {
        let mut modifiers = Vec::new();
        self.unit_modifiers(&mut modifiers)?;
        for &slot in slots {
            skill_modifiers(&self.overlays.skill, slot, &mut modifiers)?;
        }
        for &(slot, entries) in own {
            skill_modifiers(&Overlay::of(entries), slot, &mut modifiers)?;
        }
        self.buff_modifiers(&mut modifiers)?;
        mechcore_mcfr::sort_modifiers(&mut modifiers);
        Ok(modifiers)
    }

    fn unit_modifiers(&self, modifiers: &mut Vec<Modifier>) -> Result<()> {
        let unit = &self.overlays.unit;
        if let Some(life) = unit.aggregate(Index::MaxLife) {
            if life.value != 0 {
                return Err(Error::new("the unit DataSet has no field for a life value"));
            }
            push_rate(
                modifiers,
                ModifierChannel::MechFloatRate,
                None,
                "life_rate",
                life,
            )?;
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
            push(
                modifiers,
                ModifierChannel::MechInt,
                None,
                "move_speed_value",
                ModifierPart::Value,
                i64::try_from(speed.value / metres)
                    .map_err(|_| Error::new("a unit's move-speed value is outside i64"))?,
            );
            push_rate(
                modifiers,
                ModifierChannel::MechFloatRate,
                None,
                "move_speed_change_rate",
                speed,
            )?;
        }
        // `DataSet.floatDatas`, Q32.32 as the fire reads them, and
        // `DataSet.intDatas`' damage reduction, whole damage.
        for (index, channel, field) in [
            (
                Index::GroundFireRange,
                ModifierChannel::MechFloat,
                "gf_range_value",
            ),
            (
                Index::GroundFireLifeTime,
                ModifierChannel::MechFloat,
                "gf_life_time_value",
            ),
            (
                Index::ReduceDamage,
                ModifierChannel::MechInt,
                "reduce_damage_value",
            ),
        ] {
            if let Some(aggregate) = unit.aggregate(index) {
                if aggregate.rate()? != (0, 0) {
                    return Err(Error::new(format!(
                        "the unit DataSet has no field for a rate of {}",
                        index.name()
                    )));
                }
                push(
                    modifiers,
                    channel,
                    None,
                    field,
                    ModifierPart::Value,
                    i64::try_from(aggregate.value).map_err(|_| {
                        Error::new(format!("a unit's {} is outside i64", index.name()))
                    })?,
                );
            }
        }
        for index in [
            Index::AttackDamage,
            Index::AttackInterval,
            Index::AttackRange,
            Index::SplashRange,
            Index::AmplifyDamage,
            Index::DamagePerKill,
        ]
        .into_iter()
        .chain(SKILL_VALUES.map(|(index, ..)| index))
        {
            if unit.aggregate(index).is_some() {
                return Err(Error::new(format!(
                    "the unit DataSet has no field for {}",
                    index.name()
                )));
            }
        }
        Ok(())
    }

    /// The buffs' aggregate: move speed's value and rate, damage's rate and
    /// the rate on damage taken.
    fn buff_modifiers(&self, modifiers: &mut Vec<Modifier>) -> Result<()> {
        let buff = &self.overlays.buff;
        // `BuffManager.GetMoveSpeedChangeValue`: whole metres, as the unit's
        // own `DataSet` keeps them.
        if let Some(speed) = buff.aggregate(Index::MoveSpeed)
            && speed.value != 0
        {
            let metres = i128::from(crate::rules::SPACE_UNITS_PER_METER_SCALE);
            if speed.value % metres != 0 {
                return Err(Error::new(
                    "a buff's move-speed value is a whole number of metres",
                ));
            }
            push(
                modifiers,
                ModifierChannel::Buff,
                None,
                "move_speed_value",
                ModifierPart::Value,
                i64::try_from(speed.value / metres)
                    .map_err(|_| Error::new("a buff's move-speed value is outside i64"))?,
            );
        }
        for (index, field) in [
            (Index::MoveSpeed, "move_speed_rate"),
            (Index::AttackDamage, "damage_rate"),
            (Index::AmplifyDamage, "amplify_damage_rate"),
        ] {
            if let Some(aggregate) = buff.aggregate(index) {
                if aggregate.value != 0 && index != Index::MoveSpeed {
                    return Err(Error::new(format!(
                        "a buff's {} value is not a field this build records",
                        index.name()
                    )));
                }
                push_rate(modifiers, ModifierChannel::Buff, None, field, aggregate)?;
            }
        }
        // `BuffManager.GetAttackRangeAddValue` and `GetAttackRangeReduceValue`:
        // the buffs' whole metres on the main skill's range, the ones that
        // add and the ones that take away kept apart, a reduction signed.
        let metres = crate::rules::SPACE_UNITS_PER_METER_SCALE;
        let range_values = buff
            .corrections(Index::AttackRange)
            .map(|entry| match entry.correction {
                Correction::Value(value) if value % metres == 0 => Ok(value / metres),
                Correction::Value(_) => Err(Error::new(
                    "a buff's attack-range value is a whole number of metres",
                )),
                Correction::Rate { .. } => {
                    Err(Error::new("no buff here corrects attack range by a rate"))
                }
            })
            .collect::<Result<Vec<_>>>()?;
        for (field, value) in [
            (
                "attack_range_add_value",
                range_values.iter().filter(|value| **value > 0).sum::<i64>(),
            ),
            (
                "attack_range_reduce_value",
                range_values.iter().filter(|value| **value < 0).sum::<i64>(),
            ),
        ] {
            push(
                modifiers,
                ModifierChannel::Buff,
                None,
                field,
                ModifierPart::Value,
                value,
            );
        }
        for index in [
            Index::MaxLife,
            Index::AttackInterval,
            Index::SplashRange,
            Index::DamagePerKill,
            Index::GroundFireRange,
            Index::GroundFireLifeTime,
            Index::ReduceDamage,
        ]
        .into_iter()
        .chain(SKILL_VALUES.map(|(index, ..)| index))
        {
            if buff.aggregate(index).is_some() {
                return Err(Error::new(format!(
                    "no buff here corrects {}",
                    index.name()
                )));
            }
        }
        Ok(())
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

    /// A hit raised by what increases this unit's damage taken, before
    /// anything reduces it: `PerformHitTargetEffect`'s `damageTaken`.
    ///
    /// # Errors
    ///
    /// Returns an error when the raised hit leaves the signed range.
    pub(crate) fn damage_taken_raised(&self, amount: i64) -> Result<i64> {
        self.overlays.resolve_raised(Index::AmplifyDamage, amount)
    }

    /// What each hit on this unit loses, `FightMech.GetDataInt` of
    /// `ReduceDamageValue`: the values its `DataSet` holds, summed.
    pub(crate) fn reduce_damage(&self) -> i64 {
        self.overlays
            .unit
            .aggregate(Index::ReduceDamage)
            .map_or(0, |aggregate| {
                i64::try_from(aggregate.value).unwrap_or(i64::MAX)
            })
    }

    pub(crate) const fn attack_interval_q32(&self) -> i64 {
        self.attack_interval_q32
    }

    /// `FightSkill.GetAttackRange` of the main skill while it locks a unit
    /// of `against`'s domain, or nothing for a ground one: its
    /// `AttackRangeAirProperty` or `AttackRangeGroundProperty`, each
    /// `AttackRangeProperty`'s range with the skill's add for that domain.
    pub(crate) fn attack_range_against(&self, against: UnitDomain) -> i64 {
        self.attack_range.saturating_add(self.range_add(against))
    }

    /// [`Self::attack_range_against`] in Q32.32 metres.
    pub(crate) fn attack_range_q32_against(&self, against: UnitDomain) -> i64 {
        self.attack_range_q32
            .saturating_add(space_to_q32(self.range_add(against)))
    }

    /// The skill's `AttackAirRangeAddValue` or `AttackGroundRangeAddValue`,
    /// millimetres.
    fn range_add(&self, against: UnitDomain) -> i64 {
        self.skill_value(Index::RangeAgainst(against))
    }

    /// What a search by `DistanceIntensify` counts off a candidate of
    /// `domain`'s distance, millimetres: the skill's `AttackRangeValueAir` or
    /// `AttackRangeValueGround`, which `SetTargetSelector` hands the selector
    /// as it makes it.
    pub(crate) fn score_offset_for(&self, domain: UnitDomain) -> i64 {
        self.skill_value(Index::ScoreOffsetFor(domain))
    }

    /// Which domains the main skill attacks: its row's, switched by what its
    /// `DataSet` holds ([`switched_targets`]).
    pub(crate) fn targets(&self, row: AttackTargets) -> AttackTargets {
        switched_targets(row, &self.overlays.skill)
    }

    /// What the main skill's projectiles' speed gains, millimetres a second.
    pub(crate) fn projectile_speed_add(&self) -> i64 {
        self.skill_value(Index::ProjectileSpeed)
    }

    /// The values the skill's `DataSet` sums for one number.
    fn skill_value(&self, index: Index) -> i64 {
        self.overlays.skill.value(index)
    }

    /// The splash radius, `FightSkill.GetSplashRange`: the description's with
    /// the skill's value added. `DamagePerformer.Perform` splashes only when
    /// it is above zero, so one corrected to zero or below is none.
    pub(crate) fn splash_radius(&self) -> i64 {
        self.splash_radius.max(0)
    }
}

/// Adds one non-zero modifier.
fn push(
    modifiers: &mut Vec<Modifier>,
    channel: ModifierChannel,
    skill_slot: Option<u16>,
    field: &str,
    part: ModifierPart,
    value: i64,
) {
    if value != 0 {
        modifiers.push(Modifier {
            channel,
            skill_slot,
            field: field.to_owned(),
            part,
            value,
        });
    }
}

/// Adds a rate's enhancement and impairment, each when non-zero.
fn push_rate(
    modifiers: &mut Vec<Modifier>,
    channel: ModifierChannel,
    skill_slot: Option<u16>,
    field: &str,
    aggregate: Aggregate,
) -> Result<()> {
    let (add, reduce) = aggregate.rate()?;
    push(
        modifiers,
        channel,
        skill_slot,
        field,
        ModifierPart::Add,
        add,
    );
    push(
        modifiers,
        channel,
        skill_slot,
        field,
        ModifierPart::Reduce,
        reduce,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Channel, Correction, Entry, Index, Stats};
    use crate::rules::{SimulationConfig, UnitDomain};

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
        assert_eq!(
            stats.attack_damage_against(UnitDomain::Ground),
            rules.attack.base_damage
        );
        assert_eq!(
            stats.attack_interval_q32(),
            super::time_to_q32(i64::try_from(rules.attack.interval_time_units()).unwrap())
        );
        assert_eq!(
            stats.attack_range_against(UnitDomain::Ground),
            rules.attack.range()
        );
    }

    /// A level multiplies the description's life and damage, and nothing
    /// else, before any overlay: `tests/level/`'s recordings read 3244 and
    /// 4658 at level 2, 4866 and 6987 at level 3, and move speed, interval and
    /// range unchanged.
    /// A kill-count rate adds its enhancement once per kill, and the fight's
    /// end takes the kills away: `tests/modifier/fights/officer-kills-crawlers.yaml`
    /// reads 4983 after four kills and 3560 again on its last tick.
    #[test]
    fn each_kill_adds_the_kill_count_rate_to_the_damage() {
        let rhino = crate::rules::UnitConfigs::load()
            .unwrap()
            .get("rhino")
            .unwrap()
            .clone();
        let per_kill = (
            Channel::Skill,
            Entry {
                index: Index::DamagePerKill,
                source: "test",
                correction: Correction::Rate {
                    add: 429_496_729,
                    reduce: 0,
                },
            },
        );
        let mut stats = Stats::corrected(&rhino, 1, &[per_kill]).unwrap();
        assert_eq!(
            stats.attack_damage_against(UnitDomain::Ground),
            3560,
            "no kill, no change"
        );
        for _ in 0..4 {
            stats.add_kill(&rhino).unwrap();
        }
        assert_eq!(stats.attack_damage_against(UnitDomain::Ground), 4983);
        stats.clear_kills(&rhino).unwrap();
        assert_eq!(stats.attack_damage_against(UnitDomain::Ground), 3560);
    }

    #[test]
    fn a_level_multiplies_base_life_and_damage() {
        let rules = marksman();
        for (level, life, damage) in [(2, 3244, 4658), (3, 4866, 6987)] {
            let stats = Stats::at_level(&rules, level).unwrap();
            assert_eq!(
                (
                    stats.max_life(),
                    stats.attack_damage_against(UnitDomain::Ground)
                ),
                (life, damage)
            );
            assert_eq!(
                stats.move_speed_q32(),
                super::space_to_q32(rules.move_speed())
            );
            assert_eq!(
                stats.attack_interval_q32(),
                super::time_to_q32(i64::try_from(rules.attack.interval_time_units()).unwrap())
            );
            assert_eq!(
                stats.attack_range_against(UnitDomain::Ground),
                rules.attack.range()
            );
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
        assert_eq!(stats.attack_damage_against(UnitDomain::Ground), 6055);
    }

    /// Advanced Offensive Tactics' `+0.3`, in the Q32.32 raw the build stores
    /// and `config/officer_effects.yaml` carries.
    const THIRTY_PERCENT: i64 = 1_288_490_188;

    /// The capture, replayed against this layer.
    ///
    /// `tests/modifier/fights/officer-composition-*.yaml` hold one Marksman shooting
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
        assert_eq!(stats.attack_damage_against(UnitDomain::Ground), 3027);

        stats.overlays.channel(Channel::Skill).write(officer);
        stats.refresh(&rules).unwrap();
        assert_eq!(stats.attack_damage_against(UnitDomain::Ground), 3726);
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
        assert_eq!(
            stats.attack_range_against(UnitDomain::Ground),
            base + 20_000,
            "ten metres, twice"
        );

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
        assert_eq!(
            stats.attack_range_against(UnitDomain::Ground),
            expected,
            "the rate takes the sum"
        );
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
        assert_eq!(
            once.attack_damage_against(UnitDomain::Ground),
            2072,
            "2329 x 0.89"
        );

        let mut twice = Stats::of(&rules).unwrap();
        for _ in 0..2 {
            twice.overlays.channel(Channel::Skill).write(impair.clone());
        }
        twice.refresh(&rules).unwrap();
        assert_eq!(
            twice.attack_damage_against(UnitDomain::Ground),
            1844,
            "2329 x 0.89 x 0.89"
        );
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
        assert_eq!(stats.attack_damage_against(UnitDomain::Ground), 302);
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
