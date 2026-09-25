//! How much experience fills a formation's level.
//!
//! `config/unit_experience.yaml` carries the build's table verbatim, and
//! `docs/rules/unit_experience.md` states the formula it follows and how the
//! game reads it. A level's bar is full at the experience the game asks of the
//! next level, which is the `maximum` in a formation's `exp: current/maximum`.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const UNIT_EXPERIENCE: &str = include_str!("../../../config/unit_experience.yaml");

#[derive(Deserialize)]
struct File {
    units: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    #[serde(rename = "type")]
    type_name: String,
    upgrade_exp: Vec<i32>,
}

fn table() -> &'static BTreeMap<String, Vec<i32>> {
    static TABLE: OnceLock<BTreeMap<String, Vec<i32>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let file: File = serde_yaml::from_str(UNIT_EXPERIENCE)
            .expect("config/unit_experience.yaml is the table this crate ships");
        file.units
            .into_iter()
            .map(|row| (row.type_name, row.upgrade_exp))
            .collect()
    })
}

/// The highest level a formation reaches.
pub const MAX_LEVEL: i32 = 9;

/// The experience that fills a formation's bar at `level`.
///
/// The game asks for the next level's experience, and reading past the last
/// level stays on the last one, so level 9 fills at level 8's amount. Absent
/// for a unit the table does not name and for a level outside 1 through 9.
#[must_use]
pub fn full(type_name: &str, level: i32) -> Option<i32> {
    if !(1..=MAX_LEVEL).contains(&level) {
        return None;
    }
    let row = table().get(type_name)?;
    let at = usize::try_from(level - 1)
        .ok()?
        .min(row.len().checked_sub(1)?);
    row.get(at).copied()
}

#[cfg(test)]
mod tests {
    use super::{full, table};

    /// Every unit a standard match sells has a row, and every row holds the
    /// eight levels that have a next one.
    #[test]
    fn every_sold_unit_has_eight_levels() {
        assert_eq!(table().len(), 33);
        assert!(table().values().all(|row| row.len() == 8));
        assert_eq!(full("marksman", 1), Some(650));
        assert_eq!(full("marksman", 4), Some(2919));
        assert_eq!(full("marksman", 8), Some(4373));
        assert_eq!(full("marksman", 9), Some(4373));
        assert_eq!(full("marksman", 0), None);
        assert_eq!(full("marksman", 10), None);
        assert_eq!(full("interceptor", 1), None);
    }

    /// The formula `docs/rules/unit_experience.md` states: a level needs the
    /// first level's experience times that level's factor, rounded half up.
    /// Vulcan is the one row it does not produce, and the rules document
    /// says how it differs.
    #[test]
    fn every_row_but_vulcans_follows_the_formula() {
        const FACTORS: [(i64, i64); 8] = [
            (1, 1),
            (225_321, 100_000),
            (35_618, 10_000),
            (449_028, 100_000),
            (521_046, 100_000),
            (579_888, 100_000),
            (629_639, 100_000),
            (672_735, 100_000),
        ];
        let predict = |base: i64| -> Vec<i32> {
            FACTORS
                .iter()
                .map(|(numerator, denominator)| {
                    i32::try_from((2 * base * numerator + denominator) / (2 * denominator)).unwrap()
                })
                .collect()
        };
        for (type_name, row) in table() {
            let base = i64::from(row[0]);
            if type_name == "vulcan" {
                assert_ne!(predict(base), *row);
                assert_eq!(predict(base * 11 / 10)[1..], row[1..]);
            } else {
                assert_eq!(predict(base), *row, "{type_name}");
            }
        }
    }
}
