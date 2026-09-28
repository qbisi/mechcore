//! What a fight decided, read out of a recording of it.
//!
//! `docs/spec/document/match.md` names four fields as the fight's: the damage
//! each reactor core takes, the experience each unit gains, and which
//! contraptions and which objects earlier releases left standing remain, with
//! what this round's releases leave. `docs/spec/document/fight.md` writes them
//! onto the layout the fight started from, and this reads a recording for
//! each of them. What the recording does not answer is named rather than
//! approximated, and a fight document is written only when nothing is.
//!
//! The reader is the seam the backends meet at. A fight the simulator ran and
//! a fight the game played are the same MCFR, so what turns one into a fight
//! document, and a match's next position, is written once, here.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use mechcore_document::{
    BattleSkillEntry, Fight, FightBattleSkill, FightContraption, FightExperience, FightHash,
    FightKind, FightRelease, FightSide, FightStanding, FightUnit, Layout, Position, Source,
    Standing, UnitPlacement,
    reactor_damage::{self, Survivor as Scored},
};
use mechcore_mcfr::{
    EventPayload, McfrReader, ObjectKind, Producer, ShieldRoundPolicy, ShieldSourceKind,
    ShieldState, TerrainType, WorldSnapshot,
};
use serde::Serialize;

use crate::{
    cli::Failure,
    scene::{self, FIRST_TICK},
    turn::Side,
};

pub(crate) const SCHEMA: &str = "mechcore.fight-outcome.v4";

/// `FPoint`'s one, the recording's fixed point.
const ONE: i64 = 1 << 32;

/// How far from a missile a projectile it launched is first recorded, in
/// world units. A launched missile has moved one tick when the recording first
/// holds it, which is a few units; two missiles of one side stand further
/// apart than this, since a contraption's footprint keeps them apart.
const LAUNCH_REACH: i64 = 10;

/// How far a Sticky Oil Bomb's point may stand from the line its control
/// points span, raw. The build steps along the line with a fixed-point square
/// root and clamp, so its five inner points differ slightly from linear
/// interpolation; this bound only refuses a point that belongs to no such
/// line.
const OIL_POINT_REACH: i64 = 1 << 16;

/// The seven points a Sticky Oil Bomb's line expands into.
const OIL_POINTS: i64 = 7;

/// A recording read for what its fight decided.
pub(crate) struct Reading {
    layout: Layout,
    source: Source,
    ticks: u32,
    hash: FightHash,
    sides: [SideReading; 2],
    /// What this fight decided that the recording does not answer. A fight
    /// with an entry here converts to no fight document and settles no round.
    pub(crate) unresolved: Vec<String>,
}

/// One side's share of the reading, each result beside the layout entry it
/// belongs to; `None` is a result `unresolved` names.
struct SideReading {
    core_damage: Option<i64>,
    survivors: Vec<Survivor>,
    /// By the side's `units`, in the layout's order.
    exp: Vec<Option<FightExperience>>,
    /// By the side's `contraptions`: whether each still stands.
    contraptions: Vec<Option<bool>>,
    /// By the side's `battle_skills`: the entry with its result.
    battle_skills: Vec<Option<FightBattleSkill>>,
}

/// One formation that survived, and how much of it did.
#[derive(Serialize)]
struct Survivor {
    index: i32,
    name: String,
    /// How many members the formation went into the fight with, and how many
    /// came out of it.
    members: usize,
    alive: usize,
    /// The life those members have left, and what they would hold whole.
    life: i64,
    maximum: i64,
}

/// `show --view outcome`: what a fight document leaves out, and what keeps a
/// recording from becoming one.
#[derive(Serialize)]
pub(crate) struct Outcome {
    schema: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    recording: Option<String>,
    /// The last tick the recording holds, which is where the fight ended.
    ticks: u32,
    sides: Sides,
    pub(crate) unresolved: Vec<String>,
}

#[derive(Serialize)]
struct Sides {
    blue: SideOutcome,
    red: SideOutcome,
}

/// The formations still standing when the fight ended, by the index the
/// document knows them under. A fight document does not carry them: a unit
/// comes back whole next round, so its life hands nothing on.
#[derive(Serialize)]
struct SideOutcome {
    survivors: Vec<Survivor>,
}

