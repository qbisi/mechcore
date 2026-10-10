//! Layouts drawn so that every two decisions a fight tells apart meet.
//!
//! `docs/spec/document/generate.md` is the contract: the factors a layout is
//! an assignment of, the pairs of values a batch covers, how a candidate is
//! realized as a layout the game fights, and why a batch is a function of its
//! seed alone.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::catalog::{
    CHAIN_BLUEPRINTS, battle_skill_type_from_id, resolve_battle_skill_type, unit_type_from_id,
};
use crate::compile::{compile_layout, grid_center_remainder};
use crate::economy::{Economy, OpeningKind};
use crate::layout::{
    BattleSkillEntry, BattleSkillRelease, ContraptionPlacement, Experience, Layout, Position, Side,
    StaticPlacement, UnitPlacement, UnitSource,
};
use crate::layout_replay::{DEFAULT_MAP_ID, layout_replay};
use crate::{DocumentKind, MOVEMENT_ENHANCEMENT_SKILL, RANGE_ENHANCEMENT_SKILL};

const OFFICER_EFFECTS: &str = include_str!("../../../config/officer_effects.yaml");

/// The candidates drawn for each layout while pairs remain uncovered.
const CANDIDATES: usize = 10;
/// The placements tried for one candidate before it is dropped.
const PLACEMENT_TRIES: usize = 20;
/// The draws tried for one layout once every pair is covered.
const RANDOM_TRIES: usize = 100;

const ROUNDS: [i32; 4] = [1, 2, 3, 4];
const LEVELS: usize = 9;
const MAX_MODIFICATIONS: usize = 4;
const CONTRAPTIONS: [&str; 3] = ["shield", "interceptor", "missile"];

const LEGACY: usize = 0;
const FLANK: usize = 3;
/// The width of a flank, which a footprint has to fit across one way or the other.
const FLANK_WIDTH: i64 = 60;

/// A side's factors, in the order the space lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    LeadType,
    LeadLevel,
    LeadSource,
    LeadExp,
    LeadEquipment,
    LeadTechs,
    LeadModification,
    LeadRotated,
    LeadDepth,
    SecondType,
    SecondLevel,
    OfficerFirst,
    OfficerSecond,
    Opening,
    Blueprint,
    TowerSkills,
    TowerLevels,
    Construction,
    Contraption,
    BattleSkill,
}

const ROLES: [Role; 20] = [
    Role::LeadType,
    Role::LeadLevel,
    Role::LeadSource,
    Role::LeadExp,
    Role::LeadEquipment,
    Role::LeadTechs,
    Role::LeadModification,
    Role::LeadRotated,
    Role::LeadDepth,
    Role::SecondType,
    Role::SecondLevel,
    Role::OfficerFirst,
    Role::OfficerSecond,
    Role::Opening,
    Role::Blueprint,
    Role::TowerSkills,
    Role::TowerLevels,
    Role::Construction,
    Role::Contraption,
    Role::BattleSkill,
];

const SIDES: [&str; 2] = ["blue", "red"];

/// The factor of `role` on side `side`; factor 0 is the round.
const fn factor(side: usize, role: Role) -> usize {
    1 + side * ROLES.len() + role as usize
}

/// The random stream a batch draws every choice from: `SplitMix64`, which a
/// seed starts and nothing else moves.
struct Stream(u64);

