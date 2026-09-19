//! How much of a battle's next position its decisions predict.
//!
//! `docs/spec/document/battle.md` defines the measure. Each transition of a
//! battle starts from a round's opening position and the decisions taken from
//! it, [`crate::transition::predict`] produces the position the next round
//! opens with, and every leaf field of the recorded next position is put in one
//! of four classes. The recorded position is only ever compared against: no
//! prediction reads it.
//!
//! A leaf is a scalar reached through mappings, a formation or a panel slot
//! field aligned by its `index`, or a whole list otherwise, so an ID set and an
//! inventory with repeats are one leaf each. A leaf only one side has is still
//! a leaf, so a formation missing from the prediction counts against it.

use crate::battle::{Action, SideState, Turn};
use crate::economy::Economy;
use crate::opening::Stated;
use crate::reinforcement::Verified;
use crate::transition::{Unsettled, before_opening};
use serde::Serialize;
use serde_yaml::Value;
use std::collections::BTreeMap;

/// What the fight decides, by field group. The fight runs between the
/// decisions and the next opening, and nothing in a battle document says what
/// it did, so these leaves are neither predicted nor compared. Standard 1v1
/// has no fight-phase income, which is why `supply` is not here.
pub const FIGHT: &[&str] = &[
    "reactor_core",
    "formations.exp",
    "contraptions",
    "terrains",
    "airdrop_shields",
];

/// Field groups whose opening rule [`crate::transition::open_round`] does not
/// hold yet. Their leaves count as unimplemented whatever the prediction says,
/// because a value no rule produced can only agree by accident. A group leaves
/// this list with the rule that predicts it.
pub const UNIMPLEMENTED: &[&str] = &["supply"];

/// The fields the reinforcement deal reads from the position it is dealt in.
/// A deal checked against the recorded position is a deal of the predicted
/// one only where these leaves agree on both sides.
const DEALT_FROM: &[&str] = &[
    "formations.index",
    "formations.type",
    "next_index.unit",
    "shop.unlocked_units",
    "techs.units",
    "techs.officers",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Equal,
    Unequal,
    Unimplemented,
    Fight,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub equal: usize,
    pub unequal: usize,
    pub unimplemented: usize,
    pub fight: usize,
}

impl Counts {
    fn add(&mut self, class: Class) {
        match class {
            Class::Equal => self.equal += 1,
            Class::Unequal => self.unequal += 1,
            Class::Unimplemented => self.unimplemented += 1,
            Class::Fight => self.fight += 1,
        }
    }

    fn merge(&mut self, other: Self) {
        self.equal += other.equal;
        self.unequal += other.unequal;
        self.unimplemented += other.unimplemented;
        self.fight += other.fight;
    }
}

/// One side's transition, or the match's own fields as `side: match`.
#[derive(Debug, Serialize)]
pub struct Transition {
    /// The round the decisions were taken in; the recorded position is the
    /// one `round + 1` opens with.
    pub round: i32,
    pub side: &'static str,
    #[serde(flatten)]
    pub counts: Counts,
    /// Why nothing of this transition was predicted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpredicted: Option<String>,
}

/// A leaf the prediction got wrong. An absent value is `null`.
///
/// A decision the position cannot take is reported once, at the path
/// `actions` with the reason as its prediction, and every leaf of that side's
/// transition outside the fight counts as unequal.
#[derive(Debug, Serialize)]
pub struct Difference {
    pub round: i32,
    pub side: &'static str,
    pub path: String,
    pub predicted: Option<String>,
    pub recorded: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Coverage {
    pub total: Counts,
    /// Leaves by field group: the path with its `[index]` parts removed.
    pub fields: BTreeMap<String, Counts>,
    pub transitions: Vec<Transition>,
    pub unequal: Vec<Difference>,
    /// The last round's decisions, when no position follows them. They have
    /// nothing to be compared against, and are not a transition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub untargeted_round: Option<i32>,
}

impl Coverage {
    /// Whether every leaf outside the fight was predicted and agrees.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.total.unequal == 0 && self.total.unimplemented == 0
    }

    fn record(&mut self, leaves: &[(String, Class)]) -> Counts {
        let mut counts = Counts::default();
        for (path, class) in leaves {
            counts.add(*class);
            self.fields.entry(group(path)).or_default().add(*class);
        }
        self.total.merge(counts);
        counts
    }
}