/// Opens a recording on disk.
///
/// # Errors
///
/// Returns a refusal when the file is not a recording this build reads.
pub(crate) fn open(path: &Path) -> Result<McfrReader, Failure> {
    McfrReader::open(path)
        .map_err(|error| Failure::refused(format!("cannot read {}: {error}", path.display())))
}

/// The fight document a recording on disk converts to.
///
/// # Errors
///
/// Returns a refusal when the file is not a recording this build reads, and
/// one naming everything the recording does not answer.
pub(crate) fn fight(path: &Path) -> Result<Fight, Failure> {
    read(&open(path)?)?.fight()
}

/// Reads an open recording for what its fight decided.
///
/// # Errors
///
/// Returns a failure when the recording's scene cannot be matched to the
/// layout it embeds.
pub(crate) fn read(reader: &McfrReader) -> Result<Reading, Failure> {
    let layout = scene::layout(reader)?;
    let last = reader.terminal_tick();
    let opened = scene::snapshot(reader, FIRST_TICK)?;
    let ended = scene::snapshot(reader, last)?;
    let formations = scene::formations(&layout, &opened)?;
    let timeline = Timeline::read(reader)?;
    let started = members(&formations, &opened);
    let survived = members(&formations, &ended);

    let mut unresolved = Vec::new();
    let scores = scores(
        &layout,
        &formations,
        &timeline,
        &opened,
        &ended,
        &mut unresolved,
    )?;
    let damage = match scores {
        [Some(blue), Some(red)] => {
            let (blue, red) = reactor_damage::damage(blue, red);
            [Some(blue), Some(red)]
        }
        _ => [None, None],
    };
    let scene = Scene {
        formations: &formations,
        opened: &opened,
        ended: &ended,
        timeline: &timeline,
    };
    let sides = Side::BOTH.map(|side| {
        let mut unresolved_here = Vec::new();
        let reading = SideReading {
            core_damage: damage[side.seat()],
            survivors: survivors(side, &started, &survived, scene::units_of(&layout, side)),
            exp: experience(&layout, side, &scene, &mut unresolved_here),
            contraptions: contraptions(&layout, side, &scene, &mut unresolved_here),
            battle_skills: battle_skills(&layout, side, &scene, &mut unresolved_here),
        };
        (reading, unresolved_here)
    });
    let [(blue, blue_unresolved), (red, red_unresolved)] = sides;
    unresolved.extend(blue_unresolved);
    unresolved.extend(red_unresolved);
    Ok(Reading {
        layout,
        source: match reader.producer() {
            Producer::Game => Source::Recording,
            Producer::Simulator => Source::Simulator,
        },
        ticks: last,
        hash: FightHash {
            profile: mechcore_mcfr::HASH_PROFILE.to_owned(),
            result: reader.hashes().result_hash.clone(),
        },
        sides: [blue, red],
        unresolved,
    })
}

impl Reading {
    /// What `show --view outcome` answers.
    pub(crate) fn outcome(self, recording: Option<String>) -> Outcome {
        let [blue, red] = self.sides;
        Outcome {
            schema: SCHEMA,
            recording,
            ticks: self.ticks,
            sides: Sides {
                blue: SideOutcome {
                    survivors: blue.survivors,
                },
                red: SideOutcome {
                    survivors: red.survivors,
                },
            },
            unresolved: self.unresolved,
        }
    }

    /// The fight document: the layout the recording embeds, with the result
    /// written in.
    ///
    /// # Errors
    ///
    /// Refuses a reading with anything unresolved, naming all of it, because
    /// a fight document states nothing it did not read.
    pub(crate) fn fight(self) -> Result<Fight, Failure> {
        if !self.unresolved.is_empty() {
            return Err(Failure::refused(format!(
                "the recording does not answer everything its fight decided: {}",
                self.unresolved.join("; ")
            )));
        }
        let Layout {
            game_build,
            map_id,
            seed,
            round,
            blue,
            red,
            ..
        } = self.layout;
        let seed = seed.ok_or_else(|| Failure::refused("the recording embeds no seed"))?;
        let [blue_reading, red_reading] = self.sides;
        let fight = Fight {
            kind: FightKind::Fight,
            game_build,
            map_id,
            seed,
            round,
            source: self.source,
            ticks: Some(self.ticks),
            hash: Some(self.hash),
            blue: fight_side(blue, blue_reading)?,
            red: fight_side(red, red_reading)?,
        };
        mechcore_document::fight::validate(&fight).map_err(|error| {
            Failure::failed(format!("the fight read is not a valid fight: {error}"))
        })?;
        Ok(fight.normalized())
    }
}

