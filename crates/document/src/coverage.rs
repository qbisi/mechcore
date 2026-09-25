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

use crate::battle::{Action, Offers, SideState, SkillTarget, Turn};
use crate::economy::Economy;
use crate::opening::{Stated, Stream};
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
    "units.exp",
    "contraptions",
    "terrains",
    "airdrop_shields",
];

/// Field groups whose opening rule [`crate::transition::open_round`] does not
/// hold yet. Their leaves count as unimplemented whatever the prediction says,
/// because a value no rule produced can only agree by accident. A group leaves
/// this list with the rule that predicts it.
pub const UNIMPLEMENTED: &[&str] = &[];

/// The fields the reinforcement deal reads from the position it is dealt in.
/// A deal checked against the recorded position is a deal of the predicted
/// one only where these leaves agree on both sides.
const DEALT_FROM: &[&str] = &[
    "units.index",
    "units.name",
    "next_index.unit",
    "unlocked_units",
    "techs",
    "officers",
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

/// A round's offers as a document writes them.
fn render_offers(offers: &Offers) -> String {
    render(&to_value(offers))
}

/// A side's own stream from `seed`, `draws` values on.
fn player_stream(seed: Option<i32>, draws: u32) -> Option<Stream> {
    seed.map(|seed| {
        let mut stream = Stream::seeded(seed);
        stream.skip(draws);
        stream
    })
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
    // Each side's own stream, from the seed the header states, advanced once
    // for every hand-out a recorded round drew.
    let seeds = [stated.blue.seed, stated.red.seed];
    let mut draws = [0_u32; 2];
    for pair in stated.turns.windows(2) {
        let [turn, next] = pair else { continue };
        // What a decline pays is the round's own offer to state.
        let declined = turn
            .state
            .reinforce_offers
            .as_ref()
            .map(|offers| offers.refund);
        let mut dealt_from = true;
        for (at, (side, red)) in [("blue", false), ("red", true)].into_iter().enumerate() {
            let (state, actions, recorded) = sides(turn, next, red);
            draws[at] += crate::transition::player_draws(economy, &state.officers, turn.round);
            dealt_from &= coverage.side(
                economy,
                (turn.round, declined),
                state,
                actions,
                recorded,
                (side, player_stream(seeds[at], draws[at])),
            );
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
                &stated.blue,
                &stated.opening.blue,
                &first.state.blue,
            ),
            ("red", 1, &stated.red, &stated.opening.red, &first.state.red),
        ] {
            match crate::opening::reactor_core(stated.map_id, seat) {
                Ok(core) => {
                    let before = before_opening(core, header.constructions.clone());
                    let stream = player_stream(header.seed, 0);
                    self.side(
                        economy,
                        (0, None),
                        &before,
                        actions,
                        recorded,
                        (side, stream),
                    );
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
        (round, declined): (i32, Option<i32>),
        state: &SideState,
        actions: &[Action],
        recorded: &SideState,
        (side, stream): (&'static str, Option<Stream>),
    ) -> bool {
        let red = side == "red";
        // Round zero ends in round 1's opening without a fight.
        let fought = round > 0;
        let predicted =
            crate::transition::predict(economy, round, state, actions, red, declined, stream);
        let (leaves, unpredicted) = match &predicted {
            Ok(predicted) => {
                let mut leaves = compare(predicted, recorded, fought);
                if fought {
                    stray_shields(state, actions, recorded, &mut leaves);
                }
                (leaves, None)
            }
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
        let predicted = dealt.map(|round| Offers {
            dealt: round.offers.clone(),
            refund: round.declined,
        });
        let class = match &predicted {
            Some(offers) if offers == recorded && dealt_from => Class::Equal,
            Some(offers) if offers == recorded => Class::Unimplemented,
            _ => Class::Unequal,
        };
        if class == Class::Unequal {
            self.unequal.push(Difference {
                round: turn.round,
                side: "match",
                path: "reinforce_offers".into(),
                predicted: predicted.as_ref().map(render_offers),
                recorded: Some(render_offers(recorded)),
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
        (&turn.state.red, &turn.actions.red, &next.state.red)
    } else {
        (&turn.state.blue, &turn.actions.blue, &next.state.blue)
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
/// Marks the shield leaf unequal when the fight left standing a shield it
/// could not have: one this round neither released nor opened with.
///
/// Which shields survive is the fight's to decide, so the leaf is otherwise
/// the fight's. But a fight destroys shields and never places one, so what
/// stands after it is bounded by what stood before it and what this round
/// released, and a shield outside that is the record contradicting the rules.
fn stray_shields(state: &SideState, actions: &[Action], recorded: &SideState, leaves: &mut [Leaf]) {
    let mut possible = state.airdrop_shields.clone();
    for action in actions {
        if let Action::ReleaseCommanderSkill {
            id: crate::grbr::SHIELD_AIRDROP_SKILL,
            target: SkillTarget::Area(points),
            ..
        } = action
        {
            possible.extend(points.iter().copied());
        }
    }
    if recorded
        .airdrop_shields
        .iter()
        .all(|center| possible.contains(center))
    {
        return;
    }
    for leaf in leaves.iter_mut().filter(|leaf| leaf.0 == "airdrop_shields") {
        leaf.1 = Class::Unequal;
        leaf.2 = Some(format!("at most {}", render(&to_value(&possible))));
    }
}

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

    /// Every field a side's position can hold, as the dotted path a leaf is
    /// grouped under, read off the schema rather than off any battle.
    fn side_fields() -> Vec<String> {
        fn walk(
            node: &serde_json::Value,
            defs: &serde_json::Value,
            path: &str,
            out: &mut Vec<String>,
        ) {
            if !node.is_object() {
                return;
            }
            if let Some(name) = node["$ref"].as_str().and_then(|at| at.rsplit('/').next()) {
                walk(&defs[name], defs, path, out);
            }
            if let Some(fields) = node["properties"].as_object() {
                for (name, field) in fields {
                    let at = if path.is_empty() {
                        name.clone()
                    } else {
                        format!("{path}.{name}")
                    };
                    out.push(at.clone());
                    walk(field, defs, &at, out);
                }
            }
            walk(&node["items"], defs, path, out);
            for key in ["anyOf", "oneOf", "allOf"] {
                for branch in node[key].as_array().into_iter().flatten() {
                    walk(branch, defs, path, out);
                }
            }
        }
        let schema = serde_json::to_value(schemars::schema_for!(SideState)).unwrap();
        let mut out = Vec::new();
        walk(&schema, &schema["$defs"], "", &mut out);
        out
    }

    /// Every field the lists name is one a position can hold, so renaming a
    /// field cannot leave an entry that no leaf matches.
    #[test]
    fn every_listed_field_is_one_a_position_holds() {
        let fields = side_fields();
        assert!(fields.iter().any(|field| field == "units.exp"));
        for field in DEALT_FROM.iter().chain(FIGHT).chain(UNIMPLEMENTED) {
            assert!(
                fields.iter().any(|group| within(group, field)),
                "{field} names no field a position holds"
            );
        }
    }
}
