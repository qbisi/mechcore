//! What the page plays: every object a recording holds, as tracks over the
//! ticks it lived, and the events that make it move between them.
//!
//! A recording is a snapshot per tick. The page draws between ticks, so it
//! needs each object's whole life at once rather than the world one tick at a
//! time: this turns the snapshots inside out, one track per field of each
//! object. Lengths are centimetres and angles tenths of a degree, integers the
//! page divides back, which is finer than a sprite can show and keeps the
//! differences a track writes short.

use std::collections::{BTreeMap, HashMap};

use mechcore_document::{Layout, Position, Region};
use mechcore_mcfr::{
    BuildingState, Domain, Event, EventPayload, LiveUnitState, MotionState, ObjectKind, ObjectRef,
    ProjectileState, QVec3, Recording, ShieldSourceKind, ShieldState, TerrainGridState,
    TerrainState, TerrainType, UnitPose,
};
use serde::{Deserialize, Serialize, Serializer};

use crate::deployment::{Deployment, deployments};
use crate::track::Track;

pub const SCHEMA: &str = "mechcore.player";

/// Logic ticks per second: the `1/20` logic step MCFR's `DurableContext`
/// fixes for every producer.
const TICKS_PER_SECOND: u32 = 20;

/// Room left around the six deployment regions and the farthest object, 25
/// metres.
const MARGIN: i64 = 2_500;

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<mechcore_mcfr::Error> for Error {
    fn from(error: mechcore_mcfr::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(error: serde_yaml::Error) -> Self {
        Self(error.to_string())
    }
}

/// A recording laid out for the page.
#[derive(Debug, Serialize)]
pub struct Timeline {
    pub schema: &'static str,
    /// Who fought it: `game` or `simulator`.
    pub producer: &'static str,
    pub round: i32,
    /// The last tick; the page plays ticks `1..=ticks`.
    pub ticks: u32,
    pub ticks_per_second: u32,
    pub cores: [Core; 2],
    pub rosters: [Vec<crate::RosterEntry>; 2],
    /// Survivors' score contributions, checked against recorded settlement.
    pub score_sources: Vec<ScoreSource>,
    pub field: Field,
    pub regions: Vec<DeploymentRegion>,
    pub units: Vec<Unit>,
    pub buildings: Vec<Building>,
    pub projectiles: Vec<Projectile>,
    pub shields: Vec<Shield>,
    pub terrains: Vec<Terrain>,
    pub deployments: Vec<Deployment>,
    pub cues: Vec<Cue>,
    /// The names of the clips the units' poses play, which a [`Pose`] track
    /// numbers; empty for a recording that holds no poses.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<String>,
}

/// Player core health, separate from unit life. A standalone recording does
/// not state the starting core; only its settled damage is known.
#[derive(Debug, Serialize)]
pub struct Core {
    pub name: String,
    pub begin: Option<i32>,
    pub maximum: Option<i32>,
    pub damage: i32,
    pub officers: Vec<Officer>,
}

/// Native presentation text for an officer held in this deployment.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Officer {
    pub id: i32,
    pub name: String,
    pub effect: String,
}