/// Measures every transition a battle states.
///
/// `deal` is the reinforcement check's result over the same battle. It is the
/// prediction of `reinforce_offers`, which is dealt from a stream the header
/// seeds rather than from the decisions.
#[must_use]
pub fn measure(economy: &Economy, stated: &Stated, deal: Result<&Verified, &str>) -> Coverage {
    let mut coverage = Coverage::default();
    if let Some(first) = stated.turns.first() {
        coverage.opening(economy, stated, first);
    }
    for pair in stated.turns.windows(2) {
        let [turn, next] = pair else { continue };
        let mut dealt_from = true;
        for (side, red) in [("blue", false), ("red", true)] {
            let (state, actions, recorded) = sides(turn, next, red);
            dealt_from &= coverage.side(economy, turn.round, state, actions, recorded, side);
        }
        coverage.deal(turn, next, deal.ok(), dealt_from);
    }
    if stated.ends_on_actions {
        coverage.untargeted_round = stated.turns.last().map(|turn| turn.round);
    }
    coverage
}

impl Coverage {
    /// Round zero's transition, from the position each side chooses its
    /// opening in onto the one round 1 opens with. The header deals that
    /// position, so it is built rather than read; nothing is fought in between.
    fn opening(&mut self, economy: &Economy, stated: &Stated, first: &Turn) {
        for (side, seat, header, actions, recorded) in [
            (
                "blue",
                0,
                &stated.sides.blue,
                &stated.opening.blue,
                &first.state.sides.blue,
            ),
            (
                "red",
                1,
                &stated.sides.red,
                &stated.opening.red,
                &first.state.sides.red,
            ),
        ] {
            match crate::opening::reactor_core(stated.map_id, seat) {
                Ok(core) => {
                    let before = before_opening(core, header.constructions.clone());
                    self.side(economy, 0, &before, actions, recorded, side);
                }
                Err(reason) => {
                    let leaves: Vec<_> = side_leaves(recorded)
                        .into_keys()
                        .map(|path| {
                            let class = fixed(&path, false).unwrap_or(Class::Unimplemented);
                            (path, class)
                        })
                        .collect();
                    let counts = self.record(&leaves);
                    self.transitions.push(Transition {
                        round: 0,
                        side,
                        counts,
                        unpredicted: Some(reason),
                    });
                }
            }
        }
    }

    /// One side's transition: `actions` taken from `state` in `round`, onto
    /// the recorded position the next round opens with. Answers whether every
    /// field the deal is dealt from came out equal.
    fn side(
        &mut self,
        economy: &Economy,
        round: i32,
        state: &SideState,
        actions: &[Action],
        recorded: &SideState,
        side: &'static str,
    ) -> bool {
        let red = side == "red";
        // Round zero ends in round 1's opening without a fight.
        let fought = round > 0;
        let predicted = crate::transition::predict(economy, round, state, actions, red);
        let (leaves, unpredicted) = match &predicted {
            Ok(predicted) => (compare(predicted, recorded, fought), None),
            Err(reason) => {
                // Tables without a row, or a board without a rule for where a
                // grant lands, are what the prediction lacks. A decision
                // naming what the position does not hold, or one the game
                // refuses, is the record contradicting the rules.
                let class = match reason {
                    Unsettled::Unpriced(_) | Unsettled::GrantedPosition => Class::Unimplemented,
                    Unsettled::Missing(_) | Unsettled::Refused(_) => {
                        self.unequal.push(Difference {
                            round,
                            side,
                            path: "actions".into(),
                            predicted: Some(format!("{reason:?}")),
                            recorded: None,
                        });
                        Class::Unequal
                    }
                };
                let leaves = side_leaves(recorded)
                    .into_iter()
                    .map(|(path, value)| {
                        let class = fixed(&path, fought).unwrap_or(class);
                        (path, class, None, Some(value))
                    })
                    .collect();
                (leaves, Some(format!("{reason:?}")))
            }
        };
        let dealt_from_agrees = leaves.iter().all(|(path, class, _, _)| {
            !DEALT_FROM.iter().any(|field| within(&group(path), field)) || *class == Class::Equal
        });
        let mut classes = Vec::with_capacity(leaves.len());
        for (path, class, predicted, recorded) in leaves {
            if class == Class::Unequal && unpredicted.is_none() {
                self.unequal.push(Difference {
                    round,
                    side,
                    path: path.clone(),
                    predicted,
                    recorded,
                });
            }
            classes.push((path, class));
        }
        let counts = self.record(&classes);
        self.transitions.push(Transition {
            round,
            side,
            counts,
            unpredicted,
        });
        dealt_from_agrees
    }