impl Stream {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n`, for `n` above zero.
    #[allow(clippy::cast_possible_truncation)]
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// A value in `low..=high`.
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation
    )]
    fn between(&mut self, low: i64, high: i64) -> i64 {
        low + self.below((high - low + 1) as usize) as i64
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for at in (1..items.len()).rev() {
            items.swap(at, self.below(at + 1));
        }
    }

    /// A layout's own seed, which is positive: the opening its constructions
    /// come from is dealt for a positive seed.
    #[allow(clippy::cast_possible_truncation)]
    fn seed(&mut self) -> i32 {
        loop {
            let seed = (self.next() >> 33) as i32;
            if seed > 0 {
                return seed;
            }
        }
    }
}

#[derive(Deserialize)]
struct EffectFile {
    officers: Vec<EffectRow>,
}

#[derive(Deserialize)]
struct EffectRow {
    id: i32,
    mech_type: i32,
    #[serde(default)]
    units: Vec<String>,
}

/// What each factor's values stand for, read from the build's tables.
struct Space {
    types: Vec<(i32, &'static str, (i64, i64))>,
    equipment: Vec<i32>,
    technologies: BTreeMap<i32, Vec<i32>>,
    modifications: BTreeMap<String, Vec<i32>>,
    generic: Vec<i32>,
    /// Which generic officers a side may hold twice.
    repeatable: Vec<bool>,
    openings: Vec<i32>,
    skills: Vec<&'static str>,
    sizes: Vec<usize>,
}

impl Space {
    fn read() -> Result<Self, String> {
        let economy = Economy::embedded()?;
        let types: Vec<_> = (1..=3000)
            .filter_map(|id| unit_type_from_id(id).map(|(name, footprint)| (id, name, footprint)))
            .collect();
        let effects: EffectFile = serde_yaml::from_str(OFFICER_EFFECTS)
            .map_err(|error| format!("config/officer_effects.yaml: {error}"))?;
        let openings: Vec<i32> = economy
            .advance_teams()
            .filter(|(id, team)| {
                team.kind == OpeningKind::Officer
                    && (effects.officers.iter().any(|row| row.id == *id)
                        || economy
                            .officer(*id)
                            .is_some_and(|officer| officer.opening_unit.is_some()))
            })
            .map(|(id, _)| id)
            .collect();
        let mut modifications: BTreeMap<String, Vec<i32>> = BTreeMap::new();
        let mut generic = Vec::new();
        for row in &effects.officers {
            if CHAIN_BLUEPRINTS
                .iter()
                .any(|(_, officer)| *officer == row.id)
                || openings.contains(&row.id)
            {
                continue;
            }
            match (row.mech_type, row.units.as_slice()) {
                (10, [unit]) => modifications.entry(unit.clone()).or_default().push(row.id),
                _ => generic.push(row.id),
            }
        }
        for officers in modifications.values_mut() {
            officers.sort_unstable();
            if officers.len() > MAX_MODIFICATIONS {
                return Err(format!(
                    "a unit has {} modifications, more than the {MAX_MODIFICATIONS} the space takes",
                    officers.len()
                ));
            }
        }
        generic.sort_unstable();
        let skills = crate::names::commander_skill_ids()
            .into_iter()
            .filter_map(battle_skill_type_from_id)
            .filter(|name| resolve_battle_skill_type(name).is_some())
            .collect();
        let mut space = Self {
            types,
            equipment: crate::names::equipment_ids(),
            technologies: economy.unit_technologies(),
            modifications,
            repeatable: generic
                .iter()
                .map(|officer| crate::reinforcement::repeatable(*officer))
                .collect::<Result<_, _>>()?,
            generic,
            openings,
            skills,
            sizes: Vec::new(),
        };
        space.sizes = std::iter::once(ROUNDS.len())
            .chain((0..SIDES.len()).flat_map(|_| ROLES.map(|role| space.values(role))))
            .collect();
        Ok(space)
    }

    fn values(&self, role: Role) -> usize {
        match role {
            Role::LeadType => self.types.len(),
            Role::LeadLevel | Role::SecondLevel => LEVELS,
            Role::LeadSource | Role::LeadRotated => 2,
            Role::LeadExp | Role::LeadTechs | Role::TowerLevels | Role::Construction => 3,
            Role::LeadEquipment => 1 + self.equipment.len(),
            Role::LeadModification => 1 + MAX_MODIFICATIONS,
            Role::LeadDepth | Role::TowerSkills | Role::Contraption => 4,
            Role::SecondType => 1 + self.types.len(),
            Role::OfficerFirst | Role::OfficerSecond => 1 + self.generic.len(),
            Role::Opening => 1 + self.openings.len(),
            Role::Blueprint => 1 + CHAIN_BLUEPRINTS.len(),
            Role::BattleSkill => 1 + self.skills.len(),
        }
    }

    /// The factor and value a report names, as `blue.lead.type=marksman`.
    fn label(&self, factor: usize, value: usize) -> String {
        if factor == 0 {
            return format!("round={}", ROUNDS[value]);
        }
        let side = SIDES[(factor - 1) / ROLES.len()];
        let role = ROLES[(factor - 1) % ROLES.len()];
        let none = |value: usize, name: &dyn Fn(usize) -> String| {
            if value == 0 {
                "none".to_owned()
            } else {
                name(value - 1)
            }
        };
        let officer =
            |id: i32| crate::names::officer_name(id).map_or_else(|| id.to_string(), str::to_owned);
        let (field, text) = match role {
            Role::LeadType => ("lead.type", self.types[value].1.to_owned()),
            Role::LeadLevel => ("lead.level", (value + 1).to_string()),
            Role::LeadSource => ("lead.source", ["legacy", "joined"][value].to_owned()),
            Role::LeadExp => ("lead.exp", ["empty", "half", "full"][value].to_owned()),
            Role::LeadEquipment => (
                "lead.equipment",
                none(value, &|at| {
                    crate::names::equipment_name(self.equipment[at])
                        .unwrap_or("?")
                        .to_owned()
                }),
            ),
            Role::LeadTechs => ("lead.techs", ["none", "one", "all"][value].to_owned()),
            Role::LeadModification => {
                ("lead.modification", none(value, &|at| (at + 1).to_string()))
            }
            Role::LeadRotated => ("lead.rotated", (value == 1).to_string()),
            Role::LeadDepth => (
                "lead.depth",
                ["front", "middle", "back", "flank"][value].to_owned(),
            ),
            Role::SecondType => (
                "second.type",
                none(value, &|at| self.types[at].1.to_owned()),
            ),
            Role::SecondLevel => ("second.level", (value + 1).to_string()),
            Role::OfficerFirst => (
                "officer.first",
                none(value, &|at| officer(self.generic[at])),
            ),
            Role::OfficerSecond => (
                "officer.second",
                none(value, &|at| officer(self.generic[at])),
            ),
            Role::Opening => ("opening", none(value, &|at| officer(self.openings[at]))),
            Role::Blueprint => (
                "blueprint",
                none(value, &|at| CHAIN_BLUEPRINTS[at].0.to_string()),
            ),
            Role::TowerSkills => (
                "energy_tower_skills",
                ["none", "enhanced_range", "high_mobility", "both"][value].to_owned(),
            ),
            Role::TowerLevels => (
                "tower_strengthen_levels",
                ["none", "random", "max"][value].to_owned(),
            ),
            Role::Construction => (
                "constructions",
                ["none", "opening", "part"][value].to_owned(),
            ),
            Role::Contraption => (
                "contraption",
                none(value, &|at| CONTRAPTIONS[at].to_owned()),
            ),
            Role::BattleSkill => (
                "battle_skill",
                none(value, &|at| self.skills[at].to_owned()),
            ),
        };
        format!("{side}.{field}={text}")
    }

    /// Whether the values assigned so far may stand together, by the rules
    /// the spec's "Values that need others" lists.
    fn admits(&self, assigned: &[Option<usize>]) -> bool {
        let round = assigned[0];
        (0..SIDES.len()).all(|side| {
            let at = |role| assigned[factor(side, role)];
            let source = at(Role::LeadSource);
            let depth = at(Role::LeadDepth);
            let lead = at(Role::LeadType).map(|value| self.types[value]);
            !(matches!((source, at(Role::LeadExp)), (Some(source), Some(exp)) if source != LEGACY && exp != 0)
                || matches!((depth, round), (Some(FLANK), Some(0)))
                || matches!((depth, lead), (Some(FLANK), Some((_, _, (width, height))))
                    if width.min(height) > FLANK_WIDTH)
                || matches!((depth, source, round), (Some(FLANK), Some(LEGACY), Some(1)))
                || matches!((lead, at(Role::LeadModification)), (Some((_, name, _)), Some(value))
                    if value > self.modifications.get(name).map_or(0, Vec::len))
                || matches!((lead, at(Role::LeadTechs)), (Some((id, _, _)), Some(value))
                    if value > 0 && self.technologies.get(&id).is_none_or(Vec::is_empty))
                || matches!((at(Role::SecondType), at(Role::SecondLevel)), (Some(0), Some(level)) if level != 0)
                || matches!((at(Role::OfficerFirst), at(Role::OfficerSecond)), (Some(first), Some(second))
                    if first > 0 && first == second && !self.repeatable[first - 1]))
        })
    }
}

/// Every pair of values of two factors, numbered, and which are covered.
struct Pairs {
    /// `bases[a][b]` for `a < b`: where the pairs of factors `a` and `b` start.
    bases: Vec<Vec<usize>>,
    sizes: Vec<usize>,
    admitted: Vec<bool>,
    covered: Vec<bool>,
    /// The pairs still to cover, which a draw starts a candidate from;
    /// covered ones are removed as a draw meets them.
    open: Vec<usize>,
}

impl Pairs {
    fn new(space: &Space) -> Self {
        let sizes = space.sizes.clone();
        let mut bases = vec![vec![0; sizes.len()]; sizes.len()];
        let mut count = 0;
        for a in 0..sizes.len() {
            for b in a + 1..sizes.len() {
                bases[a][b] = count;
                count += sizes[a] * sizes[b];
            }
        }
        let mut pairs = Self {
            bases,
            sizes,
            admitted: vec![false; count],
            covered: vec![false; count],
            open: Vec::new(),
        };
        let mut assigned = vec![None; pairs.sizes.len()];
        for a in 0..pairs.sizes.len() {
            for b in a + 1..pairs.sizes.len() {
                for va in 0..pairs.sizes[a] {
                    for vb in 0..pairs.sizes[b] {
                        assigned[a] = Some(va);
                        assigned[b] = Some(vb);
                        if space.admits(&assigned) {
                            let index = pairs.index(a, va, b, vb);
                            pairs.admitted[index] = true;
                            pairs.open.push(index);
                        }
                    }
                }
                assigned[a] = None;
                assigned[b] = None;
            }
        }
        pairs
    }

    fn index(&self, a: usize, va: usize, b: usize, vb: usize) -> usize {
        let (a, va, b, vb) = if a < b {
            (a, va, b, vb)
        } else {
            (b, vb, a, va)
        };
        self.bases[a][b] + va * self.sizes[b] + vb
    }

    fn decode(&self, index: usize) -> (usize, usize, usize, usize) {
        for a in 0..self.sizes.len() {
            for b in a + 1..self.sizes.len() {
                let base = self.bases[a][b];
                let span = self.sizes[a] * self.sizes[b];
                if (base..base + span).contains(&index) {
                    let offset = index - base;
                    return (a, offset / self.sizes[b], b, offset % self.sizes[b]);
                }
            }
        }
        unreachable!("a pair index names a pair")
    }

    fn is_new(&self, index: usize) -> bool {
        self.admitted[index] && !self.covered[index]
    }

    /// An uncovered pair to start a candidate from, or none once all are.
    fn draw(&mut self, stream: &mut Stream) -> Option<usize> {
        while !self.open.is_empty() {
            let at = stream.below(self.open.len());
            let index = self.open[at];
            if self.is_new(index) {
                return Some(index);
            }
            self.open.swap_remove(at);
        }
        None
    }

    fn drop_open(&mut self, index: usize) {
        self.open.retain(|open| *open != index);
    }

    /// The pairs an assignment covers that none covered before it.
    fn gain(&self, assignment: &[usize]) -> usize {
        let mut gain = 0;
        for a in 0..assignment.len() {
            for b in a + 1..assignment.len() {
                if self.is_new(self.index(a, assignment[a], b, assignment[b])) {
                    gain += 1;
                }
            }
        }
        gain
    }

    fn cover(&mut self, assignment: &[usize]) {
        for a in 0..assignment.len() {
            for b in a + 1..assignment.len() {
                let index = self.index(a, assignment[a], b, assignment[b]);
                self.covered[index] = true;
            }
        }
    }
}

/// One layout of a batch.
pub struct Generated {
    /// Its place in the batch, which with the batch's seed draws it again.
    pub index: usize,
    pub layout: Layout,
    /// The pairs it covered that no layout before it did.
    pub new_pairs: usize,
}

/// A pair no layout covered, and why when a candidate holding it was refused.
pub struct Uncovered {
    pub pair: (String, String),
    pub refusal: Option<String>,
}

pub struct Batch {
    pub layouts: Vec<Generated>,
    /// The pairs the rules admit.
    pub pairs: usize,
    pub covered: usize,
    pub uncovered: Vec<Uncovered>,
}

/// Draws the first `count` layouts of the batch `seed` starts.
///
/// # Errors
///
/// Returns the reason when the build's tables do not read, or when no draw
/// realizes a layout once every pair is covered.
pub fn generate(seed: u64, count: usize) -> Result<Batch, String> {
    let space = Space::read()?;
    let mut pairs = Pairs::new(&space);
    let mut stream = Stream(seed);
    let mut refusals: BTreeMap<usize, String> = BTreeMap::new();
    let mut layouts = Vec::with_capacity(count);
    for index in 0..count {
        let (assignment, layout) = loop {
            if pairs.draw(&mut stream).is_none() {
                break random(&space, &mut stream)?;
            }
            let mut candidates = Vec::with_capacity(CANDIDATES);
            for _ in 0..CANDIDATES {
                let Some(start) = pairs.draw(&mut stream) else {
                    break;
                };
                // A candidate whose shuffled order reaches a factor no value
                // of which stands with those before it is drawn again.
                let Some(assignment) = candidate(&space, &pairs, &mut stream, start) else {
                    continue;
                };
                candidates.push((pairs.gain(&assignment), start, assignment));
            }
            // The most pairs first; a stable sort keeps the draw's order
            // among equals.
            candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
            let mut kept = None;
            for (_, start, assignment) in &candidates {
                match realize(&space, assignment, &mut stream) {
                    Ok(layout) => {
                        kept = Some((assignment.clone(), layout));
                        break;
                    }
                    Err(reason) => {
                        refusals.insert(*start, reason);
                    }
                }
            }
            if let Some(kept) = kept {
                break kept;
            }
            for (_, start, _) in &candidates {
                pairs.drop_open(*start);
            }
        };
        let new_pairs = pairs.gain(&assignment);
        pairs.cover(&assignment);
        layouts.push(Generated {
            index,
            layout,
            new_pairs,
        });
    }
    let admitted = pairs.admitted.iter().filter(|admitted| **admitted).count();
    let mut uncovered = Vec::new();
    let mut covered = 0;
    for (index, admitted) in pairs.admitted.iter().enumerate() {
        if !admitted {
            continue;
        }
        if pairs.covered[index] {
            covered += 1;
            continue;
        }
        let (a, va, b, vb) = pairs.decode(index);
        uncovered.push(Uncovered {
            pair: (space.label(a, va), space.label(b, vb)),
            refusal: refusals.get(&index).cloned(),
        });
    }
    Ok(Batch {
        layouts,
        pairs: admitted,
        covered,
        uncovered,
    })
}

/// A candidate started from one uncovered pair, each further factor the
/// value that covers the most uncovered pairs with those already assigned;
/// none when a factor is reached that no value of stands with them.
fn candidate(
    space: &Space,
    pairs: &Pairs,
    stream: &mut Stream,
    start: usize,
) -> Option<Vec<usize>> {
    let (a, va, b, vb) = pairs.decode(start);
    let mut assigned = vec![None; space.sizes.len()];
    assigned[a] = Some(va);
    assigned[b] = Some(vb);
    let mut order: Vec<usize> = (0..space.sizes.len())
        .filter(|factor| *factor != a && *factor != b)
        .collect();
    stream.shuffle(&mut order);
    for factor in order {
        let mut best = None;
        let mut ties = 0;
        for value in 0..space.sizes[factor] {
            assigned[factor] = Some(value);
            if !space.admits(&assigned) {
                continue;
            }
            let gain = assigned
                .iter()
                .enumerate()
                .filter(|(other, value)| *other != factor && value.is_some())
                .filter(|(other, other_value)| {
                    pairs.is_new(pairs.index(
                        factor,
                        value,
                        *other,
                        other_value.unwrap_or_default(),
                    ))
                })
                .count();
            match best {
                Some((best_gain, _)) if gain < best_gain => {}
                Some((best_gain, _)) if gain == best_gain => {
                    ties += 1;
                    if stream.below(ties + 1) == 0 {
                        best = Some((gain, value));
                    }
                }
                _ => {
                    best = Some((gain, value));
                    ties = 0;
                }
            }
        }
        assigned[factor] = Some(best?.1);
    }
    assigned.into_iter().collect()
}

/// A layout drawn at random, every factor among the values the rules admit.
fn random(space: &Space, stream: &mut Stream) -> Result<(Vec<usize>, Layout), String> {
    let mut last = String::new();
    for _ in 0..RANDOM_TRIES {
        let mut assigned = vec![None; space.sizes.len()];
        let mut order: Vec<usize> = (0..space.sizes.len()).collect();
        stream.shuffle(&mut order);
        for factor in order {
            let admitted: Vec<usize> = (0..space.sizes[factor])
                .filter(|value| {
                    assigned[factor] = Some(*value);
                    space.admits(&assigned)
                })
                .collect();
            if admitted.is_empty() {
                assigned[factor] = None;
                break;
            }
            assigned[factor] = Some(admitted[stream.below(admitted.len())]);
        }
        // An order that reaches a factor no value of which stands with those
        // before it is drawn again.
        let Some(assignment) = assigned.into_iter().collect::<Option<Vec<usize>>>() else {
            continue;
        };
        match realize(space, &assignment, stream) {
            Ok(layout) => return Ok((assignment, layout)),
            Err(reason) => last = reason,
        }
    }
    Err(format!(
        "no layout drawn at random in {RANDOM_TRIES} tries was legal; the last: {last}"
    ))
}

/// Places what an assignment holds, and keeps the first placement that
/// compiles and is written as a replay.
fn realize(space: &Space, assignment: &[usize], stream: &mut Stream) -> Result<Layout, String> {
    let round = ROUNDS[assignment[0]];
    let mut last = String::new();
    for _ in 0..PLACEMENT_TRIES {
        let seed = stream.seed();
        // The constructions a side may hold are the ones its seed lays.
        let opening = crate::opening::predict(Economy::embedded()?, seed, DEFAULT_MAP_ID)?;
        let layout = Layout {
            kind: DocumentKind::Layout,
            game_build: crate::economy::this_build(),
            map_id: None,
            seed: Some(seed),
            round,
            blue: side(
                space,
                assignment,
                0,
                round,
                &opening.constructions.blue,
                stream,
            ),
            red: side(
                space,
                assignment,
                1,
                round,
                &opening.constructions.red,
                stream,
            ),
        };
        let checked = compile_layout(layout.clone())
            .and_then(|plan| layout_replay(&plan, &layout.game_build).map(|_| ()));
        match checked {
            Ok(()) => return Ok(layout.normalized()),
            Err(reason) => last = reason,
        }
    }
    Err(last)
}

const MAIN_X: (i64, i64) = (-300, 300);
const MAIN_Y: (i64, i64) = (-310, -10);
const FLANK_Y: (i64, i64) = (10, 310);
const LEFT_FLANK_X: (i64, i64) = (-360, -300);
const RIGHT_FLANK_X: (i64, i64) = (300, 360);
const DEPTHS: [(i64, i64); 3] = [(-60, -10), (-180, -60), (-310, -180)];

/// A centre on the deployment grid for a footprint of `extent` along one
/// axis, its footprint inside `region` and the centre inside `band` when the
/// two meet, or inside `region` alone when they do not.
fn on_grid(stream: &mut Stream, extent: i64, region: (i64, i64), band: (i64, i64)) -> i64 {
    let Some(remainder) = grid_center_remainder(extent) else {
        return 0;
    };
    let fits = (region.0 + extent / 2, region.1 - extent / 2);
    let within = (fits.0.max(band.0), fits.1.min(band.1));
    let draw = |stream: &mut Stream, (low, high): (i64, i64)| {
        let first = low + (remainder - low).rem_euclid(10);
        (first <= high).then(|| first + 10 * stream.between(0, (high - first) / 10))
    };
    draw(stream, within)
        .or_else(|| draw(stream, fits))
        .unwrap_or_default()
}

/// A footprint placed in a region, its centre's `y` inside `band`.
fn place(
    stream: &mut Stream,
    (width, height): (i64, i64),
    x: (i64, i64),
    y: (i64, i64),
    band: (i64, i64),
) -> Position {
    let x = on_grid(stream, width, x, x);
    let y = on_grid(stream, height, y, band);
    Position {
        x: i32::try_from(x).unwrap_or_default(),
        y: i32::try_from(y).unwrap_or_default(),
    }
}

/// A point anywhere in `x` and `y`, on a five-unit step.
fn anywhere(stream: &mut Stream, x: (i64, i64), y: (i64, i64)) -> Position {
    let mut coordinate =
        |(low, high): (i64, i64)| i32::try_from(5 * stream.between(low / 5, high / 5)).unwrap_or(0);
    Position {
        x: coordinate(x),
        y: coordinate(y),
    }
}

fn level(value: usize) -> i32 {
    i32::try_from(value + 1).unwrap_or(1)
}

#[allow(clippy::too_many_lines)]
fn side(
    space: &Space,
    assignment: &[usize],
    side: usize,
    round: i32,
    opening: &[StaticPlacement],
    stream: &mut Stream,
) -> Side {
    let at = |role| assignment[factor(side, role)];
    let economy = Economy::embedded().ok();

    let (lead_id, lead_name, footprint) = space.types[at(Role::LeadType)];
    let lead_level = level(at(Role::LeadLevel));
    let legacy = at(Role::LeadSource) == LEGACY;
    let rotated = at(Role::LeadRotated) == 1;
    let exp = match at(Role::LeadExp) {
        0 => None,
        half_or_full => crate::experience::full(lead_name, lead_level).map(|maximum| Experience {
            current: if half_or_full == 1 {
                maximum / 2
            } else {
                maximum
            },
            maximum,
        }),
    };
    let lead_position = if at(Role::LeadDepth) == FLANK {
        // A flank turns a footprint a quarter, and `rotated` turns it back.
        let world = if rotated {
            footprint
        } else {
            (footprint.1, footprint.0)
        };
        let x = if stream.below(2) == 0 {
            LEFT_FLANK_X
        } else {
            RIGHT_FLANK_X
        };
        place(stream, world, x, FLANK_Y, FLANK_Y)
    } else {
        let world = if rotated {
            (footprint.1, footprint.0)
        } else {
            footprint
        };
        place(stream, world, MAIN_X, MAIN_Y, DEPTHS[at(Role::LeadDepth)])
    };
    let lead = UnitPlacement {
        type_name: lead_name.to_owned(),
        index: 0,
        position: lead_position,
        level: Some(lead_level),
        exp,
        rotated: Some(rotated),
        equipment: match at(Role::LeadEquipment) {
            0 => Vec::new(),
            value => vec![space.equipment[value - 1]],
        },
        travelling: (at(Role::LeadDepth) == FLANK && !legacy).then_some(true),
        source: if legacy {
            UnitSource::Legacy
        } else {
            UnitSource::Joined
        },
    };

    let techs = match (at(Role::LeadTechs), space.technologies.get(&lead_id)) {
        (1, Some(all)) => vec![all[stream.below(all.len())]],
        (2, Some(all)) => all.clone(),
        _ => Vec::new(),
    };

    let mut officers = Vec::new();
    for role in [Role::OfficerFirst, Role::OfficerSecond] {
        if at(role) > 0 {
            officers.push(space.generic[at(role) - 1]);
        }
    }
    if at(Role::LeadModification) > 0
        && let Some(modifications) = space.modifications.get(lead_name)
    {
        officers.push(modifications[at(Role::LeadModification) - 1]);
    }
    let mut delivered = None;
    if at(Role::Opening) > 0 {
        let opening = space.openings[at(Role::Opening) - 1];
        officers.push(opening);
        if let Some(officer) = economy.and_then(|economy| economy.officer(opening))
            && let Some(squad) = officer.opening_unit
            && officer.active_round.contains(&round)
            && let Some((name, footprint)) = unit_type_from_id(squad.unit)
        {
            delivered = Some(UnitPlacement {
                type_name: name.to_owned(),
                index: 0,
                position: place(stream, footprint, MAIN_X, MAIN_Y, MAIN_Y),
                level: Some(squad.level),
                exp: None,
                rotated: None,
                equipment: Vec::new(),
                travelling: None,
                source: UnitSource::Delivered,
            });
        }
    }

    let second = match at(Role::SecondType) {
        0 => None,
        value => {
            let (_, name, footprint) = space.types[value - 1];
            Some(UnitPlacement {
                type_name: name.to_owned(),
                index: 0,
                position: place(stream, footprint, MAIN_X, MAIN_Y, MAIN_Y),
                level: Some(level(at(Role::SecondLevel))),
                exp: None,
                rotated: None,
                equipment: Vec::new(),
                travelling: None,
                source: UnitSource::Joined,
            })
        }
    };

    // The allocator names legacy units, then a delivered squad, then the
    // units that join, with no gap.
    let (first, joined) = if legacy {
        (Some(lead), None)
    } else {
        (None, Some(lead))
    };
    let mut units: Vec<UnitPlacement> = [first, delivered, joined, second]
        .into_iter()
        .flatten()
        .collect();
    for (index, unit) in units.iter_mut().enumerate() {
        unit.index = i32::try_from(index).unwrap_or_default();
    }

    // A part keeps each of the seed's constructions on an even draw, and its
    // index with it: one an earlier round destroyed leaves a gap.
    let constructions = match at(Role::Construction) {
        0 => Vec::new(),
        1 => opening.to_vec(),
        _ => opening
            .iter()
            .filter(|_| stream.below(2) == 0)
            .cloned()
            .collect(),
    };
    let contraptions = match at(Role::Contraption) {
        0 => Vec::new(),
        value => {
            let name = CONTRAPTIONS[value - 1];
            let position = if name == "interceptor" {
                place(stream, (30, 30), MAIN_X, MAIN_Y, MAIN_Y)
            } else {
                anywhere(stream, MAIN_X, MAIN_Y)
            };
            vec![ContraptionPlacement {
                type_name: name.to_owned(),
                index: 0,
                position,
            }]
        }
    };
    let battle_skills = match at(Role::BattleSkill) {
        0 => Vec::new(),
        value => {
            let name = space.skills[value - 1];
            let count = resolve_battle_skill_type(name).map_or(1, |spec| spec.positions);
            let positions = (0..count)
                .map(|_| anywhere(stream, MAIN_X, (MAIN_Y.0, FLANK_Y.1)))
                .collect();
            vec![BattleSkillEntry::Release(BattleSkillRelease {
                type_name: name.to_owned(),
                positions,
            })]
        }
    };

    Side {
        officers,
        techs,
        blueprints: match at(Role::Blueprint) {
            0 => Vec::new(),
            value => vec![CHAIN_BLUEPRINTS[value - 1].0],
        },
        energy_tower_skills: match at(Role::TowerSkills) {
            0 => Vec::new(),
            1 => vec![RANGE_ENHANCEMENT_SKILL],
            2 => vec![MOVEMENT_ENHANCEMENT_SKILL],
            _ => vec![RANGE_ENHANCEMENT_SKILL, MOVEMENT_ENHANCEMENT_SKILL],
        },
        tower_strengthen_levels: match at(Role::TowerLevels) {
            0 => Vec::new(),
            1 => vec![
                i32::try_from(stream.between(1, 4)).unwrap_or(1),
                i32::try_from(stream.between(1, 4)).unwrap_or(1),
            ],
            _ => vec![4, 4],
        },
        units,
        recovered: Vec::new(),
        constructions,
        contraptions,
        battle_skills,
    }
}

#[cfg(test)]
mod tests {
    use super::{Space, generate};
    use crate::{canonical_yaml, compile_layout};

    #[test]
    fn a_batch_is_its_seed_and_a_layout_its_place() {
        let short = generate(7, 3).expect("a batch draws");
        let long = generate(7, 5).expect("a batch draws");
        for (left, right) in short.layouts.iter().zip(&long.layouts) {
            assert_eq!(left.layout, right.layout);
        }
        let other = generate(8, 3).expect("a batch draws");
        assert_ne!(short.layouts[0].layout, other.layouts[0].layout);
    }

    #[test]
    fn every_layout_compiles_and_the_first_covers_the_most() {
        let batch = generate(11, 6).expect("a batch draws");
        for generated in &batch.layouts {
            compile_layout(generated.layout.clone()).expect("a generated layout compiles");
            canonical_yaml(generated.layout.clone()).expect("a generated layout writes");
        }
        assert!(batch.layouts[0].new_pairs >= batch.layouts[5].new_pairs);
        assert!(batch.covered > 0 && batch.covered < batch.pairs);
    }

    #[test]
    fn the_space_reads_the_build() {
        let space = Space::read().expect("the tables read");
        assert!(
            space.generic.contains(&20002),
            "Advanced Offensive Tactics is generic"
        );
        assert!(
            space.openings.contains(&20029),
            "Marksman Specialist delivers a squad"
        );
        assert!(
            !space.generic.contains(&20310),
            "a chain's officer is a blueprint's"
        );
        assert_eq!(space.modifications["marksman"][0], 30201);
    }
}
