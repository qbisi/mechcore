//! The fields `ICommonMechDataChangeDataSource` answers, and what each one is.
//!
//! `OfficerData`, `TechnologyData`, `EquipmentData` and `EnergyTowerSkillData`
//! all implement that interface, so one field means the same thing whichever
//! of them wrote it. This is where a field becomes a correction, and both
//! [`super::officers`] and [`super::technologies`] read their own table and
//! hand the numbers here.
//!
//! A field that corrects one of the numbers [`crate::data::Stats`] derives
//! becomes a correction in the channel MCFR records it in: a recording keeps
//! exactly one place for each field, so which channel a field lives in is the
//! recording's own shape rather than a choice made here.

use crate::data::{Channel, Correction, Index};

/// The build's quantum for a distance and for a time, which
/// `crates/simulation/src/rules.rs` quantizes a description with.
pub(crate) const METERS: i64 = 1_000;
pub(crate) const SECONDS: i64 = 2_000;
const FIXED_ONE: i128 = 1 << 32;

/// An `FPoint` in its own unit, in the quantized units the simulator holds.
///
/// Every value in either table is a whole number of metres or a tenth of a
/// second, so this is exact; it truncates toward zero for anything else, as
/// the build's own quantization does.
pub(crate) fn fixed_to(raw: i64, quantum: i64) -> i64 {
    let scaled = i128::from(raw) * i128::from(quantum) / FIXED_ONE;
    i64::try_from(scaled).unwrap_or(i64::MAX)
}

/// What one source writes onto a unit, as its own table read it.
///
/// Every field is optional because a row carries the ones it uses and leaves
/// the rest at zero, and both tables write zero and absent to mean the same
/// thing.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Fields {
    pub(crate) life_rate: Option<i64>,
    pub(crate) damage_rate: Option<i64>,
    pub(crate) attack_range_rate: Option<i64>,
    pub(crate) attack_interval_rate: Option<i64>,
    pub(crate) attack_range_value: Option<i64>,
    pub(crate) attack_interval_value: Option<i64>,
    pub(crate) speed_value: Option<i64>,
}

/// What the fields write, in the channels the recording keeps them in.
///
/// A rate is routed by its sign, as `MultiplicativeDataFloat.Refresh` routes
/// it: a positive rate enhances and a negative one impairs, and the two are
/// not each other's negation once there are two of them.
pub(crate) fn corrections(fields: Fields) -> Vec<(Channel, Index, Correction)> {
    let mut written = Vec::new();
    let mut rate = |value: Option<i64>, channel, index| match value.filter(|value| *value != 0) {
        None => {}
        Some(raw) if raw > 0 => {
            written.push((
                channel,
                index,
                Correction::Rate {
                    add: raw,
                    reduce: 0,
                },
            ));
        }
        Some(raw) => written.push((
            channel,
            index,
            Correction::Rate {
                add: 0,
                reduce: -raw,
            },
        )),
    };
    rate(fields.damage_rate, Channel::Skill, Index::AttackDamage);
    rate(
        fields.attack_interval_rate,
        Channel::Skill,
        Index::AttackInterval,
    );
    rate(fields.attack_range_rate, Channel::Skill, Index::AttackRange);
    rate(fields.life_rate, Channel::Unit, Index::MaxLife);

    // An FPoint value in metres or seconds reaches the simulator in the
    // number's own quantized units, which is what `Stats` resolves against.
    if let Some(raw) = fields.attack_range_value.filter(|raw| *raw != 0) {
        written.push((
            Channel::Skill,
            Index::AttackRange,
            Correction::Value(fixed_to(raw, METERS)),
        ));
    }
    if let Some(raw) = fields.attack_interval_value.filter(|raw| *raw != 0) {
        written.push((
            Channel::Skill,
            Index::AttackInterval,
            Correction::Value(fixed_to(raw, SECONDS)),
        ));
    }

    // A plain integer lands in `DataSet.intDatas`, whose entries `FightMech`
    // builds as `DataIntGroup(Int32.MinValue, Int32.MaxValue, 0)`: a sum whose
    // clamp is the whole range and therefore never binds.
    if let Some(raw) = fields.speed_value.filter(|raw| *raw != 0) {
        written.push((
            Channel::Unit,
            Index::MoveSpeed,
            Correction::Value(raw.saturating_mul(METERS)),
        ));
    }
    written
}

/// Why a field this build will not apply is refused.
pub(crate) const VALUE_ELSEWHERE: &str = "no number this simulator derives is the one it corrects";
pub(crate) const SPLASH: &str = "no number this simulator derives is a splash radius";
pub(crate) const KILLS: &str = "no mechanism here counts a unit's kills";
pub(crate) const PROJECTILE: &str = "no mechanism here reads a projectile's own numbers";