#[derive(Debug, Serialize)]
pub struct ScoreSource {
    pub unit: u64,
    pub team: u32,
    pub score: i64,
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

/// One of a side's three deployment regions, in centimetres in the world's
/// frame: its least corner and its greatest.
#[derive(Debug, Serialize)]
pub struct DeploymentRegion {
    pub team: u32,
    pub flank: bool,
    pub x0: i64,
    pub z0: i64,
    pub x1: i64,
    pub z1: i64,
}

/// Each side's three deployment regions, where the layout places its
/// formations: its main half and the two flanks on the other side's half. A
/// side's own frame is the world's for blue and turned half a turn for red.
fn deployment_regions() -> Vec<DeploymentRegion> {
    let mut regions = Vec::new();
    for team in [0, 1] {
        let turn = if team == 0 { 100 } else { -100 };
        for region in Region::ALL {
            let ((x0, y0), (x1, y1)) = region.bounds();
            let (xa, xb) = (x0 * turn, x1 * turn);
            let (za, zb) = (y0 * turn, y1 * turn);
            regions.push(DeploymentRegion {
                team,
                flank: region.is_flank(),
                x0: xa.min(xb),
                z0: za.min(zb),
                x1: xa.max(xb),
                z1: za.max(zb),
            });
        }
    }
    regions
}

/// The rectangle the page frames, centred on the map's centre.
#[derive(Debug, Serialize)]
pub struct Field {
    pub half_width: i64,
    pub half_depth: i64,
}

/// One unit's life. Every track holds one value per tick from `from` to `to`.
#[derive(Debug, Serialize)]
pub struct Unit {
    pub id: u64,
    pub team: u32,
    pub formation: u64,
    pub unit_type: u32,
    /// The layout's name for the type, which picks the sprite.
    pub kind: String,
    pub air: bool,
    pub radius: i64,
    pub max_life: i32,
    pub from: u32,
    pub to: u32,
    pub x: Track,
    pub y: Track,
    pub z: Track,
    /// The chassis' facing.
    pub body: Track,
    /// The turret's facing, for a unit with a body.
    pub turret: Option<Track>,
    pub life: Track,
    /// The personal shield's energy, for a unit that ever holds one.
    pub shield: Option<Track>,
    /// `MotionState`: 0 idle, 1 moving, 2 attacking, 3 stopped, 4 between.
    pub motion: Track,
    /// What the first weapon fires at, as a [`Ref`] number: a unit by its
    /// id, a building by its negated id, nothing by 0.
    pub aim: Track,
    /// How the game drew the unit, for a recording that holds its poses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pose: Option<Pose>,
}

/// A unit's animated pose, tick by tick: the clip its model's base layer
/// plays most and how far through it, from the `unit_pose` channel.
#[derive(Debug, Serialize)]
pub struct Pose {
    /// The clip's index in [`Timeline::clips`], -1 for none.
    pub clip: Track,
    /// The state's normalized time in thousandths: cycles played, the integer
    /// part counting loops.
    pub time: Track,
}

/// One building's life. It never moves; only its life changes.
#[derive(Debug, Serialize)]
pub struct Building {
    pub id: u64,
    pub team: u32,
    pub building_type: u32,
    /// `energy_tower`, `research_center`, or the construction the layout
    /// placed where it stands.
    pub kind: String,
    /// The layout's index of that construction, which a wall's blocks share.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<i32>,
    pub x: i64,
    pub z: i64,
    pub width: i64,
    pub depth: i64,
    pub max_life: i32,
    pub from: u32,
    pub to: u32,
    pub life: Track,
}

#[derive(Debug, Serialize)]
pub struct Projectile {
    pub id: u64,
    pub team: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<Ref>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<Ref>,
    /// What fired it when no unit or building did: `missile` for a Sentry
    /// Missile, the contraption that is its own projectile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub from: u32,
    pub to: u32,
    pub x: Track,
    pub y: Track,
    pub z: Track,
}

#[derive(Debug, Serialize)]
pub struct Shield {
    pub id: u64,
    pub team: u32,
    /// `contraption`, `commander_skill`, `owner_advanced` or
    /// `spawned_temporary`.
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<Ref>,
    pub from: u32,
    pub to: u32,
    pub x: Track,
    pub z: Track,
    pub radius: Track,
    pub energy: Track,
    pub max_energy: Track,
    /// 1 while it intercepts, 0 while it does not.
    pub active: Track,
}

/// A terrain's recorded shapes. Unchanged consecutive ticks share a frame;
/// cuts and conversions switch shapes at their recorded tick, without easing.
#[derive(Debug, Serialize)]
pub struct Terrain {
    pub id: u64,
    pub frames: Vec<TerrainFrame>,
}

#[derive(Debug, Serialize)]
pub struct TerrainFrame {
    pub from: u32,
    pub to: u32,
    #[serde(flatten)]
    pub shape: TerrainShape,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TerrainShape {
    pub kind: TerrainType,
    pub x: i64,
    pub z: i64,
    pub radius: i64,
    pub grid: Option<TerrainGrid>,
}

/// Five-metre cells: the origin is the first cell's centre, row z's bit x
/// marks cell (x, z). MCFR's transposed storage is converted back to world
/// axes before it reaches the page.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TerrainGrid {
    pub x: i64,
    pub z: i64,
    pub width: u32,
    pub height: u32,
    pub rows: Vec<u32>,
}

/// An object named across tracks and cues: `u12` is unit 12, `b3` building 3,
/// `p`, `s` and `t` a projectile, a shield and a terrain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ref(pub ObjectRef);

impl Ref {
    const fn letter(self) -> char {
        match self.0.kind {
            ObjectKind::Unit => 'u',
            ObjectKind::Projectile => 'p',
            ObjectKind::Building => 'b',
            ObjectKind::Shield => 's',
            ObjectKind::Terrain => 't',
        }
    }