/// One side of the fight document, from the layout's side and what was read.
fn fight_side(side: mechcore_document::Side, read: SideReading) -> Result<FightSide, Failure> {
    let answered = || Failure::failed("a result was left unanswered with nothing unresolved");
    let core_damage = read.core_damage.ok_or_else(answered)?;
    Ok(FightSide {
        core_damage: i32::try_from(core_damage)
            .map_err(|_| Failure::failed(format!("core damage {core_damage} is out of range")))?,
        officers: side.officers,
        techs: side.techs,
        blueprints: side.blueprints,
        energy_tower_skills: side.energy_tower_skills,
        tower_strengthen_levels: side.tower_strengthen_levels,
        units: side
            .units
            .into_iter()
            .zip(read.exp)
            .map(|(unit, exp)| {
                Ok(FightUnit {
                    type_name: unit.type_name,
                    index: unit.index,
                    position: unit.position,
                    level: unit.level,
                    exp: Some(exp.ok_or_else(answered)?),
                    rotated: unit.rotated,
                    equipment: unit.equipment,
                    travelling: unit.travelling,
                })
            })
            .collect::<Result<_, Failure>>()?,
        constructions: side.constructions,
        contraptions: side
            .contraptions
            .into_iter()
            .zip(read.contraptions)
            .map(|(contraption, retained)| {
                Ok(FightContraption {
                    type_name: contraption.type_name,
                    index: contraption.index,
                    position: contraption.position,
                    retained: retained.ok_or_else(answered)?,
                })
            })
            .collect::<Result<_, Failure>>()?,
        battle_skills: read
            .battle_skills
            .into_iter()
            .map(|entry| entry.ok_or_else(answered))
            .collect::<Result<_, Failure>>()?,
    })
}

/// What the whole fight shows, beside its first and last snapshots.
struct Timeline {
    /// Units the fight created, and units that died in it.
    created: BTreeSet<u64>,
    died: BTreeSet<u64>,
    /// Each projectile released with no owner: its team and where it is
    /// first recorded, in world units.
    launches: Vec<(u32, (i64, i64))>,
    /// Every shield the fight held at any tick, as it was when first seen.
    shields: BTreeMap<u64, ShieldState>,
}

impl Timeline {
    fn read(reader: &McfrReader) -> Result<Self, Failure> {
        let mut timeline = Self {
            created: BTreeSet::new(),
            died: BTreeSet::new(),
            launches: Vec::new(),
            shields: BTreeMap::new(),
        };
        for shield in scene::snapshot(reader, FIRST_TICK)?.shields {
            timeline.shields.insert(shield.shield_id, shield);
        }
        for tick in FIRST_TICK..=reader.terminal_tick() {
            let events = reader.events(tick).map_err(|error| {
                Failure::refused(format!("recording has no events at tick {tick}: {error}"))
            })?;
            let mut ownerless = BTreeSet::new();
            let mut shielded = false;
            for event in events.events {
                let Some(subject) = event.subject else {
                    continue;
                };
                match (subject.kind, &event.payload) {
                    (ObjectKind::Unit, EventPayload::UnitCreated { .. }) => {
                        timeline.created.insert(subject.id);
                    }
                    (ObjectKind::Unit, EventPayload::UnitDied { .. }) => {
                        timeline.died.insert(subject.id);
                    }
                    (ObjectKind::Projectile, EventPayload::ProjectileReleased { .. })
                        if event.source.is_none() =>
                    {
                        ownerless.insert(subject.id);
                    }
                    (ObjectKind::Shield, EventPayload::ShieldCreated { .. }) => shielded = true,
                    _ => {}
                }
            }
            if ownerless.is_empty() && !shielded {
                continue;
            }
            let state = scene::snapshot(reader, tick)?;
            for projectile in state
                .projectiles
                .iter()
                .filter(|projectile| ownerless.contains(&projectile.projectile_id))
                .filter(|projectile| projectile.owner.is_none())
            {
                timeline.launches.push((
                    projectile.team_id,
                    (projectile.position.x >> 32, projectile.position.z >> 32),
                ));
            }
            for shield in state.shields {
                timeline.shields.entry(shield.shield_id).or_insert(shield);
            }
        }
        Ok(timeline)
    }
}