    /// The offers `next` is dealt. The deal is checked against the recorded
    /// position, so it is a deal of the predicted one only where `dealt_from`
    /// says every field it reads agreed.
    fn deal(&mut self, turn: &Turn, next: &Turn, deal: Option<&Verified>, dealt_from: bool) {
        let Some(recorded) = &next.state.reinforce_offers else {
            return;
        };
        let dealt = deal.and_then(|verified| {
            verified
                .rounds
                .iter()
                .find(|round| round.round == next.round)
        });
        let class = match dealt {
            Some(round) if round.offers == *recorded && dealt_from => Class::Equal,
            Some(round) if round.offers == *recorded => Class::Unimplemented,
            _ => Class::Unequal,
        };
        if class == Class::Unequal {
            self.unequal.push(Difference {
                round: turn.round,
                side: "match",
                path: "reinforce_offers".into(),
                predicted: dealt.map(|round| render(&to_value(&round.offers))),
                recorded: Some(render(&to_value(recorded))),
            });
        }
        let counts = self.record(&[("reinforce_offers".into(), class)]);
        self.transitions.push(Transition {
            round: turn.round,
            side: "match",
            counts,
            unpredicted: None,
        });
    }
}

fn sides<'a>(
    turn: &'a Turn,
    next: &'a Turn,
    red: bool,
) -> (&'a SideState, &'a [Action], &'a SideState) {
    if red {
        (
            &turn.state.sides.red,
            &turn.actions.red,
            &next.state.sides.red,
        )
    } else {
        (
            &turn.state.sides.blue,
            &turn.actions.blue,
            &next.state.sides.blue,
        )
    }
}

type Leaf = (String, Class, Option<String>, Option<String>);

/// Classifies every leaf either position has. `fought` says whether a fight
/// ran between the decisions and the recorded position.
fn compare(predicted: &SideState, recorded: &SideState, fought: bool) -> Vec<Leaf> {
    let mut predicted = side_leaves(predicted);
    let mut pairs: Vec<(String, Option<String>, Option<String>)> = side_leaves(recorded)
        .into_iter()
        .map(|(path, held)| {
            let made = predicted.remove(&path);
            (path, made, Some(held))
        })
        .collect();
    pairs.extend(
        predicted
            .into_iter()
            .map(|(path, made)| (path, Some(made), None)),
    );
    let mut leaves: Vec<Leaf> = pairs
        .into_iter()
        .map(|(path, made, held)| {
            let class = fixed(&path, fought).unwrap_or(if made == held {
                Class::Equal
            } else {
                Class::Unequal
            });
            (path, class, made, held)
        })
        .collect();
    leaves.sort_by(|left, right| left.0.cmp(&right.0));
    leaves
}

/// The class a leaf has whatever its value: the fight's, when one was
/// `fought`, or not yet predicted.
fn fixed(path: &str, fought: bool) -> Option<Class> {
    let group = group(path);
    if fought && FIGHT.iter().any(|field| within(&group, field)) {
        Some(Class::Fight)
    } else if UNIMPLEMENTED.iter().any(|field| within(&group, field)) {
        Some(Class::Unimplemented)
    } else {
        None
    }
}