    /// The number an `aim` track holds: a unit's id, a building's negated.
    fn number(reference: Option<ObjectRef>) -> i64 {
        let Some(reference) = reference else {
            return 0;
        };
        let id = i64::try_from(reference.id).unwrap_or(0);
        match reference.kind {
            ObjectKind::Unit => id,
            ObjectKind::Building => -id,
            _ => 0,
        }
    }
}

impl Serialize for Ref {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("{}{}", self.letter(), self.0.id))
    }
}

/// Something that happened between two ticks, which the page animates:
/// `t` is the tick it led to.
#[derive(Debug, Serialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum Cue {
    /// A new unit was created; its first snapshot may join the fight later.
    Spawn {
        t: u32,
        u: u64,
    },
    /// A weapon released projectile `p`.
    Fire {
        t: u32,
        p: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        by: Option<Ref>,
        #[serde(skip_serializing_if = "Option::is_none")]
        at: Option<Ref>,
        #[serde(skip_serializing_if = "Option::is_none")]
        weapon: Option<i32>,
        /// The firing skill's slot among its unit's skills.
        #[serde(skip_serializing_if = "Option::is_none")]
        slot: Option<u16>,
        /// The radius the firing skill splashes over, in centimetres, as its
        /// `splash_range` stood on the tick it fired; none when it does not.
        #[serde(skip_serializing_if = "Option::is_none")]
        splash: Option<i64>,
    },
    /// `at` lost `n` life or energy, carried by projectile `p` if one did.
    Hit {
        t: u32,
        at: Ref,
        #[serde(skip_serializing_if = "Option::is_none")]
        by: Option<Ref>,
        n: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        p: Option<u64>,
        /// The slot of the skill that struck, when no projectile carried it.
        #[serde(skip_serializing_if = "Option::is_none")]
        slot: Option<u16>,
        /// The radius the blow splashes over, in centimetres, when no
        /// projectile carried it and its skill splashes.
        #[serde(skip_serializing_if = "Option::is_none")]
        splash: Option<i64>,
    },
    Heal {
        t: u32,
        at: Ref,
        n: i32,
    },
    /// Projectile `p` left the fight where it stood: shot down, taken by
    /// shield `s`, or spent.
    Gone {
        t: u32,
        p: u64,
        x: i64,
        y: i64,
        z: i64,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        intercepted: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        s: Option<u64>,
    },
    Die {
        t: u32,
        u: u64,
        x: i64,
        z: i64,
    },
    Fall {
        t: u32,
        b: u64,
        x: i64,
        z: i64,
    },
    ShieldUp {
        t: u32,
        s: u64,
    },
    ShieldDown {
        t: u32,
        s: u64,
        x: i64,
        z: i64,
    },
}

/// Lays a recording out for the page, with the units' poses when the
/// recording holds its `unit_pose` channel.
///
/// # Errors
///
/// Returns an error when a tick of the recording cannot be read or the
/// embedded deployment-presentation table is invalid.
pub fn timeline(
    recording: &dyn Recording,
    poses: Option<&[(u32, UnitPose)]>,
) -> Result<Timeline, Error> {
    let layout = mechcore_document::parse_embedded_yaml(recording.layout_yaml().as_bytes()).ok();
    let ticks = recording.terminal_tick();
    let mut builder = Builder::new(layout.as_ref());
    for deployment in deployments(layout.as_ref())? {
        builder.reach(
            deployment.x0.abs() + deployment.radius,
            deployment.z0.abs() + deployment.radius,
        );
        builder.reach(
            deployment.x1.abs() + deployment.radius,
            deployment.z1.abs() + deployment.radius,
        );
        builder.deployments.push(deployment);
    }
    if let Some(poses) = poses {
        builder.poses(poses);
    }
    for tick in 1..=ticks {
        let state = recording.state(tick)?;
        for unit in &state.live_units {
            builder.unit(tick, unit);
        }
        for building in &state.buildings {
            builder.building(tick, building);
        }
        for projectile in &state.projectiles {
            builder.projectile(tick, projectile);
        }
        for shield in &state.shields {
            builder.shield(tick, shield);
        }
        for terrain in &state.terrains {
            builder.terrain(tick, terrain);
        }
        for event in &recording.events(tick)?.events {
            builder.cue(tick, event);
        }
    }
    let mut timeline = builder.finish(
        recording.producer().as_str(),
        ticks,
        layout.as_ref().map_or(0, |layout| layout.round),
    );
    if let Some(layout) = &layout {
        timeline.rosters =
            [&layout.blue, &layout.red].map(|side| crate::unit_roster(side, &[], None));
    }
    let officers: BTreeMap<i32, Officer> =
        serde_json::from_str(include_str!("../web/officers.json"))
            .map_err(|error| Error(format!("cannot read officer descriptions: {error}")))?;
    for (team, core) in timeline.cores.iter_mut().enumerate() {
        core.damage = recording.settlement().teams[1 - team].score;
        if let Some(layout) = &layout {
            core.officers = [&layout.blue, &layout.red][team]
                .officers
                .iter()
                .map(|id| {
                    officers.get(id).cloned().unwrap_or_else(|| Officer {
                        id: *id,
                        name: format!("Officer {id}"),
                        effect: "Description unavailable".into(),
                    })
                })
                .collect();
        }
    }
    Ok(timeline)
}

