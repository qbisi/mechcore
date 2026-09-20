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
//! which `scripts/officer-composition.mcscript` measured against the game and
//! `docs/rules/officer_effects.md` records. The decompilation index carries no
//! method bodies, so nothing here is read off the build; what the build stores
//! and what it then computed were captured together and agree. What that
//! capture did not reach — a value correction, and one number corrected in two
//! channels at once — is refused rather than extended to.

use crate::{Error, Result, rules::UnitConfig};

/// One, in the Q32.32 fixed point a rate is stored in.
const ONE: i128 = 1 << 32;

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
}

impl Index {
    const fn name(self) -> &'static str {
        match self {
            Self::MoveSpeed => "move speed",
            Self::MaxLife => "max life",
            Self::AttackDamage => "attack damage",
            Self::AttackInterval => "attack interval",
            Self::AttackRange => "attack range",
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
/// A rate's two halves are both non-negative and neutral at zero, as MCFR
/// stores them: the native reduce getter answers the remaining multiplier and
/// the recording keeps `1 − it`, so a 30% slow is `reduce` of 0.3.
#[allow(
    dead_code,
    reason = "a mechanism writes these; the neutral case and the refusal are \
              tested now"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Correction {
    /// Q32.32 raw, `{add, reduce}`.
    Rate { add: i64, reduce: i64 },
    /// Native units, `{add, reduce}`.
    Value { add: i64, reduce: i64 },
}

impl Correction {
    /// Whether this correction changes nothing, which is the only case the
    /// composition rule is not needed for.
    const fn neutral(self) -> bool {
        match self {
            Self::Rate { add, reduce } | Self::Value { add, reduce } => add == 0 && reduce == 0,
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

    /// The description's number, corrected by every overlay that touches it.
    ///
    /// A rate is summed within its channel and applied once:
    /// `base × (1 + Σ add − Σ reduce)`, truncated toward zero. The build sums
    /// them itself — two officers of one kind reach a recording as one entry
    /// holding their sum rather than two entries — so summing here mirrors
    /// what the build stores rather than inventing an order over entries.
    /// `docs/rules/officer_effects.md` records the capture that establishes
    /// it.
    ///
    /// # Errors
    ///
    /// Two questions that capture does not answer are refused rather than
    /// guessed: a value correction, which nothing has measured the composition
    /// of, and one number corrected in more than one channel at once, which
    /// nothing has measured the order of.
    fn resolve(&self, index: Index, base: i64) -> Result<i64> {
        let mut corrected: Option<(&'static str, i128)> = None;
        for (channel, overlay) in [
            ("unit", &self.unit),
            ("skill", &self.skill),
            ("buff", &self.buff),
        ] {
            let mut rate = 0_i128;
            for entry in overlay.corrections(index) {
                match entry.correction {
                    Correction::Rate { add, reduce } => {
                        rate += i128::from(add) - i128::from(reduce);
                    }
                    Correction::Value { .. } if !entry.correction.neutral() => {
                        return Err(Error::new(format!(
                            "{} carries a {channel} value correction from {}, and how a \
                             value composes is not measured: see the unresolved questions \
                             in docs/spec/simulation/architecture.md",
                            index.name(),
                            entry.source
                        )));
                    }
                    Correction::Value { .. } => {}
                }
            }
            if rate != 0 {
                if let Some((first, _)) = corrected {
                    return Err(Error::new(format!(
                        "{} is corrected in the {first} channel and the {channel} channel \
                         at once, and what order two channels apply in is not measured: \
                         see the unresolved questions in \
                         docs/spec/simulation/architecture.md",
                        index.name()
                    )));
                }
                corrected = Some((channel, rate));
            }
        }
        let Some((_, rate)) = corrected else {
            return Ok(base);
        };
        let scaled = i128::from(base) * (ONE + rate) / ONE;
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
    move_speed: i64,
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
    pub(crate) fn of(rules: &UnitConfig) -> Result<Self> {
        let mut stats = Self {
            overlays: Overlays::default(),
            move_speed: 0,
            max_life: 0,
            attack_damage: 0,
            attack_interval: 0,
            attack_range: 0,
        };
        stats.refresh(rules)?;
        Ok(stats)
    }

    /// Recomputes every derived number from the description and the overlays.
    ///
    /// # Errors
    ///
    /// Returns an error when an overlay carries a correction that is not
    /// neutral; see [`Overlays::resolve`].
    pub(crate) fn refresh(&mut self, rules: &UnitConfig) -> Result<()> {
        let resolve = |index, base| self.overlays.resolve(index, base);
        self.move_speed = resolve(Index::MoveSpeed, rules.move_speed())?;
        self.max_life = resolve(Index::MaxLife, rules.max_life)?;
        self.attack_damage = resolve(Index::AttackDamage, rules.attack.base_damage)?;
        self.attack_interval = u64::try_from(resolve(
            Index::AttackInterval,
            i64::try_from(rules.attack.interval_time_units())
                .map_err(|_| Error::new("attack interval is outside the signed range"))?,
        )?)
        .map_err(|_| Error::new("attack interval resolved below zero"))?;
        self.attack_range = resolve(Index::AttackRange, rules.attack.range())?;
        Ok(())
    }

    pub(crate) const fn move_speed(&self) -> i64 {
        self.move_speed
    }

    pub(crate) const fn max_life(&self) -> i64 {
        self.max_life
    }

    pub(crate) const fn attack_damage(&self) -> i64 {
        self.attack_damage
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
        assert_eq!(stats.move_speed(), rules.move_speed());
        assert_eq!(stats.max_life(), rules.max_life);
        assert_eq!(stats.attack_damage(), rules.attack.base_damage);
        assert_eq!(stats.attack_interval(), rules.attack.interval_time_units());
        assert_eq!(stats.attack_range(), rules.attack.range());
    }

    /// Advanced Offensive Tactics' `+0.3`, in the Q32.32 raw the build stores
    /// and `config/officer_effects.yaml` carries.
    const THIRTY_PERCENT: i64 = 1_288_490_188;

    /// The capture, replayed against this layer.
    ///
    /// `scripts/officer-composition.mcscript` recorded one Marksman shooting
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
            source: "Loadout",
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
            source: "Loadout",
            correction: Correction::Rate { add: 0, reduce: 0 },
        });
        stats.refresh(&rules).unwrap();
        assert_eq!(stats.move_speed(), rules.move_speed());
    }

    /// The capture measured a rate. A value is refused until one measures it.
    #[test]
    fn a_value_correction_is_refused_until_it_is_measured() {
        let rules = marksman();
        let mut stats = Stats::of(&rules).unwrap();
        stats.overlays.channel(Channel::Unit).write(Entry {
            index: Index::AttackRange,
            source: "Loadout",
            correction: Correction::Value { add: 40, reduce: 0 },
        });
        let refused = stats.refresh(&rules).unwrap_err().to_string();
        assert!(refused.contains("attack range"), "{refused}");
        assert!(refused.contains("Loadout"), "{refused}");
        assert!(refused.contains("unit value correction"), "{refused}");
    }

    /// The capture put both officers in one channel, so what two channels do
    /// to one number in what order is still nobody's measurement.
    #[test]
    fn two_channels_correcting_one_number_are_refused() {
        let rules = marksman();
        let mut stats = Stats::of(&rules).unwrap();
        for channel in [Channel::Skill, Channel::Buff] {
            stats.overlays.channel(channel).write(Entry {
                index: Index::AttackDamage,
                source: "Loadout",
                correction: Correction::Rate {
                    add: THIRTY_PERCENT,
                    reduce: 0,
                },
            });
        }
        let refused = stats.refresh(&rules).unwrap_err().to_string();
        assert!(refused.contains("attack damage"), "{refused}");
        assert!(
            refused.contains("skill channel and the buff channel"),
            "{refused}"
        );
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
                correction: Correction::Value { add: 40, reduce: 0 },
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
