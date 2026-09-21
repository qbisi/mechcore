//! What differs between two recordings, field by field and tick by tick.
//!
//! A hash says whether two recordings agree; this says where they do not. Every
//! object a tick holds is flattened into its leaves — a unit's
//! `motion_state`, a weapon's `attack_target`, a building's `life.current` —
//! and the two recordings are compared leaf by leaf over every tick they both
//! hold. A leaf's **field group** is its path with the object and any list
//! position removed, so `unit 2`'s `weapon_aims[0].attack_target` is counted
//! under `units.weapon_aims.attack_target`: one group answers for that field
//! across every object and every tick.
//!
//! The groups are what make a content difference readable. The content hash
//! differs between the game and the simulator on every recording, for fields
//! nobody here is looking at, so a verdict on the whole layer says nothing; a
//! group that differs on one tick of one unit says exactly where to look.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use mechcore_mcfr::{EventKind, McfrReader, ObjectKind, ObjectRef, TickSlice};
use serde::Serialize;
use serde_json::{Map, Value};

/// How many differing leaves one tick's detail names before counting the rest.
const SHOWN: usize = 50;

/// Each collection a tick holds, the key it is written under and the field
/// that identifies one of its objects.
const COLLECTIONS: [(&str, &str, &str); 5] = [
    ("units", "live_units", "unit_id"),
    ("projectiles", "projectiles", "projectile_id"),
    ("buildings", "buildings", "building_id"),
    ("shields", "shields", "shield_id"),
    ("terrains", "terrains", "terrain_id"),
];

/// The field groups a comparison is asked about: every group, or those under
/// the named prefixes.
#[derive(Clone, Debug, Default)]
pub(crate) struct Selection(Vec<String>);

impl Selection {
    /// A selection of the named groups; a name selects itself and every group
    /// under it, so `units` selects every unit field.
    pub(crate) fn of(names: impl IntoIterator<Item = String>) -> Self {
        Self(
            names
                .into_iter()
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty())
                .collect(),
        )
    }

    pub(crate) fn is_everything(&self) -> bool {
        self.0.is_empty()
    }

    fn admits(&self, group: &str) -> bool {
        self.0.is_empty()
            || self.0.iter().any(|name| {
                group == name
                    || group
                        .strip_prefix(name.as_str())
                        .is_some_and(|rest| rest.starts_with('.'))
            })
    }
}

/// Where one field group differs.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) first_divergence: u32,
    pub(crate) last_divergence: u32,
    pub(crate) divergent_ticks: u32,
}

/// Every selected field group that differs, over every tick both recordings
/// hold.
pub(crate) struct Fields {
    pub(crate) compared_ticks: u32,
    pub(crate) groups: BTreeMap<String, Span>,
}

impl Fields {
    pub(crate) fn equal(&self) -> bool {
        self.groups.is_empty()
    }

    /// The earliest tick any selected group differs at.
    pub(crate) fn first_divergence(&self) -> Option<u32> {
        self.groups.values().map(|span| span.first_divergence).min()
    }

    /// The groups as a nested map, `units.motion_state` under `units`, so an
    /// expectation reaches one by its dotted path.
    pub(crate) fn nested(&self) -> Value {
        let mut root = Map::new();
        for (group, span) in &self.groups {
            let mut node = &mut root;
            for part in group.split('.') {
                node = node
                    .entry(part)
                    .or_insert_with(|| Value::Object(Map::new()))
                    .as_object_mut()
                    .expect("every node is a map");
            }
            if let Value::Object(fields) = serde_json::to_value(span).expect("a span serializes") {
                node.extend(fields);
            }
        }
        Value::Object(root)
    }
}

