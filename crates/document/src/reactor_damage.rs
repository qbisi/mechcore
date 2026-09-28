//! What a fight's survivors take off the other side's reactor core.
//!
//! `config/reactor_damage.yaml` carries the build's scores and rates
//! verbatim, and `docs/rules/reactor_damage.md` states the rule: each unit
//! alive when a fight ends scores its row's value at its level, cut by a rate
//! for each way it did not come from its own side's formations, and every side
//! that scores takes its score off every other side's core.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const REACTOR_DAMAGE: &str = include_str!("../../../config/reactor_damage.yaml");

/// `FPoint`'s one.
const ONE: i64 = 1 << 32;

#[derive(Deserialize)]
struct File {
    support_unit_score_rate: i64,
    rebirth_unit_score_rate: i64,
    team_changed_unit_score_rate: i64,
    units: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    unit_id: u32,
    score: Vec<i32>,
}

struct Table {
    support: i64,
    rebirth: i64,
    team_changed: i64,
    scores: BTreeMap<u32, Vec<i32>>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let file: File = serde_yaml::from_str(REACTOR_DAMAGE)
            .expect("config/reactor_damage.yaml is the table this crate ships");
        Table {
            support: file.support_unit_score_rate,
            rebirth: file.rebirth_unit_score_rate,
            team_changed: file.team_changed_unit_score_rate,
            scores: file
                .units
                .into_iter()
                .map(|row| (row.unit_id, row.score))
                .collect(),
        }
    })
}

/// One unit alive at a fight's end, as far as its score depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Survivor {
    /// The unit's type, the build's `mechID`.
    pub unit_type: u32,
    /// Its level, when it is known. A unit whose row scores every level alike
    /// needs none.
    pub level: Option<i32>,
    /// Whether it was not deployed from its side's own formations: summoned,
    /// produced or spawned, which is any `MechCreateType` but the default.
    pub support: bool,
    /// Whether it has been reborn in this fight.
    pub reborn: bool,
    /// Whether it fights for a side other than the one it started on.
    pub team_changed: bool,
}

/// `FightMech.GetScore`: the unit's row at its level, times the rate of each
/// way it did not come from its side's formations, floored.
///
/// # Errors
///
/// Returns why a score cannot be answered: the type has no row, or the row
/// scores levels differently and the unit's level is not known.
pub fn score(unit: Survivor) -> Result<i64, String> {
    let table = table();
    let row = table
        .scores
        .get(&unit.unit_type)
        .ok_or_else(|| format!("unit type {} has no score", unit.unit_type))?;
    let value = match unit.level {
        // `MechExpData.GetData` reads past the last level as the last.
        Some(level) => {
            let at = usize::try_from(level.max(1) - 1).unwrap_or(0);
            row[at.min(row.len() - 1)]
        }
        None if row.iter().all(|value| *value == row[0]) => row[0],
        None => {
            return Err(format!(
                "unit type {} scores by level, and the unit's level is not known",
                unit.unit_type
            ));
        }
    };
    let mut rate = ONE;
    if unit.support {
        rate = multiply(rate, table.support);
    }
    if unit.reborn {
        rate = multiply(rate, table.rebirth);
    }
    if unit.team_changed {
        rate = multiply(rate, table.team_changed);
    }
    Ok(multiply(i64::from(value) << 32, rate) >> 32)
}

/// `FPoint` multiplication.
fn multiply(left: i64, right: i64) -> i64 {
    i64::try_from((i128::from(left) * i128::from(right)) >> 32).unwrap_or(i64::MAX)
}

/// A side's score: the sum of its survivors' scores.
///
/// # Errors
///
/// Returns the first survivor whose score cannot be answered.
pub fn side_score(survivors: impl IntoIterator<Item = Survivor>) -> Result<i64, String> {
    survivors.into_iter().map(score).sum()
}