/// What every reading of one side is taken against.
struct Scene<'a> {
    formations: &'a BTreeMap<u64, (Side, i32)>,
    opened: &'a WorldSnapshot,
    ended: &'a WorldSnapshot,
    timeline: &'a Timeline,
}

/// The team a side is recorded under.
const fn team(side: Side) -> u32 {
    match side {
        Side::Blue => 0,
        Side::Red => 1,
    }
}

/// Where a layout position stands in the world, in the recording's raw fixed
/// point.
fn raw(position: Position, side: Side) -> (i64, i64) {
    let (x, z) = scene::world(position, side);
    (x * ONE, z * ONE)
}

/// Each side's score, `TeamScoreCalculator.CalculateTeamScore`: what the
/// units alive at the fight's end score for the side they then serve.
///
/// A unit is classified by what the recording shows of it. It came from its
/// side's formations when it opened the fight in a formation a placement
/// takes and the fight did not create it; any other unit was summoned,
/// produced or spawned. It changed sides when its team is not the one it
/// started on. It was reborn when it died in the fight and stands at the end:
/// a rebirth revives the unit that died, `RebirthTask.RebirthMech`, which is
/// the one way a dead unit stands again.
fn scores(
    layout: &Layout,
    formations: &BTreeMap<u64, (Side, i32)>,
    timeline: &Timeline,
    opened: &WorldSnapshot,
    ended: &WorldSnapshot,
    unresolved: &mut Vec<String>,
) -> Result<[Option<i64>; 2], Failure> {
    let opened_in: BTreeMap<u64, u64> = opened
        .live_units
        .iter()
        .map(|unit| (unit.unit_id, unit.formation_id))
        .collect();
    let mut scores = [Some(0_i64), Some(0_i64)];
    for unit in ended
        .live_units
        .iter()
        .filter(|unit| unit.life.current > 0 && unit.active)
    {
        let side = match unit.team_id {
            0 => Side::Blue,
            1 => Side::Red,
            other => {
                return Err(Failure::refused(format!(
                    "recording holds team {other}, and a match has two"
                )));
            }
        };
        let name = mechcore_document::unit_type_from_id(
            i32::try_from(unit.unit_type_id).unwrap_or(i32::MAX),
        )
        .map_or_else(
            || unit.unit_type_id.to_string(),
            |(name, _)| name.to_owned(),
        );
        // A unit that changes sides joins a formation of the side it serves, so
        // where it came from is the formation it opened the fight in.
        let placement = opened_in
            .get(&unit.unit_id)
            .and_then(|formation| formations.get(formation))
            .and_then(|(owner, index)| {
                scene::units_of(layout, *owner)
                    .iter()
                    .find(|placement| placement.index == *index)
            });
        let survivor = Scored {
            unit_type: unit.unit_type_id,
            level: placement.map(|placement| placement.level.unwrap_or(1)),
            support: timeline.created.contains(&unit.unit_id) || placement.is_none(),
            reborn: timeline.died.contains(&unit.unit_id),
            team_changed: unit.team_id != unit.original_team_id,
        };
        match reactor_damage::score(survivor) {
            Ok(score) => {
                if let Some(total) = &mut scores[side.seat()] {
                    *total += score;
                }
            }
            Err(reason) => {
                unresolved.push(format!(
                    "{} core_damage: unit {} of {name} on {}: {reason}",
                    side.other().name(),
                    unit.unit_id,
                    side.name()
                ));
                scores[side.seat()] = None;
            }
        }
    }
    Ok(scores)
}

/// Each unit's experience across the fight: the layout's before, the
/// formation's `experience` at the last tick after, and its bar.
///
/// A formation's experience is `MechTeam.expFloat`, which holds -1.0 until the
/// formation first gains any. The fight's end cuts it to a whole number,
/// `MechTeam.PruneExp`, which the last tick already holds when a side was
/// destroyed and does not when the fight ran out of time, so the last tick's
/// experience is cut here as the fight's end cuts it. The formation opens the
/// fight holding what the layout says, which is how the pairing of formation
/// to placement is checked, and its bar is the table's for the placement's
/// type and level.
fn experience(
    layout: &Layout,
    side: Side,
    scene: &Scene,
    unresolved: &mut Vec<String>,
) -> Vec<Option<FightExperience>> {
    let recorded = |snapshot: &WorldSnapshot, formation: u64| {
        snapshot
            .formations
            .iter()
            .find(|row| row.formation_id == formation)
            .copied()
    };
    let whole = |raw: i64| (raw % ONE == 0).then_some(raw / ONE);
    let gained = |raw: i64| if raw == -ONE { Some(0) } else { whole(raw) };
    let cut = |raw: i64| match raw {
        _ if raw == -ONE => Some(0),
        0.. => Some(raw / ONE),
        _ => None,
    };
    scene::units_of(layout, side)
        .iter()
        .map(|placement| {
            let at = format!(
                "{} units index {} ({}) exp",
                side.name(),
                placement.index,
                placement.type_name
            );
            let answer =
                unit_experience(placement, side, scene, &recorded, [&gained, &cut, &whole]);
            answer
                .map_err(|reason| unresolved.push(format!("{at}: {reason}")))
                .ok()
        })
        .collect()
}