/// Whether `group` is `field` or lies under it.
fn within(group: &str, field: &str) -> bool {
    group
        .strip_prefix(field)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// A leaf's path with its `[index]` parts removed.
fn group(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut inside = false;
    for character in path.chars() {
        match character {
            '[' => inside = true,
            ']' => inside = false,
            _ if !inside => out.push(character),
            _ => {}
        }
    }
    out
}

fn to_value<T: Serialize>(value: &T) -> Value {
    serde_yaml::to_value(value).unwrap_or(Value::Null)
}

/// Every leaf of one side's position, by path.
fn side_leaves(state: &SideState) -> BTreeMap<String, String> {
    let mut leaves = BTreeMap::new();
    if let Value::Mapping(fields) = to_value(state) {
        for (key, value) in &fields {
            walk(key.as_str().unwrap_or_default(), value, &mut leaves);
        }
    }
    leaves
}

fn walk(path: &str, value: &Value, leaves: &mut BTreeMap<String, String>) {
    match value {
        Value::Mapping(fields) => {
            for (key, field) in fields {
                walk(
                    &format!("{path}.{}", key.as_str().unwrap_or_default()),
                    field,
                    leaves,
                );
            }
        }
        Value::Sequence(items)
            if !items.is_empty() && items.iter().all(|item| index(item).is_some()) =>
        {
            for item in items {
                let at = index(item).unwrap_or_default();
                walk(&format!("{path}[{at}]"), item, leaves);
            }
        }
        other => {
            leaves.insert(path.to_owned(), render(other));
        }
    }
}

/// The `index` a listed object is aligned by.
fn index(item: &Value) -> Option<i64> {
    item.as_mapping()?.get("index")?.as_i64()
}

/// A value on one line, in the document's flow spelling.
fn render(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Sequence(items) => format!(
            "[{}]",
            items.iter().map(render).collect::<Vec<_>>().join(", ")
        ),
        Value::Mapping(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(key, field)| format!("{}: {}", render(key), render(field)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Tagged(tagged) => format!("{} {}", tagged.tag, render(&tagged.value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked() -> Vec<(String, Stated)> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir("../../tests/battle").expect("tracked battles") {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "yaml") {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            let stated = crate::opening::stated(&bytes).unwrap().unwrap();
            out.push((
                path.file_name().unwrap().to_string_lossy().into_owned(),
                stated,
            ));
        }
        out.sort_by(|left, right| left.0.cmp(&right.0));
        out
    }

    fn measured(economy: &Economy, stated: &Stated) -> Coverage {
        let opening = crate::opening::verify(economy, stated).unwrap();
        let deal = crate::reinforcement::verify(economy, stated, &opening).unwrap();
        measure(economy, stated, Ok(&deal))
    }

    /// The tracked corpus, by field group, as `[equal, unequal, unimplemented,
    /// fight]`. A change in any count is a change in what the transition
    /// predicts, and has to be made here to pass.
    #[test]
    fn tracked_battles_cover_what_the_table_says() {
        let economy = Economy::embedded().unwrap();
        let mut fields: BTreeMap<String, Counts> = BTreeMap::new();
        let mut unequal = Vec::new();
        let mut untargeted = 0;
        for (name, stated) in tracked() {
            let coverage = measured(&economy, &stated);
            for (group, counts) in coverage.fields {
                fields.entry(group).or_default().merge(counts);
            }
            unequal.extend(
                coverage
                    .unequal
                    .iter()
                    .map(|difference| format!("{name} {difference:?}")),
            );
            untargeted += usize::from(coverage.untargeted_round.is_some());
        }
        assert!(unequal.is_empty(), "{unequal:#?}");
        let expected: BTreeMap<String, Counts> = [
            ("airdrop_shields", [0, 0, 0, 9]),
            ("battle_skills.cooldown", [1062, 0, 0, 0]),
            ("battle_skills.id", [1062, 0, 0, 0]),
            ("battle_skills.index", [1062, 0, 0, 0]),
            ("blueprints", [433, 0, 0, 0]),
            ("constructions.index", [794, 0, 0, 0]),
            ("constructions.position.x", [794, 0, 0, 0]),
            ("constructions.position.y", [794, 0, 0, 0]),
            ("constructions.type", [794, 0, 0, 0]),
            ("contraptions.index", [0, 0, 0, 346]),
            ("contraptions.position.x", [0, 0, 0, 346]),
            ("contraptions.position.y", [0, 0, 0, 346]),
            ("contraptions.type", [0, 0, 0, 346]),
            ("equipment", [24, 0, 0, 0]),
            ("formations.equipment", [319, 0, 0, 0]),
            ("formations.exp", [0, 0, 0, 8902]),
            ("formations.index", [9494, 0, 0, 0]),
            ("formations.level", [2600, 0, 0, 0]),
            ("formations.movable", [455, 0, 0, 0]),
            ("formations.position.x", [9494, 0, 0, 0]),
            ("formations.position.y", [9494, 0, 0, 0]),
            ("formations.rotated", [3058, 0, 0, 0]),
            ("formations.type", [9494, 0, 0, 0]),
            ("formations.value", [9494, 0, 0, 0]),
            ("next_index.contraption", [668, 0, 0, 0]),
            ("next_index.unit", [668, 0, 0, 0]),
            ("reactor_core", [82, 0, 0, 586]),
            ("reinforce_offers", [293, 0, 0, 0]),
            ("shop.buys_remaining", [668, 0, 0, 0]),
            ("shop.unlocked_units", [668, 0, 0, 0]),
            ("shop.unlocks_remaining", [668, 0, 0, 0]),
            ("supply", [0, 0, 668, 0]),
            ("techs.officers", [668, 0, 0, 0]),
            ("techs.units", [389, 0, 0, 0]),
            ("terrains", [0, 0, 0, 10]),
            ("tower_strengthen_levels", [668, 0, 0, 0]),
        ]
        .into_iter()
        .map(|(group, [equal, unequal, unimplemented, fight])| {
            (
                group.to_owned(),
                Counts {
                    equal,
                    unequal,
                    unimplemented,
                    fight,
                },
            )
        })
        .collect();
        assert_eq!(fields, expected);
        assert_eq!(untargeted, 41);
    }

    fn first_battle() -> Stated {
        tracked().swap_remove(0).1
    }

    /// A recorded field the prediction covers, changed, is reported at its
    /// path; a format-valid wrong value does not pass. A changed opening
    /// disagrees with both transitions it joins: the one that should reach it,
    /// and the one that carries it forward.
    #[test]
    fn a_changed_covered_field_is_unequal_at_its_path() {
        let economy = Economy::embedded().unwrap();
        let mut stated = first_battle();
        stated.turns[2].state.sides.red.tower_strengthen_levels[0] += 1;
        stated.turns[2].state.sides.blue.formations[0]
            .formation
            .position
            .x += 10;
        let coverage = measured(&economy, &stated);
        let at: Vec<_> = coverage
            .unequal
            .iter()
            .map(|difference| (difference.round, difference.side, difference.path.as_str()))
            .collect();
        let index = stated.turns[2].state.sides.blue.formations[0]
            .formation
            .index;
        assert_eq!(
            at,
            [
                (
                    2,
                    "blue",
                    format!("formations[{index}].position.x").as_str()
                ),
                (2, "red", "tower_strengthen_levels"),
                (
                    3,
                    "blue",
                    format!("formations[{index}].position.x").as_str()
                ),
                (3, "red", "tower_strengthen_levels"),
            ]
        );
        assert!(!coverage.complete());
    }

    /// A decision dropped from a round is a position the record does not hold.
    #[test]
    fn a_dropped_decision_is_unequal_downstream() {
        let economy = Economy::embedded().unwrap();
        let mut stated = first_battle();
        let (round, at) = stated
            .turns
            .iter()
            .find_map(|turn| {
                let at = turn.actions.blue.iter().position(|action| {
                    matches!(action, crate::battle::Action::UpgradeTechnology { .. })
                })?;
                Some((turn.round, at))
            })
            .unwrap();
        let turn = stated
            .turns
            .iter_mut()
            .find(|turn| turn.round == round)
            .unwrap();
        turn.actions.blue.remove(at);
        let coverage = measured(&economy, &stated);
        let at: Vec<_> = coverage
            .unequal
            .iter()
            .map(|difference| (difference.round, difference.side, difference.path.as_str()))
            .collect();
        assert_eq!(at, [(round, "blue", "techs.units")]);
    }

    /// A decision naming a formation the position does not hold is the record
    /// contradicting the rules, not a rule the prediction lacks.
    #[test]
    fn a_decision_the_position_cannot_take_is_unequal() {
        let economy = Economy::embedded().unwrap();
        let mut stated = first_battle();
        stated.turns[2]
            .actions
            .red
            .push(crate::battle::Action::UpgradeUnit { index: 9_999 });
        let coverage = measured(&economy, &stated);
        let at: Vec<_> = coverage
            .unequal
            .iter()
            .map(|difference| (difference.round, difference.side, difference.path.as_str()))
            .collect();
        assert_eq!(at, [(stated.turns[2].round, "red", "actions")]);
        let red = coverage
            .transitions
            .iter()
            .find(|transition| {
                transition.round == stated.turns[2].round && transition.side == "red"
            })
            .unwrap();
        assert!(red.counts.unequal > 0 && red.counts.equal == 0, "{red:?}");
    }
}