/// `TeamScoreCalculatorReduce.Perform` over two sides: each side that scores
/// takes its score off the other's core. Answers the damage blue's core and
/// red's core take, given blue's score and red's.
#[must_use]
pub const fn damage(blue: i64, red: i64) -> (i64, i64) {
    (
        if red > 0 { red } else { 0 },
        if blue > 0 { blue } else { 0 },
    )
}

#[cfg(test)]
mod tests {
    use super::{Survivor, damage, score, side_score, table};

    const FORTRESS: u32 = 1;
    const WASP: u32 = 6;
    const CRAWLER: u32 = 10;

    fn deployed(unit_type: u32) -> Survivor {
        Survivor {
            unit_type,
            level: Some(1),
            support: false,
            reborn: false,
            team_changed: false,
        }
    }

    /// Every rate this build ships is a quarter, which is what the cases
    /// below are worked in.
    #[test]
    fn every_rate_is_a_quarter() {
        let table = table();
        for rate in [table.support, table.rebirth, table.team_changed] {
            assert_eq!(rate, 1 << 30);
        }
    }

    #[test]
    fn a_deployed_unit_scores_its_row() {
        assert_eq!(score(deployed(FORTRESS)), Ok(350));
        assert_eq!(score(deployed(CRAWLER)), Ok(4));
    }

    /// A summoned Wasp scores a quarter of 17, floored.
    #[test]
    fn a_summoned_unit_scores_a_quarter_floored() {
        let summoned = Survivor {
            support: true,
            ..deployed(WASP)
        };
        assert_eq!(score(summoned), Ok(4));
        let crawler = Survivor {
            support: true,
            ..deployed(CRAWLER)
        };
        assert_eq!(score(crawler), Ok(1));
    }

    /// A controlled unit is cut once for changing sides, and again for each
    /// other way it did not come from its side's formations.
    #[test]
    fn a_controlled_unit_scores_a_quarter() {
        let controlled = Survivor {
            team_changed: true,
            ..deployed(FORTRESS)
        };
        assert_eq!(score(controlled), Ok(87));
        let both = Survivor {
            support: true,
            ..controlled
        };
        assert_eq!(score(both), Ok(21));
    }

    /// A unit that died and was brought back scores a quarter, and a summoned
    /// one that was reborn takes both cuts before the one floor.
    #[test]
    fn a_reborn_unit_scores_a_quarter() {
        let reborn = Survivor {
            reborn: true,
            ..deployed(FORTRESS)
        };
        assert_eq!(score(reborn), Ok(87));
        let summoned = Survivor {
            support: true,
            ..reborn
        };
        assert_eq!(score(summoned), Ok(21));
        // A quarter of a quarter of a Wasp's 17 is a floored 1, not 0 twice.
        let wasp = Survivor {
            support: true,
            reborn: true,
            ..deployed(WASP)
        };
        assert_eq!(score(wasp), Ok(1));
    }

    /// A row that scores every level alike needs no level; one past the last
    /// level reads the last.
    #[test]
    fn a_level_is_read_as_the_table_reads_it() {
        let unknown = Survivor {
            level: None,
            ..deployed(FORTRESS)
        };
        assert_eq!(score(unknown), Ok(350));
        let past = Survivor {
            level: Some(12),
            ..deployed(FORTRESS)
        };
        assert_eq!(score(past), Ok(350));
        assert!(score(deployed(999_999)).is_err());
    }

    #[test]
    fn one_side_with_survivors_damages_the_other() {
        let blue = side_score([deployed(FORTRESS), deployed(CRAWLER)]).unwrap();
        assert_eq!(blue, 354);
        assert_eq!(damage(blue, 0), (0, 354));
        assert_eq!(damage(0, 17), (17, 0));
    }

    /// A fight that runs out of time leaves both sides standing, and each
    /// takes the other's score.
    #[test]
    fn both_sides_with_survivors_damage_each_other() {
        assert_eq!(damage(354, 17), (17, 354));
    }

    #[test]
    fn neither_side_with_survivors_damages_nothing() {
        assert_eq!(side_score([]), Ok(0));
        assert_eq!(damage(0, 0), (0, 0));
    }
}