struct Builder<'a> {
    layout: Option<&'a Layout>,
    units: BTreeMap<u64, Unit>,
    buildings: BTreeMap<u64, Building>,
    projectiles: BTreeMap<u64, Projectile>,
    shields: BTreeMap<u64, Shield>,
    terrains: BTreeMap<u64, Terrain>,
    deployments: Vec<Deployment>,
    cues: Vec<Cue>,
    reach: (i64, i64),
    /// Each unit's base-layer pose by tick and unit, when the recording
    /// holds poses: the clip's index and the normalized time in thousandths.
    poses: Option<HashMap<(u32, u64), (i64, i64)>>,
    clips: Vec<String>,
    /// Each unit's skills' splash radius in centimetres, by unit and skill
    /// slot, as it stood on the last tick read.
    splashes: BTreeMap<(u64, u16), i64>,
}

impl<'a> Builder<'a> {
    const fn new(layout: Option<&'a Layout>) -> Self {
        Self {
            layout,
            units: BTreeMap::new(),
            buildings: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            shields: BTreeMap::new(),
            terrains: BTreeMap::new(),
            deployments: Vec::new(),
            cues: Vec::new(),
            reach: (0, 0),
            poses: None,
            clips: Vec::new(),
            splashes: BTreeMap::new(),
        }
    }