fn unit_experience(
    placement: &UnitPlacement,
    side: Side,
    scene: &Scene,
    recorded: &dyn Fn(&WorldSnapshot, u64) -> Option<mechcore_mcfr::FormationState>,
    [gained, cut, whole]: [&dyn Fn(i64) -> Option<i64>; 3],
) -> Result<FightExperience, String> {
    let formation = scene
        .formations
        .iter()
        .find(|(_, owner)| **owner == (side, placement.index))
        .map(|(formation, _)| *formation)
        .ok_or("no recorded formation is this placement's")?;
    let (Some(opened), Some(ended)) = (
        recorded(scene.opened, formation),
        recorded(scene.ended, formation),
    ) else {
        return Err(format!(
            "formation {formation} has no experience row at the first or the last tick"
        ));
    };
    let level = placement.level.unwrap_or(1);
    let bar = mechcore_document::experience::full(&placement.type_name, level)
        .ok_or_else(|| format!("the table holds no bar for level {level}"))?;
    let before = placement.exp.map_or(0, |exp| exp.current);
    let to_i32 = |value: i64| i32::try_from(value).ok();
    let opened_with = gained(opened.experience).and_then(to_i32);
    if opened_with != Some(before) {
        return Err(format!(
            "formation {formation} opens the fight holding {} raw, and the layout says {before}",
            opened.experience
        ));
    }
    let maximum = whole(ended.max_experience).and_then(to_i32);
    if maximum != Some(bar) {
        return Err(format!(
            "formation {formation}'s bar is {} raw, and level {level}'s is {bar}",
            ended.max_experience
        ));
    }
    let after = cut(ended.experience).and_then(to_i32).ok_or_else(|| {
        format!(
            "formation {formation} ends holding {} raw, which is no experience",
            ended.experience
        )
    })?;
    if after > bar {
        return Err(format!(
            "formation {formation} ends holding {after}, past its bar {bar}, which the build \
             does not allow"
        ));
    }
    if after < before {
        return Err(format!(
            "formation {formation} ends holding {after}, less than the {before} it opened with"
        ));
    }
    Ok(FightExperience {
        before,
        after,
        maximum: bar,
    })
}

/// Whether each of the side's contraptions stands when the fight ends.
///
/// Each kind is found where the build keeps it. A shield contraption is a
/// shield of the contraption source and an interceptor a building, each
/// standing exactly where the layout put it when the fight opens, and it
/// stands at the end when the recording still holds it. A missile is no
/// object a recording holds: it fires once, `MineSystem.ActiveMine`, as a
/// projectile nobody owns, from where it stands, and is spent. So a missile
/// is gone when such a projectile of its side is first recorded beside it.
fn contraptions(
    layout: &Layout,
    side: Side,
    scene: &Scene,
    unresolved: &mut Vec<String>,
) -> Vec<Option<bool>> {
    let placed = &scene::side_of(layout, side).contraptions;
    let missiles: Vec<(i32, (i64, i64))> = placed
        .iter()
        .filter(|contraption| contraption.type_name == "missile")
        .map(|contraption| (contraption.index, scene::world(contraption.position, side)))
        .collect();
    let fired = fired(side, &missiles, scene.timeline);
    placed
        .iter()
        .map(|contraption| {
            let at = format!(
                "{} contraptions index {} ({}) retained",
                side.name(),
                contraption.index,
                contraption.type_name
            );
            let position = raw(contraption.position, side);
            let answer = match contraption.type_name.as_str() {
                "shield" => {
                    standing_shield(scene, side, ShieldSourceKind::Contraption, position, &[])
                }
                "interceptor" => interceptor(scene, side, position),
                "missile" => fired
                    .as_ref()
                    .map(|fired| !fired.contains(&contraption.index))
                    .map_err(Clone::clone),
                other => Err(format!(
                    "a {other} contraption is not one this reader finds"
                )),
            };
            answer
                .map_err(|reason| unresolved.push(format!("{at}: {reason}")))
                .ok()
        })
        .collect()
}