/// Compares every tick both recordings hold, group by group.
///
/// # Errors
///
/// Returns an error when a tick cannot be read.
pub(crate) fn fields(
    left: &McfrReader,
    right: &McfrReader,
    selection: &Selection,
) -> Result<Fields, String> {
    let compared_ticks = left.tick_count().min(right.tick_count());
    let mut groups: BTreeMap<String, Span> = BTreeMap::new();
    for tick in 1..=compared_ticks {
        let (left_tick, right_tick) = (read(left, tick)?, read(right, tick)?);
        let differing: BTreeSet<String> = differences(&left_tick, &right_tick)
            .into_iter()
            .map(|difference| difference.group)
            .filter(|group| selection.admits(group))
            .collect();
        for group in differing {
            groups
                .entry(group)
                .and_modify(|span| {
                    span.last_divergence = tick;
                    span.divergent_ticks += 1;
                })
                .or_insert(Span {
                    first_divergence: tick,
                    last_divergence: tick,
                    divergent_ticks: 1,
                });
        }
    }
    Ok(Fields {
        compared_ticks,
        groups,
    })
}

/// One tick, explained: what differs there, what happened around it, and
/// where every object the differences name stands.
#[derive(Serialize)]
pub(crate) struct Detail {
    pub(crate) tick: u32,
    pub(crate) differences: Vec<Shown>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) further: Option<usize>,
    /// Each side's events on this tick and the one before, one line each.
    pub(crate) events: Sides<Vec<String>>,
    /// Every object a difference names, as each side has it at this tick.
    pub(crate) references: BTreeMap<String, Sides<String>>,
}

#[derive(Serialize)]
pub(crate) struct Shown {
    pub(crate) object: String,
    pub(crate) field: String,
    pub(crate) left: String,
    pub(crate) right: String,
}

#[derive(Serialize)]
pub(crate) struct Sides<T> {
    pub(crate) left: T,
    pub(crate) right: T,
}