    /// Indexes the base layer of every pose: the layer that poses a unit,
    /// which the controllers' second layers only blend into on the move.
    fn poses(&mut self, rows: &[(u32, UnitPose)]) {
        let mut numbered: HashMap<String, i64> = HashMap::new();
        let mut poses = HashMap::new();
        for (tick, pose) in rows {
            if pose.layer != 0 {
                continue;
            }
            let clip = pose
                .clips
                .iter()
                .max_by(|left, right| left.weight.total_cmp(&right.weight))
                .map_or(-1, |clip| {
                    let next = i64::try_from(numbered.len()).unwrap_or(i64::MAX);
                    *numbered.entry(clip.name.clone()).or_insert_with(|| {
                        self.clips.push(clip.name.clone());
                        next
                    })
                });
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a normalized time in thousandths stays far inside i64"
            )]
            let time = (f64::from(pose.normalized_time) * 1_000.0).round() as i64;
            poses.insert((*tick, pose.unit.id), (clip, time));
        }
        self.poses = Some(poses);
    }

    fn reach(&mut self, x: i64, z: i64) {
        self.reach = (self.reach.0.max(x.abs()), self.reach.1.max(z.abs()));
    }

    fn unit(&mut self, tick: u32, state: &LiveUnitState) {
        let position = centimetres3(state.position);
        for skill in &state.skills {
            let splash = skill
                .enabled
                .as_ref()
                .map_or(0, |enabled| centimetres(enabled.splash_range));
            self.splashes
                .insert((state.unit_id, skill.skill_slot), splash);
        }
        self.reach(position.0, position.2);
        let unit = self.units.entry(state.unit_id).or_insert_with(|| Unit {
            id: state.unit_id,
            team: state.team_id,
            formation: state.formation_id,
            unit_type: state.unit_type_id,
            kind: unit_kind(state.unit_type_id),
            air: state.domain == Domain::Air,
            radius: centimetres(state.collision_radius),
            max_life: state.life.maximum,
            from: tick,
            to: tick,
            x: Track::default(),
            y: Track::default(),
            z: Track::default(),
            body: Track::default(),
            turret: state.turret_rotation.map(|_| Track::default()),
            life: Track::default(),
            shield: None,
            motion: Track::default(),
            aim: Track::default(),
            pose: None,
        });
        if self.poses.is_some() && unit.pose.is_none() {
            let mut pose = Pose {
                clip: Track::default(),
                time: Track::default(),
            };
            for _ in 0..unit.life.len() {
                pose.clip.push(-1);
                pose.time.push(0);
            }
            unit.pose = Some(pose);
        }
        let shield = if state.personal_shield.enabled || state.personal_shield.active {
            i64::from(state.personal_shield.energy.current.max(0))
        } else {
            0
        };
        if shield > 0 && unit.shield.is_none() {
            let mut track = Track::default();
            for _ in 0..unit.life.len() {
                track.push(0);
            }
            unit.shield = Some(track);
        }
        // A tick the unit skipped holds what it last was.
        while unit.to + 1 < tick {
            unit.to += 1;
            repeat_unit(unit);
        }
        unit.to = tick;
        unit.team = state.team_id;
        unit.x.push(position.0);
        unit.y.push(position.1);
        unit.z.push(position.2);
        unit.body.push(tenths_of_a_degree(state.body_rotation));
        if let Some(turret) = &mut unit.turret {
            turret.push(
                state
                    .turret_rotation
                    .map_or_else(|| turret.last().unwrap_or_default(), tenths_of_a_degree),
            );
        }
        unit.life.push(i64::from(state.life.current));
        if let Some(track) = &mut unit.shield {
            track.push(shield);
        }
        unit.motion.push(motion(state.motion_state));
        let aim = state
            .skills
            .iter()
            .find_map(|skill| skill.enabled.as_ref()?.attack_target)
            .or(state.mech_lock_target);
        unit.aim.push(Ref::number(aim));
        if let (Some(pose), Some(poses)) = (&mut unit.pose, &self.poses) {
            let (clip, time) = poses
                .get(&(tick, state.unit_id))
                .copied()
                .unwrap_or((-1, 0));
            pose.clip.push(clip);
            pose.time.push(time);
        }
    }

    fn building(&mut self, tick: u32, state: &BuildingState) {
        let (x, _, z) = centimetres3(state.position);
        self.reach(x, z);
        let (kind, group) = building_kind(self.layout, state);
        let building = self
            .buildings
            .entry(state.building_id)
            .or_insert_with(|| Building {
                id: state.building_id,
                team: state.team_id,
                building_type: state.building_type_id,
                kind,
                group,
                x,
                z,
                width: centimetres(state.bounds_width),
                depth: centimetres(state.bounds_height),
                max_life: state.life.maximum,
                from: tick,
                to: tick,
                life: Track::default(),
            });
        while building.to + 1 < tick {
            building.to += 1;
            let last = building.life.last().unwrap_or_default();
            building.life.push(last);
        }
        building.to = tick;
        building.max_life = building.max_life.max(state.life.maximum);
        building.life.push(i64::from(state.life.current));
    }

    fn projectile(&mut self, tick: u32, state: &ProjectileState) {
        let position = centimetres3(state.position);
        let layout = self.layout;
        let projectile = self
            .projectiles
            .entry(state.projectile_id)
            .or_insert_with(|| Projectile {
                id: state.projectile_id,
                team: state.team_id,
                owner: state.owner.map(Ref),
                target: state.target.map(Ref),
                kind: state
                    .owner
                    .is_none()
                    .then(|| launcher(layout, state.team_id, position))
                    .flatten(),
                from: tick,
                to: tick,
                x: Track::default(),
                y: Track::default(),
                z: Track::default(),
            });
        while projectile.to + 1 < tick {
            projectile.to += 1;
            for track in [&mut projectile.x, &mut projectile.y, &mut projectile.z] {
                let last = track.last().unwrap_or_default();
                track.push(last);
            }
        }
        projectile.to = tick;
        projectile.x.push(position.0);
        projectile.y.push(position.1);
        projectile.z.push(position.2);
    }

    fn shield(&mut self, tick: u32, state: &ShieldState) {
        let (x, _, z) = centimetres3(state.position);
        let shield = self
            .shields
            .entry(state.shield_id)
            .or_insert_with(|| Shield {
                id: state.shield_id,
                team: state.team_id,
                source: shield_source(state.source_kind),
                owner: state.owner.map(Ref),
                from: tick,
                to: tick,
                x: Track::default(),
                z: Track::default(),
                radius: Track::default(),
                energy: Track::default(),
                max_energy: Track::default(),
                active: Track::default(),
            });
        while shield.to + 1 < tick {
            shield.to += 1;
            for track in [
                &mut shield.x,
                &mut shield.z,
                &mut shield.radius,
                &mut shield.energy,
                &mut shield.max_energy,
                &mut shield.active,
            ] {
                let last = track.last().unwrap_or_default();
                track.push(last);
            }
        }
        shield.to = tick;
        shield.x.push(x);
        shield.z.push(z);
        shield.radius.push(centimetres(state.radius));
        shield.energy.push(i64::from(state.energy.current));
        shield.max_energy.push(i64::from(state.energy.maximum));
        shield.active.push(i64::from(state.active));
    }

    fn terrain(&mut self, tick: u32, state: &TerrainState) {
        let (x, _, z) = centimetres3(state.position);
        let radius = centimetres(state.radius);
        self.reach(x.abs() + radius, z.abs() + radius);
        let shape = TerrainShape {
            kind: state.terrain_type,
            x,
            z,
            radius,
            grid: state.grid.as_ref().map(terrain_grid),
        };
        let terrain = self
            .terrains
            .entry(state.terrain_id)
            .or_insert_with(|| Terrain {
                id: state.terrain_id,
                frames: Vec::new(),
            });
        if let Some(last) = terrain.frames.last_mut()
            && last.to + 1 == tick
            && last.shape == shape
        {
            last.to = tick;
        } else {
            terrain.frames.push(TerrainFrame {
                from: tick,
                to: tick,
                shape,
            });
        }
    }

    /// The splash radius of a unit's skill, in centimetres, as it stood on the
    /// last tick read; none when the source is no unit, the slot is unknown
    /// or the skill does not splash.
    fn splash(&self, source: Option<ObjectRef>, slot: Option<u16>) -> Option<i64> {
        let source = source.filter(|source| source.kind == ObjectKind::Unit)?;
        let splash = *self.splashes.get(&(source.id, slot?))?;
        (splash > 0).then_some(splash)
    }

    /// The cue a released projectile makes.
    fn fire(&self, t: u32, event: &Event, weapon: Option<i32>, slot: Option<u16>) -> Option<Cue> {
        let projectile = event.subject?;
        Some(Cue::Fire {
            t,
            p: projectile.id,
            by: event.source.map(Ref),
            at: event.target.map(Ref),
            weapon,
            slot,
            splash: self.splash(event.source, slot),
        })
    }

    /// The cue a blow makes: its splash when no projectile carried it.
    fn hit(&self, t: u32, event: &Event, amount: i32, slot: Option<u16>) -> Option<Cue> {
        let target = event.target?;
        let p = event.subject.map(|projectile| projectile.id);
        Some(Cue::Hit {
            t,
            at: Ref(target),
            by: event.source.map(Ref),
            n: amount,
            p,
            slot: p.is_none().then_some(slot).flatten(),
            splash: p
                .is_none()
                .then(|| self.splash(event.source, slot))
                .flatten(),
        })
    }

    fn cue(&mut self, t: u32, event: &Event) {
        let cue = match &event.payload {
            EventPayload::UnitCreated { .. } => {
                let Some(unit) = event.subject else {
                    return;
                };
                Cue::Spawn { t, u: unit.id }
            }
            EventPayload::ProjectileReleased {
                weapon_index,
                skill_slot,
            } => {
                let Some(cue) = self.fire(t, event, *weapon_index, *skill_slot) else {
                    return;
                };
                cue
            }
            EventPayload::Damage { amount, skill_slot } => {
                let Some(cue) = self.hit(t, event, *amount, *skill_slot) else {
                    return;
                };
                cue
            }
            EventPayload::Healing { amount } => {
                let Some(target) = event.target else {
                    return;
                };
                Cue::Heal {
                    t,
                    at: Ref(target),
                    n: *amount,
                }
            }
            EventPayload::ProjectileRemoved {
                position,
                intercepted,
                absorbed_by,
            } => {
                let Some(projectile) = event.subject else {
                    return;
                };
                let (x, y, z) = centimetres3(*position);
                Cue::Gone {
                    t,
                    p: projectile.id,
                    x,
                    y,
                    z,
                    intercepted: *intercepted,
                    s: absorbed_by.map(|shield| shield.id),
                }
            }
            EventPayload::UnitDied { position } => {
                let Some(unit) = event.subject else {
                    return;
                };
                let (x, _, z) = centimetres3(*position);
                Cue::Die {
                    t,
                    u: unit.id,
                    x,
                    z,
                }
            }
            EventPayload::BuildingDestroyed { position } => {
                let Some(building) = event.subject else {
                    return;
                };
                let (x, _, z) = centimetres3(*position);
                Cue::Fall {
                    t,
                    b: building.id,
                    x,
                    z,
                }
            }
            EventPayload::ShieldCreated { .. } => {
                let Some(shield) = event.subject else {
                    return;
                };
                Cue::ShieldUp { t, s: shield.id }
            }
            EventPayload::ShieldDestroyed { position, .. } => {
                let Some(shield) = event.subject else {
                    return;
                };
                let (x, _, z) = centimetres3(*position);
                Cue::ShieldDown {
                    t,
                    s: shield.id,
                    x,
                    z,
                }
            }
            _ => return,
        };
        self.cues.push(cue);
    }

    fn finish(mut self, producer: &'static str, ticks: u32, round: i32) -> Timeline {
        let regions = deployment_regions();
        for region in &regions {
            self.reach(region.x0, region.z0);
            self.reach(region.x1, region.z1);
        }
        Timeline {
            schema: SCHEMA,
            producer,
            round,
            ticks,
            ticks_per_second: TICKS_PER_SECOND,
            cores: ["Blue", "Red"].map(|name| Core {
                name: name.into(),
                begin: None,
                maximum: None,
                damage: 0,
                officers: Vec::new(),
            }),
            score_sources: Vec::new(),
            rosters: [Vec::new(), Vec::new()],
            field: Field {
                half_width: self.reach.0 + MARGIN,
                half_depth: self.reach.1 + MARGIN,
            },
            regions,
            units: self.units.into_values().collect(),
            buildings: self.buildings.into_values().collect(),
            projectiles: self.projectiles.into_values().collect(),
            shields: self.shields.into_values().collect(),
            terrains: self.terrains.into_values().collect(),
            deployments: self.deployments,
            cues: self.cues,
            clips: self.clips,
        }
    }
}