/// Which of a side's missiles fired: each launch of the side's is paired
/// with the missile it is first recorded beside, and a launch beside none
/// is some other ownerless projectile. A missile fires once, so two launches
/// beside one, or one launch beside two, is refused rather than guessed.
fn fired(
    side: Side,
    missiles: &[(i32, (i64, i64))],
    timeline: &Timeline,
) -> Result<BTreeSet<i32>, String> {
    let mut fired = BTreeSet::new();
    for (_, launched) in timeline
        .launches
        .iter()
        .filter(|(launcher, _)| *launcher == team(side))
    {
        let beside: Vec<i32> = missiles
            .iter()
            .filter(|(_, at)| scene::distance(*at, *launched) <= LAUNCH_REACH * LAUNCH_REACH)
            .map(|(index, _)| *index)
            .collect();
        match beside[..] {
            [] => {}
            [index] => {
                if !fired.insert(index) {
                    return Err(format!(
                        "missile {index} is beside two launches, and a missile fires once"
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "a launch at {launched:?} stands beside missiles {beside:?}"
                ));
            }
        }
    }
    Ok(fired)
}

/// Whether the shield standing at `position` when the fight opens still
/// stands when it ends.
///
/// A shield stands until it is destroyed and resets between rounds, so one
/// the recording still holds at its last tick carries on, spent or not; one
/// the build destroys as the round ends does not. `taken` are shields another
/// entry already answers for.
fn standing_shield(
    scene: &Scene,
    side: Side,
    source: ShieldSourceKind,
    position: (i64, i64),
    taken: &[u64],
) -> Result<bool, String> {
    let found: Vec<&ShieldState> = scene
        .opened
        .shields
        .iter()
        .filter(|shield| {
            shield.team_id == team(side)
                && shield.source_kind == source
                && (shield.position.x, shield.position.z) == position
                && !taken.contains(&shield.shield_id)
        })
        .collect();
    let [shield] = found[..] else {
        return Err(format!(
            "the fight opens with {} shields of its kind where it stands",
            found.len()
        ));
    };
    Ok(stands(scene.ended, shield.shield_id))
}

/// Whether a shield stands at the end and carries into the next round.
fn stands(ended: &WorldSnapshot, shield: u64) -> bool {
    ended.shields.iter().any(|standing| {
        standing.shield_id == shield
            && standing.round_policy != ShieldRoundPolicy::DestroyAtRoundEnd
    })
}

/// Whether the interceptor at `position` stands when the fight ends: the one
/// building of the side standing there when it opens, `FightInterceptor`'s
/// `FightCrystal`.
fn interceptor(scene: &Scene, side: Side, position: (i64, i64)) -> Result<bool, String> {
    let found: Vec<u64> = scene
        .opened
        .buildings
        .iter()
        .filter(|building| {
            building.team_id == team(side) && (building.position.x, building.position.z) == position
        })
        .map(|building| building.building_id)
        .collect();
    let [building] = found[..] else {
        return Err(format!(
            "the fight opens with {} buildings of the side where it stands",
            found.len()
        ));
    };
    Ok(scene
        .ended
        .buildings
        .iter()
        .any(|standing| standing.building_id == building && standing.life.current > 0))
}

/// Each `battle_skills` entry with what the fight left of it.
///
/// A standing shield is read as a shield contraption is, from the shields of
/// the commander-skill source. A standing oil area is in its last round and
/// carries nothing. A Shield Airdrop released this round is retained when a
/// commander-skill shield of the side stands where it was released at the
/// end; a Sticky Oil Bomb released this round leaves the points of its line
/// the fight created and still holds as oil that outlives the round. Every
/// other release carries nothing.
fn battle_skills(
    layout: &Layout,
    side: Side,
    scene: &Scene,
    unresolved: &mut Vec<String>,
) -> Vec<Option<FightBattleSkill>> {
    let entries = &scene::side_of(layout, side).battle_skills;
    // A shield a standing entry answers for is not also a release's.
    let mut standing_ids = Vec::new();
    for entry in entries {
        if let BattleSkillEntry::Standing(Standing::Shield { position }) = entry {
            let position = raw(*position, side);
            standing_ids.extend(
                scene
                    .opened
                    .shields
                    .iter()
                    .filter(|shield| {
                        shield.team_id == team(side)
                            && shield.source_kind == ShieldSourceKind::CommanderSkill
                            && (shield.position.x, shield.position.z) == position
                    })
                    .map(|shield| shield.shield_id),
            );
        }
    }
    // Where this round's Shield Airdrops dropped, and how many at each place.
    let place = |positions: &[Position]| -> Vec<(i32, i32)> {
        positions
            .iter()
            .map(|position| (position.x, position.y))
            .collect()
    };
    let mut dropped_at: BTreeMap<Vec<(i32, i32)>, usize> = BTreeMap::new();
    for entry in entries {
        if let BattleSkillEntry::Release(release) = entry
            && release.type_name == "shield_airdrop"
        {
            *dropped_at.entry(place(&release.positions)).or_default() += 1;
        }
    }
    entries
        .iter()
        .enumerate()
        .map(|(at, entry)| {
            let answer = match entry {
                BattleSkillEntry::Standing(standing) => match standing {
                    Standing::Shield { position } => standing_shield(
                        scene,
                        side,
                        ShieldSourceKind::CommanderSkill,
                        raw(*position, side),
                        &[],
                    )
                    .map(|retained| {
                        FightBattleSkill::Standing(FightStanding {
                            standing: standing.clone(),
                            retained,
                        })
                    }),
                    Standing::Oil(_) => Ok(FightBattleSkill::Standing(FightStanding {
                        standing: standing.clone(),
                        retained: true,
                    })),
                },
                BattleSkillEntry::Release(release) => {
                    let result = match release.type_name.as_str() {
                        "shield_airdrop" if dropped_at[&place(&release.positions)] > 1 => Err(
                            "another Shield Airdrop is released at the same place, and one shield \
                             there cannot answer for both"
                                .to_owned(),
                        ),
                        "shield_airdrop" => airdrop(scene, side, &release.positions, &standing_ids)
                            .map(|retained| (retained, BTreeMap::new())),
                        "sticky_oil_bomb" => oil(scene, side, &release.positions),
                        _ => Ok((true, BTreeMap::new())),
                    };
                    result.map(|(retained, grid_rows)| {
                        FightBattleSkill::Release(FightRelease {
                            release: release.clone(),
                            retained,
                            grid_rows,
                        })
                    })
                }
            };
            answer
                .map_err(|reason| {
                    unresolved.push(format!(
                        "{} battle_skills[{at}] ({}): {reason}",
                        side.name(),
                        entry_name(entry)
                    ));
                })
                .ok()
        })
        .collect()
}

fn entry_name(entry: &BattleSkillEntry) -> &str {
    match entry {
        BattleSkillEntry::Release(release) => &release.type_name,
        BattleSkillEntry::Standing(Standing::Shield { .. }) => "shield_airdrop",
        BattleSkillEntry::Standing(Standing::Oil(_)) => "sticky_oil_bomb",
    }
}

/// Whether the shield a Shield Airdrop released this round stands at the
/// fight's end: a commander-skill shield of the side, at the release's one
/// position, that no standing entry answers for. The fight may have made
/// one and destroyed it, or never made one; either way none stands.
fn airdrop(
    scene: &Scene,
    side: Side,
    positions: &[Position],
    standing: &[u64],
) -> Result<bool, String> {
    let [position] = positions else {
        return Err(format!(
            "a release names {} positions, and a Shield Airdrop drops one shield",
            positions.len()
        ));
    };
    let position = raw(*position, side);
    let made: Vec<u64> = scene
        .timeline
        .shields
        .values()
        .filter(|shield| {
            shield.team_id == team(side)
                && shield.source_kind == ShieldSourceKind::CommanderSkill
                && (shield.position.x, shield.position.z) == position
                && !standing.contains(&shield.shield_id)
        })
        .map(|shield| shield.shield_id)
        .collect();
    if made.len() > 1 {
        return Err(format!(
            "the fight made {} shields where it dropped one",
            made.len()
        ));
    }
    Ok(made.iter().any(|shield| stands(scene.ended, *shield)))
}

/// What a Sticky Oil Bomb released this round leaves: whether any of its
/// points does, and `grid_rows` for those that do, in the side's own frame.
///
/// Its seven points lie on the line between its two positions, and each the
/// fight created is an oil terrain of the side at one of them. One that still
/// stands at the end with a round left to run carries into the next round,
/// whole when it holds no grid, and as its grid's rows when it does.
fn oil(
    scene: &Scene,
    side: Side,
    positions: &[Position],
) -> Result<(bool, BTreeMap<u32, Vec<u32>>), String> {
    let [start, end] = positions else {
        return Err(format!(
            "a release names {} positions, and a Sticky Oil Bomb's line two",
            positions.len()
        ));
    };
    let (start, end) = (raw(*start, side), raw(*end, side));
    let points: Vec<(i64, i64)> = (0..OIL_POINTS)
        .map(|index| {
            let along = |from: i64, to: i64| {
                let step = i128::from(to - from) * i128::from(index) / i128::from(OIL_POINTS - 1);
                from + i64::try_from(step).unwrap_or(0)
            };
            (along(start.0, end.0), along(start.1, end.1))
        })
        .collect();
    let there_before: BTreeSet<u64> = scene
        .opened
        .terrains
        .iter()
        .map(|terrain| terrain.terrain_id)
        .collect();
    let mut grid_rows = BTreeMap::new();
    for terrain in scene.ended.terrains.iter().filter(|terrain| {
        terrain.terrain_type == TerrainType::Oil
            && terrain.team_id == Some(team(side))
            && !there_before.contains(&terrain.terrain_id)
    }) {
        let beside: Vec<usize> = points
            .iter()
            .enumerate()
            .filter(|(_, point)| {
                (terrain.position.x - point.0).abs() <= OIL_POINT_REACH
                    && (terrain.position.z - point.1).abs() <= OIL_POINT_REACH
            })
            .map(|(index, _)| index)
            .collect();
        let point = match beside[..] {
            [] => continue,
            [point] => u32::try_from(point).unwrap_or(u32::MAX),
            _ => {
                return Err(format!(
                    "an oil terrain stands on points {beside:?} at once"
                ));
            }
        };
        if terrain.remaining_rounds.unwrap_or(0) == 0 {
            continue;
        }
        let rows = match &terrain.grid {
            None => Vec::new(),
            Some(grid) if grid.size_x == 12 && grid.size_y == 12 && grid.rows.len() == 12 => {
                match side {
                    Side::Blue => grid.rows.clone(),
                    Side::Red => mechcore_document::rotate_oil_grid_rows(&grid.rows),
                }
            }
            Some(grid) => {
                return Err(format!(
                    "point {point}'s grid is {}x{}, and an oil point's is 12x12",
                    grid.size_x, grid.size_y
                ));
            }
        };
        if grid_rows.insert(point, rows).is_some() {
            return Err(format!("point {point} is left twice"));
        }
    }
    Ok((!grid_rows.is_empty(), grid_rows))
}

/// How many members of each formation one snapshot holds, and their life.
fn members(
    formations: &BTreeMap<u64, (Side, i32)>,
    snapshot: &WorldSnapshot,
) -> BTreeMap<(Side, i32), (usize, i64, i64)> {
    let mut counted: BTreeMap<(Side, i32), (usize, i64, i64)> = BTreeMap::new();
    for unit in &snapshot.live_units {
        let Some(formation) = formations.get(&unit.formation_id) else {
            continue;
        };
        let entry = counted.entry(*formation).or_default();
        entry.0 += 1;
        entry.1 += i64::from(unit.life.current);
        entry.2 += i64::from(unit.life.maximum);
    }
    counted
}

/// One side's formations that still stand, in document index order.
fn survivors(
    side: Side,
    started: &BTreeMap<(Side, i32), (usize, i64, i64)>,
    survived: &BTreeMap<(Side, i32), (usize, i64, i64)>,
    placed: &[UnitPlacement],
) -> Vec<Survivor> {
    placed
        .iter()
        .filter_map(|placement| {
            let (alive, life, maximum) = *survived.get(&(side, placement.index))?;
            Some(Survivor {
                index: placement.index,
                name: placement.type_name.clone(),
                members: started
                    .get(&(side, placement.index))
                    .map_or(alive, |(members, _, _)| *members),
                alive,
                life,
                maximum,
            })
        })
        .collect()
}