/// Explains one tick both recordings hold.
///
/// # Errors
///
/// Returns an error when the tick is outside either recording or cannot be
/// read.
pub(crate) fn detail(
    left: &McfrReader,
    right: &McfrReader,
    tick: u32,
    selection: &Selection,
) -> Result<Detail, String> {
    let held = left.tick_count().min(right.tick_count());
    if tick == 0 || tick > held {
        return Err(format!(
            "tick {tick} is not one both recordings hold; they hold 1 to {held}"
        ));
    }
    let (left_tick, right_tick) = (read(left, tick)?, read(right, tick)?);
    let differing: Vec<Difference> = differences(&left_tick, &right_tick)
        .into_iter()
        .filter(|difference| selection.admits(&difference.group))
        .collect();
    let mut named = BTreeSet::new();
    for difference in &differing {
        for value in [&difference.left, &difference.right].into_iter().flatten() {
            references_in(value, &mut named);
        }
    }
    let references = named
        .into_iter()
        .map(|reference| {
            Ok((
                name(reference),
                Sides {
                    left: describe(left, &left_tick, reference)?,
                    right: describe(right, &right_tick, reference)?,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    let further = differing
        .len()
        .checked_sub(SHOWN)
        .filter(|further| *further > 0);
    Ok(Detail {
        tick,
        differences: differing
            .into_iter()
            .take(SHOWN)
            .map(|difference| Shown {
                object: difference.object,
                field: difference.field,
                left: difference
                    .left
                    .as_ref()
                    .map_or_else(|| "absent".into(), render),
                right: difference
                    .right
                    .as_ref()
                    .map_or_else(|| "absent".into(), render),
            })
            .collect(),
        further,
        events: Sides {
            left: around(left, tick)?,
            right: around(right, tick)?,
        },
        references,
    })
}

/// One leaf that differs at one tick.
struct Difference {
    group: String,
    object: String,
    field: String,
    left: Option<Value>,
    right: Option<Value>,
}

fn read(reader: &McfrReader, tick: u32) -> Result<TickSlice, String> {
    reader.tick(tick).map_err(|error| error.to_string())
}

/// Every leaf that differs between two ticks, objects first and then events.
///
/// An object only one side holds differs once, in its `present` field,
/// rather than in every leaf it has.
fn differences(left: &TickSlice, right: &TickSlice) -> Vec<Difference> {
    let (left_objects, right_objects) = (objects(&left.state), objects(&right.state));
    let keys: BTreeSet<&(&str, u64)> = left_objects.keys().chain(right_objects.keys()).collect();
    let mut out = Vec::new();
    for key in keys {
        let (collection, id) = *key;
        let object = format!("{} {id}", singular(collection));
        match (left_objects.get(key), right_objects.get(key)) {
            (Some(left_leaves), Some(right_leaves)) => {
                let paths: BTreeSet<&String> =
                    left_leaves.keys().chain(right_leaves.keys()).collect();
                for path in paths {
                    let (left_value, right_value) = (left_leaves.get(path), right_leaves.get(path));
                    if left_value != right_value {
                        out.push(Difference {
                            group: format!("{collection}.{}", without_positions(path)),
                            object: object.clone(),
                            field: path.clone(),
                            left: left_value.cloned(),
                            right: right_value.cloned(),
                        });
                    }
                }
            }
            (left_leaves, right_leaves) => out.push(Difference {
                group: format!("{collection}.present"),
                object,
                field: "present".into(),
                left: Some(Value::Bool(left_leaves.is_some())),
                right: Some(Value::Bool(right_leaves.is_some())),
            }),
        }
    }
    let (left_events, right_events) = (events(left), events(right));
    if left_events != right_events {
        out.push(Difference {
            group: "events".into(),
            object: "events".into(),
            field: "events".into(),
            left: Some(Value::from(left_events)),
            right: Some(Value::from(right_events)),
        });
    }
    out
}

/// A tick's objects, each flattened into its leaves.
fn objects(
    state: &mechcore_mcfr::WorldSnapshot,
) -> BTreeMap<(&'static str, u64), BTreeMap<String, Value>> {
    let written = serde_json::to_value(state).unwrap_or(Value::Null);
    let mut out = BTreeMap::new();
    for (collection, key, identity) in COLLECTIONS {
        for item in written[key].as_array().into_iter().flatten() {
            let Some(id) = item[identity].as_u64() else {
                continue;
            };
            let mut leaves = BTreeMap::new();
            if let Value::Object(fields) = item {
                for (field, value) in fields.iter().filter(|(field, _)| *field != identity) {
                    flatten(field, value, &mut leaves);
                }
            }
            out.insert((collection, id), leaves);
        }
    }
    out
}

/// Flattens one value into leaves under `path`.
///
/// A reference to another object is one leaf, because it names that object
/// rather than holding it. A list of objects is flattened by position, and a
/// list of scalars is one leaf. What is absent — a null, an empty list — has no
/// leaf, so a field that is absent on one side and present on the other
/// differs as `absent` against its value.
fn flatten(path: &str, value: &Value, leaves: &mut BTreeMap<String, Value>) {
    match value {
        Value::Null => {}
        Value::Object(fields) if reference_of(value).is_some() || fields.is_empty() => {
            if !fields.is_empty() {
                leaves.insert(path.to_owned(), value.clone());
            }
        }
        Value::Object(fields) => {
            for (field, inner) in fields {
                flatten(&format!("{path}.{field}"), inner, leaves);
            }
        }
        Value::Array(items) if items.is_empty() => {}
        Value::Array(items) if items.iter().any(Value::is_object) => {
            for (at, item) in items.iter().enumerate() {
                flatten(&format!("{path}[{at}]"), item, leaves);
            }
        }
        other => {
            leaves.insert(path.to_owned(), other.clone());
        }
    }
}

fn reference_of(value: &Value) -> Option<ObjectRef> {
    let fields = value.as_object()?;
    if fields.len() == 2 && fields.contains_key("kind") && fields.contains_key("id") {
        serde_json::from_value(value.clone()).ok()
    } else {
        None
    }
}

fn references_in(value: &Value, found: &mut BTreeSet<ObjectRef>) {
    if let Some(reference) = reference_of(value) {
        found.insert(reference);
        return;
    }
    match value {
        Value::Array(items) => items.iter().for_each(|item| references_in(item, found)),
        Value::Object(fields) => fields.values().for_each(|item| references_in(item, found)),
        Value::String(line) => {
            // An event line names objects in the same spelling `name` writes.
            let words: Vec<&str> = line.split_whitespace().collect();
            for pair in words.windows(2) {
                if let (Some(kind), Ok(id)) = (kind_named(pair[0]), pair[1].parse::<u64>()) {
                    found.insert(ObjectRef { kind, id });
                }
            }
        }
        _ => {}
    }
}

fn without_positions(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut inside = false;
    for character in path.chars() {
        match character {
            '[' => inside = true,
            ']' => inside = false,
            other if !inside => out.push(other),
            _ => {}
        }
    }
    out
}

fn singular(collection: &str) -> &str {
    collection.strip_suffix('s').unwrap_or(collection)
}

const KINDS: [(ObjectKind, &str); 5] = [
    (ObjectKind::Unit, "unit"),
    (ObjectKind::Projectile, "projectile"),
    (ObjectKind::Building, "building"),
    (ObjectKind::Shield, "shield"),
    (ObjectKind::Terrain, "terrain"),
];

fn kind_named(word: &str) -> Option<ObjectKind> {
    KINDS
        .iter()
        .find(|(_, spelled)| *spelled == word)
        .map(|(kind, _)| *kind)
}

fn name(reference: ObjectRef) -> String {
    let spelled = KINDS
        .iter()
        .find(|(kind, _)| *kind == reference.kind)
        .map_or("object", |(_, spelled)| spelled);
    format!("{spelled} {}", reference.id)
}

/// A leaf as a person reads it: a reference by name, a string bare,
/// everything else as compact JSON.
fn render(value: &Value) -> String {
    if let Some(reference) = reference_of(value) {
        return name(reference);
    }
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) if items.iter().all(Value::is_string) => format!(
            "[{}]",
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        other => other.to_string(),
    }
}

/// A tick's events, one line each, in the order they were recorded.
fn events(tick: &TickSlice) -> Vec<String> {
    tick.events
        .events
        .iter()
        .map(|event| {
            let mut line = serde_json::to_value(event.kind())
                .ok()
                .and_then(|kind| kind.as_str().map(str::to_owned))
                .unwrap_or_default();
            for (label, reference) in [
                ("", event.subject),
                ("from", event.source),
                ("at", event.target),
            ] {
                if let Some(reference) = reference {
                    if !label.is_empty() {
                        line.push(' ');
                        line.push_str(label);
                    }
                    line.push(' ');
                    line.push_str(&name(reference));
                }
            }
            if let Ok(Value::Object(payload)) = serde_json::to_value(&event.payload) {
                for (field, value) in payload.iter().filter(|(field, _)| *field != "kind") {
                    if !value.is_null() {
                        let _ = write!(line, " {field}={}", render(value));
                    }
                }
            }
            line
        })
        .collect()
}

/// The events of a tick and the one before it, each line prefixed with its
/// tick, because what explains a difference has usually just happened.
fn around(reader: &McfrReader, tick: u32) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for at in tick.saturating_sub(1).max(1)..=tick {
        for line in events(&read(reader, at)?) {
            out.push(format!("t{at} {line}"));
        }
    }
    Ok(out)
}

/// Where one referenced object stands on one side at a tick: alive and how
/// much, or gone and since when.
fn describe(reader: &McfrReader, at: &TickSlice, reference: ObjectRef) -> Result<String, String> {
    let state = &at.state;
    let standing = match reference.kind {
        ObjectKind::Unit => state
            .live_units
            .iter()
            .find(|unit| unit.unit_id == reference.id)
            .map(|unit| format!("life {} of {}", unit.life.current, unit.life.maximum)),
        ObjectKind::Building => state
            .buildings
            .iter()
            .find(|building| building.building_id == reference.id && building.life.current > 0)
            .map(|building| {
                format!(
                    "life {} of {}",
                    building.life.current, building.life.maximum
                )
            }),
        ObjectKind::Projectile => state
            .projectiles
            .iter()
            .any(|projectile| projectile.projectile_id == reference.id)
            .then(|| "in flight".to_owned()),
        ObjectKind::Shield => state
            .shields
            .iter()
            .any(|shield| shield.shield_id == reference.id)
            .then(|| "standing".to_owned()),
        ObjectKind::Terrain => state
            .terrains
            .iter()
            .any(|terrain| terrain.terrain_id == reference.id)
            .then(|| "standing".to_owned()),
    };
    if let Some(standing) = standing {
        return Ok(standing);
    }
    let ended = match reference.kind {
        ObjectKind::Unit => Some((EventKind::UnitDied, "died")),
        ObjectKind::Building => Some((EventKind::BuildingDestroyed, "destroyed")),
        ObjectKind::Projectile => Some((EventKind::ProjectileRemoved, "removed")),
        ObjectKind::Shield => Some((EventKind::ShieldDestroyed, "destroyed")),
        ObjectKind::Terrain => Some((EventKind::TerrainRemoved, "removed")),
    };
    if let Some((kind, verb)) = ended {
        for tick in (1..=at.tick).rev() {
            let slice = if tick == at.tick {
                None
            } else {
                Some(read(reader, tick)?)
            };
            let events = &slice.as_ref().unwrap_or(at).events.events;
            if events
                .iter()
                .any(|event| event.kind() == kind && event.subject == Some(reference))
            {
                return Ok(format!("{verb} at t{tick}"));
            }
        }
    }
    Ok("absent".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_is_the_path_without_positions() {
        assert_eq!(
            without_positions("weapon_aims[0].attack_target"),
            "weapon_aims.attack_target"
        );
        assert_eq!(without_positions("life.current"), "life.current");
    }

    #[test]
    fn a_selection_takes_a_group_and_everything_under_it() {
        let selection = Selection::of(["units.weapon_aims".to_owned(), "events".to_owned()]);
        assert!(selection.admits("units.weapon_aims.attack_target"));
        assert!(selection.admits("events"));
        assert!(!selection.admits("units.weapon_aims_extra"));
        assert!(!selection.admits("units.motion_state"));
        assert!(Selection::default().admits("anything"));
    }

    #[test]
    fn a_reference_is_one_leaf_and_absence_is_none() {
        let mut leaves = BTreeMap::new();
        flatten(
            "unit",
            &serde_json::json!({
                "lock": {"kind": "unit", "id": 2},
                "gone": null,
                "empty": [],
                "aims": [{"target": {"kind": "building", "id": 6}}],
                "tags": ["a", "b"],
            }),
            &mut leaves,
        );
        assert_eq!(
            leaves.keys().collect::<Vec<_>>(),
            ["unit.aims[0].target", "unit.lock", "unit.tags"]
        );
        assert_eq!(render(&leaves["unit.aims[0].target"]), "building 6");
        assert_eq!(render(&leaves["unit.tags"]), "[a; b]");
    }

    #[test]
    fn nested_groups_are_reachable_by_their_dotted_path() {
        let fields = Fields {
            compared_ticks: 20,
            groups: BTreeMap::from([(
                "units.motion_state".to_owned(),
                Span {
                    first_divergence: 19,
                    last_divergence: 19,
                    divergent_ticks: 1,
                },
            )]),
        };
        let nested = fields.nested();
        assert_eq!(nested["units"]["motion_state"]["first_divergence"], 19);
        assert_eq!(fields.first_divergence(), Some(19));
    }

    #[test]
    fn an_event_line_names_the_objects_a_reference_would() {
        let mut found = BTreeSet::new();
        references_in(
            &Value::from("building_destroyed building 6 from unit 2"),
            &mut found,
        );
        assert_eq!(
            found.into_iter().map(name).collect::<Vec<_>>(),
            ["unit 2", "building 6"]
        );
    }
}