/// Native `GridBlockInt.grids[z]` holds x at bit `31 - x`, as
/// `RangeItemEffectLayerGrid.ShowTestGridBlock` places its cells. MCFR
/// transposes those words into low-bit masks: stored row x holds bit z.
/// Undo that transpose so the page can use ordinary spatial rows.
fn terrain_grid(grid: &TerrainGridState) -> TerrainGrid {
    let mut rows = vec![0_u32; usize::try_from(grid.size_x).unwrap_or_default()];
    for (x, column) in grid.rows.iter().copied().enumerate().take(32) {
        for (z, row) in rows.iter_mut().enumerate().take(32) {
            if column & (1 << z) != 0 {
                *row |= 1 << x;
            }
        }
    }
    TerrainGrid {
        x: centimetres(grid.origin_x),
        z: centimetres(grid.origin_y),
        width: grid.size_y,
        height: grid.size_x,
        rows,
    }
}

fn repeat_unit(unit: &mut Unit) {
    let mut tracks = vec![
        &mut unit.x,
        &mut unit.y,
        &mut unit.z,
        &mut unit.body,
        &mut unit.life,
        &mut unit.motion,
        &mut unit.aim,
    ];
    if let Some(turret) = &mut unit.turret {
        tracks.push(turret);
    }
    if let Some(shield) = &mut unit.shield {
        tracks.push(shield);
    }
    if let Some(pose) = &mut unit.pose {
        tracks.push(&mut pose.clip);
        tracks.push(&mut pose.time);
    }
    for track in tracks {
        let last = track.last().unwrap_or_default();
        track.push(last);
    }
}

/// A Q32.32 length in whole centimetres, rounded to nearest.
pub(crate) fn centimetres(raw: i64) -> i64 {
    scaled(raw, 100)
}

fn centimetres3(position: QVec3) -> (i64, i64, i64) {
    (
        centimetres(position.x),
        centimetres(position.y),
        centimetres(position.z),
    )
}

/// A Q32.32 angle in degrees as tenths of a degree in `0..3600`.
fn tenths_of_a_degree(raw: i64) -> i64 {
    scaled(raw, 10).rem_euclid(3600)
}

pub(crate) fn scaled(raw: i64, scale: i64) -> i64 {
    let value = (i128::from(raw) * i128::from(scale) + (1_i128 << 31)) >> 32;
    i64::try_from(value).unwrap_or(if value < 0 { i64::MIN } else { i64::MAX })
}

const fn motion(state: MotionState) -> i64 {
    match state {
        MotionState::Idle => 0,
        MotionState::Moving => 1,
        MotionState::Attacking => 2,
        MotionState::Stopped => 3,
        MotionState::Transitioning => 4,
    }
}

const fn shield_source(kind: ShieldSourceKind) -> &'static str {
    match kind {
        ShieldSourceKind::Contraption => "contraption",
        ShieldSourceKind::CommanderSkill => "commander_skill",
        ShieldSourceKind::OwnerAdvanced => "owner_advanced",
        ShieldSourceKind::SpawnedTemporary => "spawned_temporary",
    }
}

fn unit_kind(unit_type: u32) -> String {
    i32::try_from(unit_type)
        .ok()
        .and_then(mechcore_document::unit_type_from_id)
        .map_or_else(|| format!("unit_{unit_type}"), |(name, _)| name.to_owned())
}

/// What a building is. A tower says so by its `BuildingType`; a
/// construction's object does not say which construction released it, so it
/// is the construction the layout placed nearest to it on its side, the way
/// the command line's `show --view buildings` reads one.
fn building_kind(layout: Option<&Layout>, state: &BuildingState) -> (String, Option<i32>) {
    match state.building_type_id {
        1 => return ("energy_tower".to_owned(), None),
        2 => return ("research_center".to_owned(), None),
        3 => {}
        other => return (format!("building_{other}"), None),
    }
    // A Missile Interceptor is a building of the same type, placed among the
    // side's contraptions.
    let (x, _, z) = centimetres3(state.position);
    placements(layout, state.team_id)
        .min_by_key(|&(_, _, px, pz)| (px - x).pow(2) + (pz - z).pow(2))
        .map_or_else(
            || ("construction".to_owned(), None),
            |(name, index, _, _)| (name.to_owned(), Some(index)),
        )
}

/// Each construction and contraption a side's layout places: its name, its
/// index and where it stands in the world, in centimetres.
fn placements(layout: Option<&Layout>, team: u32) -> impl Iterator<Item = (&str, i32, i64, i64)> {
    let side = match (layout, team) {
        (Some(layout), 0) => Some(&layout.blue),
        (Some(layout), 1) => Some(&layout.red),
        _ => None,
    };
    // Red's board is blue's turned around.
    let turn = if team == 0 { 1 } else { -1 };
    let world = move |position: &Position| {
        (
            i64::from(position.x) * 100 * turn,
            i64::from(position.y) * 100 * turn,
        )
    };
    side.into_iter().flat_map(move |side| {
        let constructions = side.constructions.iter().map(move |placement| {
            let (x, z) = world(&placement.position);
            (placement.type_name.as_str(), placement.index, x, z)
        });
        let contraptions = side.contraptions.iter().map(move |placement| {
            let (x, z) = world(&placement.position);
            (placement.type_name.as_str(), placement.index, x, z)
        });
        constructions.chain(contraptions)
    })
}

/// What fired a projectile no unit or building owns: a Sentry Missile is its
/// own projectile, released where the layout placed it, so one that starts
/// within 10 m of its side's `missile` placement is that missile.
fn launcher(layout: Option<&Layout>, team: u32, (x, _, z): (i64, i64, i64)) -> Option<String> {
    placements(layout, team)
        .filter(|&(name, _, _, _)| name == "missile")
        .any(|(_, _, px, pz)| (px - x).pow(2) + (pz - z).pow(2) <= 1000 * 1000)
        .then(|| "missile".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{centimetres, tenths_of_a_degree, terrain_grid};
    use mechcore_mcfr::TerrainGridState;

    const METRE: i64 = 1 << 32;

    #[test]
    fn terrain_masks_return_to_world_axes() {
        let grid = terrain_grid(&TerrainGridState {
            origin_x: -10 * METRE,
            origin_y: 20 * METRE,
            size_x: 2,
            size_y: 3,
            rows: vec![1, 2, 1],
        });
        assert_eq!((grid.x, grid.z), (-1000, 2000));
        assert_eq!((grid.width, grid.height), (3, 2));
        assert_eq!(grid.rows, [0b101, 0b010]);
    }

    #[test]
    fn right_hand_oil_is_cut_toward_the_shield() {
        // A 30 m oil at (60, -40), beside a 70 m shield at (0, 0).
        // The shield is above and left: those cells must be absent.
        let grid = terrain_grid(&TerrainGridState {
            origin_x: 65 * METRE / 2,
            origin_y: -135 * METRE / 2,
            size_x: 12,
            size_y: 12,
            rows: vec![0, 0, 6, 14, 31, 63, 255, 1023, 2046, 2046, 1020, 240],
        });
        assert_eq!(grid.rows[10] & (1 << 4), 0); // (52.5, -17.5), inside shield.
        assert_ne!(grid.rows[4] & (1 << 10), 0); // (82.5, -47.5), outside shield.
    }

    #[test]
    fn lengths_round_to_the_nearest_centimetre() {
        assert_eq!(centimetres(140 * METRE), 14_000);
        assert_eq!(centimetres(-170 * METRE), -17_000);
        assert_eq!(centimetres(METRE / 3), 33);
        assert_eq!(centimetres(-METRE / 3), -33);
    }

    #[test]
    fn angles_wrap_into_one_turn() {
        assert_eq!(tenths_of_a_degree(90 * METRE), 900);
        assert_eq!(tenths_of_a_degree(-90 * METRE), 2700);
        assert_eq!(tenths_of_a_degree(360 * METRE), 0);
    }
}
