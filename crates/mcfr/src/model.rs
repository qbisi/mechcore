use std::{cmp::Ordering, collections::BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Error, Result, canonical};

/// The format: how a recording is stored, which the hash does not read.
macro_rules! format_version {
    () => {
        "0.23.0"
    };
}

/// The hash definition, numbered on its own: it moves when what the hash
/// reads, or how, changes, and with it every pin; a change to the format that
/// leaves the hashed content alone leaves it. Its domain strings spell profile
/// `n` as `0.n.0`, the form it was written in while it was the format's.
macro_rules! hash_profile {
    () => {
        "23"
    };
}
pub(crate) use hash_profile;

pub const MCFR_FORMAT: &str = format_version!();
/// The hash definition's number, which a fight document writes before its
/// hash (`23:<hex>`).
pub const HASH_PROFILE: &str = hash_profile!();
/// How a recording written before the profile was a number names profile 23.
pub(crate) const HASH_PROFILE_SPELLED_BY_FORMAT: &str = "mcfr-content-0.23.0";

/// What wrote a recording: the game, through the Adapter, or the simulator.
///
/// Two recordings of one fight are the same fight whoever wrote them, so the
/// producer is provenance beside the timeline and outside the hash. It is what
/// says whether a recording is evidence of what the game does or a statement
/// of what the simulator computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Producer {
    Game,
    Simulator,
}

impl Producer {
    /// The name the file metadata writes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::Simulator => "simulator",
        }
    }

    pub(crate) fn parse(name: &str) -> Result<Self> {
        match name {
            "game" => Ok(Self::Game),
            "simulator" => Ok(Self::Simulator),
            other => Err(Error::invalid(format!(
                "ticks.parquet metadata producer {other:?} is neither game nor simulator"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Hashes {
    pub result_hash: String,
}

impl Hashes {
    pub(crate) fn from_raw(result: [u8; canonical::HASH_BYTES]) -> Self {
        Self {
            result_hash: canonical::hex(&result),
        }
    }

    pub(crate) fn validate_encoding(&self) -> Result<()> {
        canonical::parse_hex(&self.result_hash, "result_hash")?;
        Ok(())
    }
}

/// One end-of-logical-tick comparison unit.
///
/// `events` are the native events observed while advancing to this tick's
/// `state`. MCFR begins at tick one; `S(0)` and `E(0)` are not stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TickSlice {
    pub events: TransitionEvents,
    pub state: WorldSnapshot,
    pub tick: u32,
    pub tick_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TickHashes {
    pub tick_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DurableContext {
    pub combat_round: u32,
    pub logic_step: Rational,
    pub match_seed: i32,
    pub time_units_per_second: u32,
}

impl DurableContext {
    /// Validates the durable context against the schema implemented by this crate.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid timing and numeric ratios.
    pub fn validate(&self) -> Result<()> {
        if self.combat_round == 0 {
            return Err(Error::invalid("combat_round must be positive"));
        }
        self.logic_step.validate("logic_step")?;
        if self.time_units_per_second == 0 {
            return Err(Error::invalid("time_units_per_second must be positive"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Rational {
    pub denominator: u32,
    pub numerator: u32,
}

impl Rational {
    fn validate(self, label: &str) -> Result<()> {
        if self.numerator == 0 || self.denominator == 0 {
            return Err(Error::invalid(format!(
                "{label} numerator and denominator must be positive"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WorldSnapshot {
    #[serde(default)]
    pub buildings: Vec<BuildingState>,
    /// Every formation's experience, which kills add to inside the tick.
    #[serde(default)]
    pub formations: Vec<FormationState>,
    #[serde(default)]
    pub live_units: Vec<LiveUnitState>,
    #[serde(default)]
    pub projectiles: Vec<ProjectileState>,
    /// Every unit dead and waiting to be reborn.
    #[serde(default)]
    pub rebirths: Vec<RebirthState>,
    #[serde(default)]
    pub shields: Vec<ShieldState>,
    /// The build's own damage and kill counters for the fight so far.
    #[serde(default)]
    pub statistics: Vec<DamageStatistics>,
    #[serde(default)]
    pub terrains: Vec<TerrainState>,
}

/// A formation's experience, `MechTeam`'s own: what `ExpSystem` hands it for
/// kills during the fight, and the bar it stops at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FormationState {
    /// `MechTeam.expFloat`, `FPoint` raw. A formation that has never gained
    /// any holds -1.0, the build's reset value.
    pub experience: i64,
    pub formation_id: u64,
    /// `MechTeam.maxExpFloat`, `FPoint` raw: the full bar, where gains stop.
    pub max_experience: i64,
    pub team_id: u32,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// A unit dead and waiting to be reborn: a `RebirthTask` of
/// `DeadRebirthController.rebirthTasks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RebirthState {
    /// `RebirthTask.GetPosAndRotation`: where the unit will stand again. A
    /// unit reborn where it fell answers where it fell; one that follows an
    /// ally answers where its pilot is.
    pub position: QVec3,
    pub unit_id: u64,
}

/// Whose counters a row of the build's damage statistics is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RecorderKind {
    /// A formation, `MechTeam`: every unit of it counts together.
    Formation,
    /// A construction group, `FightConstructionCombination`, or a
    /// construction outside one.
    Construction,
    /// A unit with no formation, which counts alone: a summon, or a
    /// mind-controlled unit while it serves the other side.
    Unit,
}

/// One entry of `BattleStatisticManager`'s current round, as the build keeps
/// it inside the logic tick: what a formation or construction group dealt,
/// killed and took in this fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DamageStatistics {
    /// `DamageMax`: the sum of each hit's damage after every mitigation, before
    /// it is held to what the target had left.
    pub damage: i32,
    /// `DamageReal`: the life, or personal shield energy, the hits took.
    pub damage_real: i32,
    /// `DamageTaken`: what the recorder's own members were hit for, raised
    /// by what increases damage taken and before anything reduces it.
    pub damage_taken: i32,
    /// `KillCount`: hits after which their target was no longer alive.
    pub kills: i32,
    pub recorder: RecorderKind,
    /// The formation's `formation_id`; for a construction group, the lowest
    /// `building_id` among its constructions; for a unit, its `unit_id`.
    pub recorder_id: u64,
    pub team_id: u32,
}

impl DamageStatistics {
    /// The row's place in its tick: team, recorder kind, recorder.
    #[must_use]
    pub fn key(&self) -> (u32, RecorderKind, u64) {
        (self.team_id, self.recorder, self.recorder_id)
    }
}

impl WorldSnapshot {
    pub fn canonicalize(&mut self) {
        self.live_units.sort_by_key(|value| value.unit_id);
        self.projectiles.sort_by_key(|value| value.projectile_id);
        self.buildings.sort_by_key(|value| value.building_id);
        self.shields.sort_by_key(|value| value.shield_id);
        self.terrains.sort_by_key(|value| value.terrain_id);
        self.statistics.sort_by_key(DamageStatistics::key);
        self.formations
            .sort_by_key(|formation| formation.formation_id);
    }

    /// Returns all top-level object identities in the snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when an identity is zero or duplicated within its object kind.
    #[allow(clippy::too_many_lines)]
    pub fn object_keys(&self) -> Result<BTreeSet<ObjectRef>> {
        let mut keys = BTreeSet::new();
        for key in self.iter_object_keys() {
            if key.id == 0 {
                return Err(Error::invalid(format!(
                    "{:?} identity must be positive",
                    key.kind
                )));
            }
            if !keys.insert(key) {
                return Err(Error::invalid(format!(
                    "duplicate {:?} identity {}",
                    key.kind, key.id
                )));
            }
        }
        for unit in &self.live_units {
            if unit
                .skills
                .windows(2)
                .any(|pair| pair[0].skill_slot >= pair[1].skill_slot)
            {
                return Err(Error::invalid(format!(
                    "unit {} skills are not strictly ordered by slot",
                    unit.unit_id
                )));
            }
            for skill in &unit.skills {
                if skill.enabled.as_ref().is_some_and(|enabled| {
                    enabled
                        .weapons
                        .windows(2)
                        .any(|pair| pair[0].weapon_index >= pair[1].weapon_index)
                }) {
                    return Err(Error::invalid(format!(
                        "unit {} skill {} weapons are not strictly ordered",
                        unit.unit_id, skill.skill_slot
                    )));
                }
            }
        }
        for projectile in &self.projectiles {
            let mut previous = None;
            for shield in &projectile.spawn_containing_shields {
                if shield.kind != ObjectKind::Shield {
                    return Err(Error::invalid(format!(
                        "projectile {} spawn_containing_shields contains non-shield reference",
                        projectile.projectile_id
                    )));
                }
                if previous.is_some_and(|value| *shield <= value) {
                    return Err(Error::invalid(format!(
                        "projectile {} spawn_containing_shields is not strictly ordered",
                        projectile.projectile_id
                    )));
                }
                previous = Some(*shield);
            }
        }
        let mut active_orders = BTreeSet::new();
        for shield in &self.shields {
            if shield.active != shield.active_order.is_some() {
                return Err(Error::invalid(format!(
                    "shield {} active and active_order disagree",
                    shield.shield_id
                )));
            }
            if let Some(order) = shield.active_order
                && !active_orders.insert((shield.team_id, order))
            {
                return Err(Error::invalid(format!(
                    "team {} has duplicate shield active_order {}",
                    shield.team_id, order
                )));
            }
            if shield.radius <= 0 {
                return Err(Error::invalid(format!(
                    "shield {} radius must be positive",
                    shield.shield_id
                )));
            }
        }
        for team_id in self
            .shields
            .iter()
            .map(|shield| shield.team_id)
            .collect::<BTreeSet<_>>()
        {
            let mut orders = self
                .shields
                .iter()
                .filter(|shield| shield.team_id == team_id)
                .filter_map(|shield| shield.active_order)
                .collect::<Vec<_>>();
            orders.sort_unstable();
            for (expected, actual) in (0_u32..).zip(orders.iter().copied()) {
                if expected != actual {
                    return Err(Error::invalid(format!(
                        "team {team_id} shield active_order must be contiguous from zero"
                    )));
                }
            }
        }
        let live_unit_ids = self
            .live_units
            .iter()
            .map(|unit| unit.unit_id)
            .collect::<BTreeSet<_>>();
        for terrain in &self.terrains {
            if terrain.radius <= 0 {
                return Err(Error::invalid(format!(
                    "terrain {} radius must be positive",
                    terrain.terrain_id
                )));
            }
            if let Some(grid) = &terrain.grid {
                let expected_rows = usize::try_from(grid.size_y)
                    .map_err(|_| Error::invalid("terrain grid height exceeds usize"))?;
                if grid.rows.len() != expected_rows {
                    return Err(Error::invalid(format!(
                        "terrain {} grid row count does not match size_y",
                        terrain.terrain_id
                    )));
                }
                if grid.size_x == 0 || grid.size_x > 32 || grid.size_y == 0 || grid.size_y > 32 {
                    return Err(Error::invalid(format!(
                        "terrain {} grid size must be within 1..=32",
                        terrain.terrain_id
                    )));
                }
                let valid_mask = if grid.size_x == 32 {
                    u32::MAX
                } else {
                    (1_u32 << grid.size_x) - 1
                };
                if grid.rows.iter().any(|row| row & !valid_mask != 0) {
                    return Err(Error::invalid(format!(
                        "terrain {} grid row uses bits outside size_x",
                        terrain.terrain_id
                    )));
                }
            }
            let mut previous_unit = None;
            for application in &terrain.applications {
                if !live_unit_ids.contains(&application.unit_id) {
                    return Err(Error::invalid(format!(
                        "terrain {} application references non-live unit {}",
                        terrain.terrain_id, application.unit_id
                    )));
                }
                if previous_unit.is_some_and(|previous| application.unit_id <= previous) {
                    return Err(Error::invalid(format!(
                        "terrain {} applications are not strictly ordered by unit_id",
                        terrain.terrain_id
                    )));
                }
                if let Some(clock) = application.periodic_clock
                    && (clock.elapsed < 0 || clock.duration <= 0)
                {
                    return Err(Error::invalid(format!(
                        "terrain {} application clock must have non-negative elapsed and positive duration",
                        terrain.terrain_id
                    )));
                }
                previous_unit = Some(application.unit_id);
            }
            if let Some(lifetime) = terrain.logic_lifetime
                && (lifetime.elapsed < 0 || lifetime.limit <= 0)
            {
                return Err(Error::invalid(format!(
                    "terrain {} logic lifetime must have non-negative elapsed and positive limit",
                    terrain.terrain_id
                )));
            }
        }
        if self
            .formations
            .windows(2)
            .any(|pair| pair[0].formation_id >= pair[1].formation_id)
        {
            return Err(Error::invalid(
                "formations are not strictly ordered by formation_id",
            ));
        }
        if self
            .statistics
            .windows(2)
            .any(|pair| pair[0].key() >= pair[1].key())
        {
            return Err(Error::invalid(
                "statistics are not strictly ordered by team, recorder kind and recorder",
            ));
        }
        for row in &self.statistics {
            if row.recorder_id == 0
                || row.recorder_id >= 1 << 48
                || row.team_id >= 1 << 8
                || row.damage < 0
                || row.damage_real < 0
                || row.kills < 0
                || row.damage_taken < 0
            {
                return Err(Error::invalid(format!(
                    "statistics row {:?} {} of team {} is out of range",
                    row.recorder, row.recorder_id, row.team_id
                )));
            }
        }
        Ok(keys)
    }

    fn iter_object_keys(&self) -> impl Iterator<Item = ObjectRef> + '_ {
        self.live_units
            .iter()
            .map(|value| ObjectRef::new(ObjectKind::Unit, value.unit_id))
            .chain(
                self.projectiles
                    .iter()
                    .map(|value| ObjectRef::new(ObjectKind::Projectile, value.projectile_id)),
            )
            .chain(
                self.buildings
                    .iter()
                    .map(|value| ObjectRef::new(ObjectKind::Building, value.building_id)),
            )
            .chain(
                self.shields
                    .iter()
                    .map(|value| ObjectRef::new(ObjectKind::Shield, value.shield_id)),
            )
            .chain(
                self.terrains
                    .iter()
                    .map(|value| ObjectRef::new(ObjectKind::Terrain, value.terrain_id)),
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Unit,
    Projectile,
    Building,
    Shield,
    Terrain,
}

impl ObjectKind {
    const COUNT: usize = 5;
    const ALL: [Self; Self::COUNT] = [
        Self::Unit,
        Self::Projectile,
        Self::Building,
        Self::Shield,
        Self::Terrain,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Unit => 0,
            Self::Projectile => 1,
            Self::Building => 2,
            Self::Shield => 3,
            Self::Terrain => 4,
        }
    }
}

/// Its fields are declared in key order, as every hashed object's are, and it
/// orders by kind and then id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ObjectRef {
    pub id: u64,
    pub kind: ObjectKind,
}

impl Ord for ObjectRef {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.kind, self.id).cmp(&(other.kind, other.id))
    }
}

impl PartialOrd for ObjectRef {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl ObjectRef {
    #[must_use]
    pub const fn new(kind: ObjectKind, id: u64) -> Self {
        Self { id, kind }
    }
}

/// Allocates source-neutral identities for MCFR objects and formations.
///
/// Object identities use independent namespaces per [`ObjectKind`]. Formations use a separate
/// namespace. Every namespace starts at one and advances without gaps.
#[derive(Debug, Clone)]
pub struct IdentityAllocator {
    next_formation_id: u64,
    next_object_ids: [u64; ObjectKind::COUNT],
}

impl Default for IdentityAllocator {
    fn default() -> Self {
        Self {
            next_object_ids: [1; ObjectKind::COUNT],
            next_formation_id: 1,
        }
    }
}

impl IdentityAllocator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates the next identity in an object-kind namespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the namespace is exhausted.
    pub fn allocate_object(&mut self, kind: ObjectKind) -> Result<ObjectRef> {
        let next = &mut self.next_object_ids[kind.index()];
        let id = *next;
        *next = next
            .checked_add(1)
            .ok_or_else(|| Error::invalid(format!("{kind:?} identity overflow")))?;
        Ok(ObjectRef::new(kind, id))
    }

    /// Allocates the next formation identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the formation namespace is exhausted.
    pub fn allocate_formation(&mut self) -> Result<u64> {
        let id = self.next_formation_id;
        self.next_formation_id = self
            .next_formation_id
            .checked_add(1)
            .ok_or_else(|| Error::invalid("formation identity overflow"))?;
        Ok(id)
    }

    pub(crate) fn from_initial(snapshot: &WorldSnapshot) -> Result<Self> {
        let keys = snapshot.object_keys()?;
        let mut allocator = Self::new();
        for kind in ObjectKind::ALL {
            for key in keys.iter().filter(|key| key.kind == kind) {
                let expected = allocator.allocate_object(kind)?;
                if *key != expected {
                    return Err(Error::invalid(format!(
                        "initial {kind:?} identities must be contiguous from one; expected {}, found {}",
                        expected.id, key.id
                    )));
                }
            }
        }
        validate_initial_unit_order(snapshot)?;
        validate_initial_formation_order(snapshot)?;
        allocator.observe_formations(snapshot)?;
        Ok(allocator)
    }

    pub(crate) fn observe_formations(&mut self, snapshot: &WorldSnapshot) -> Result<()> {
        let formation_ids = snapshot
            .live_units
            .iter()
            .map(|unit| unit.formation_id)
            .collect::<BTreeSet<_>>();
        for formation_id in formation_ids {
            if formation_id == 0 {
                return Err(Error::invalid("formation identity must be positive"));
            }
            if formation_id >= self.next_formation_id {
                let expected = self.allocate_formation()?;
                if formation_id != expected {
                    return Err(Error::invalid(format!(
                        "formation identities must be contiguous; expected {expected}, found {formation_id}"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct QVec3 {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

/// A vector on the ground plane, raw Q32.32: world `x` and `z`. Movement has
/// no vertical part in this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct QPlanar {
    pub x: i64,
    pub z: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct QPose {
    pub position: QVec3,
    pub rotation: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Ground,
    Air,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MotionState {
    Idle,
    Moving,
    Attacking,
    Stopped,
    /// Between two states while a move ability runs: `MotionFSM` holds its
    /// `TransitionState` until the ability hands it the next one.
    Transitioning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Normal,
    Disappear,
    Stealth,
    Hide,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LiveUnitState {
    pub active: bool,
    pub body_rotation: i64,
    /// Every buff in the unit's `BuffManager.buffs`, in the build's order.
    #[serde(default)]
    pub buffs: Vec<BuffState>,
    pub collision_radius: i64,
    /// The unit's entry in `TeamTranslationSystem.translatingDatas` while a
    /// control beam is turning it, and null while none is.
    #[serde(default)]
    pub control: Option<ControlState>,
    pub domain: Domain,
    pub formation_id: u64,
    pub life: GaugeI32,
    pub mech_lock_target: Option<ObjectRef>,
    pub motion_state: MotionState,
    /// `FightMech.GetMoveSpeed()`, Q32.32 raw: the speed the fight moves the
    /// unit at, after every correction on it.
    pub move_speed: i64,
    pub original_team_id: u32,
    pub personal_shield: PersonalShieldState,
    pub position: QVec3,
    /// `FightMech.rebirthCount`: how many times the unit has been reborn in
    /// this fight. Left out while it is 0, as it is for nearly every unit.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rebirth_count: u32,
    /// Every skill `FightMech.GetSkills()` holds, strictly ascending by slot.
    #[serde(default)]
    pub skills: Vec<SkillState>,
    pub targetable: bool,
    pub team_id: u32,
    /// The rotation of the unit's turret, `FightMech.mechBody`'s
    /// `FightTransform`: what a unit with a body turns toward its attack
    /// target and measures its attack angle from, while `body_rotation`
    /// keeps the chassis. Null for a unit without a body.
    pub turret_rotation: Option<i64>,
    pub unit_id: u64,
    pub unit_type_id: u32,
    pub velocity: QPlanar,
    pub visibility: Visibility,
}

/// A unit a control beam is turning: a `TranslationData`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ControlState {
    /// `TranslationData.progress`: the power its beams' hits have added. The
    /// unit changes side once it reaches the unit's life.
    pub progress: i32,
    /// The owners of the skills in `TranslationData.sources`, in the order
    /// the build keeps them: the unit goes to the first one's side.
    pub sources: Vec<ObjectRef>,
}

/// One skill of a unit: a `FightSkill` its `GetSkills()` holds.
///
/// A grouped unit's skills are the group's slots, and a unit with an extra
/// weapon holds the extra skill's slots after its main skill's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SkillState {
    /// What the skill holds while `FightSkill.IsEnable()`, and null while a
    /// buff has switched it off: a switched-off skill keeps its slot and
    /// nothing else of it is read.
    pub enabled: Option<EnabledSkill>,
    /// The skill's index in `GetSkills()`.
    pub skill_slot: u16,
}

/// What an enabled skill holds at the sampling boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EnabledSkill {
    /// `FightSkill.GetAttackCount()`: the blows started since the skill
    /// entered its attack state, less one.
    pub attack_count: i32,
    /// `FightSkill.GetNormalDamage(0)`.
    pub attack_damage: i32,
    /// The `SkillAttackController` phase, null while no blow is under way.
    pub attack_phase: Option<AttackPhase>,
    /// `FightSkill.GetAttackRange()`, Q32.32 raw.
    pub attack_range: i64,
    /// `FightSkill.GetAttackTarget()`: what the skill's weapons fire at.
    pub attack_target: Option<ObjectRef>,
    /// `FightSkill.attackTime`, logic ticks.
    pub attack_time: i32,
    /// `FightSkill.GetCurrentAttackInterval()`: the interval this cycle was
    /// scheduled with, stagger included, in logic ticks.
    pub current_attack_interval: i32,
    /// `FightSkill.lockTarget`.
    pub lock_target: Option<ObjectRef>,
    /// `SkillAttackController.performCount`: the blows whose cycle has run
    /// out, backswing and all, since the skill entered its attack state.
    pub perform_count: i32,
    /// `FightSkill.GetSplashRange()`, Q32.32 raw: 0 for a skill that does not
    /// splash.
    pub splash_range: i64,
    /// The `SkillStateController` state.
    pub state: SkillMachineState,
    /// The skill's weapons, strictly ascending by index.
    #[serde(default)]
    pub weapons: Vec<WeaponState>,
}

/// A `SkillStateController` state, by its class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SkillMachineState {
    /// `SkillIdleState`.
    Idle,
    /// `SkillPrepareState`.
    Prepare,
    /// `SkillAttackState`.
    Attack,
    /// `SkillCoolingState`.
    Cooling,
    /// `SkillReloadingState`.
    Reloading,
    /// `SkillLockState`.
    Lock,
}

/// A `SkillAttackController` phase, by its controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AttackPhase {
    /// `attackWaitBeforeController`: the wait for the attack point.
    Before,
    /// `attackingController`: the blow being released.
    Attacking,
    /// `attackWaitAfterController`: the backswing.
    After,
}

/// One weapon of a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WeaponState {
    /// The weapon's `FightTransform`, null for a weapon without one.
    #[serde(default)]
    pub pose: Option<QPose>,
    /// `WeaponData.get_Index()`.
    pub weapon_index: i32,
}

/// Compares initial units in format 0.3.0 identity order.
///
/// Teams are visited first; members within one team use ascending world `z`, then ascending
/// world `x`. Equal positions within one team are not ordered by a synthetic tie-breaker.
#[must_use]
pub fn compare_initial_unit_order(
    left_team: u32,
    left_position: &QVec3,
    right_team: u32,
    right_position: &QVec3,
) -> Ordering {
    left_team
        .cmp(&right_team)
        .then_with(|| left_position.z.cmp(&right_position.z))
        .then_with(|| left_position.x.cmp(&right_position.x))
}

fn validate_initial_unit_order(snapshot: &WorldSnapshot) -> Result<()> {
    for pair in snapshot.live_units.windows(2) {
        let ordering = compare_initial_unit_order(
            pair[0].team_id,
            &pair[0].position,
            pair[1].team_id,
            &pair[1].position,
        );
        if ordering != Ordering::Less {
            let [left, right] = [&pair[0], &pair[1]].map(|unit| {
                format!(
                    "unit {} (type {}, formation {}, team {}) at world x {} z {}",
                    unit.unit_id,
                    unit.unit_type_id,
                    unit.formation_id,
                    unit.team_id,
                    unit.position.x,
                    unit.position.z
                )
            });
            return Err(Error::invalid(format!(
                "initial unit identities must follow ascending team, world z, then world x: \
                 {left} comes before {right}"
            )));
        }
    }
    Ok(())
}

fn validate_initial_formation_order(snapshot: &WorldSnapshot) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut expected = 1_u64;
    for unit in &snapshot.live_units {
        if unit.formation_id == 0 {
            return Err(Error::invalid("formation identity must be positive"));
        }
        if seen.insert(unit.formation_id) {
            if unit.formation_id != expected {
                return Err(Error::invalid(format!(
                    "initial formation identities must follow first appearance in unit identity order; expected {expected}, found {}",
                    unit.formation_id
                )));
            }
            expected = expected
                .checked_add(1)
                .ok_or_else(|| Error::invalid("formation identity overflow"))?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PersonalShieldState {
    pub active: bool,
    pub enabled: bool,
    pub energy: GaugeI32,
}

/// What a buff's numbers come from: `Buff.data`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BuffDataKind {
    /// A `BuffData` row, by its `id`.
    Buff,
    /// A technology that serves as its own buff data, `BurrowTech`, by its id.
    Technology,
}

/// `Buff.data`: the row, or the technology, whose numbers the buff writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BuffDataRef {
    pub id: u32,
    pub kind: BuffDataKind,
}

/// One `Buff` a unit holds: what it carries from tick to tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BuffState {
    pub data: BuffDataRef,
    /// `Buff.maxDurationtime`: ticks it runs for in all, lengthened by each
    /// reset.
    pub duration: i32,
    /// `Buff.durationTime`: ticks run since it was written or last reset.
    pub elapsed: i32,
    /// `Buff.source`: the unit or building it counts as from, null for one
    /// no object wrote.
    #[serde(default)]
    pub source: Option<ObjectRef>,
    /// `Buff.sourceTeamController`: the side it is from.
    pub source_team: u32,
    /// `IBEC_AdditiveEffectBuff.additiveStack`, 0 for a buff that does not
    /// stack.
    pub stacks: i32,
    /// `Buff.stepTime`: its periodic clock.
    pub step: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProjectileState {
    pub cached_target_position: QVec3,
    pub cached_target_radius: i64,
    pub life: GaugeI32,
    /// `FightProjectile.moveRange`, Q32.32 raw: the reach it was given as it
    /// was made, which a projectile locking its target must stay within of
    /// its owner to land.
    pub move_range: i64,
    #[serde(default)]
    pub owner: Option<ObjectRef>,
    pub position: QVec3,
    pub projectile_id: u64,
    #[serde(default)]
    pub spawn_containing_shields: Vec<ObjectRef>,
    #[serde(default)]
    pub target: Option<ObjectRef>,
    pub team_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GaugeI32 {
    pub current: i32,
    pub maximum: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BuildingState {
    pub available: bool,
    pub bounds_height: i64,
    pub bounds_width: i64,
    pub building_id: u64,
    pub building_type_id: u32,
    pub collision_enabled: bool,
    pub life: GaugeI32,
    pub position: QVec3,
    pub targetable: bool,
    pub team_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ShieldSourceKind {
    Contraption,
    CommanderSkill,
    OwnerAdvanced,
    SpawnedTemporary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ShieldRoundPolicy {
    DestroyAtRoundEnd,
    ResetToMax,
    RetainState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ShieldDestroyedReason {
    EnergyDepleted,
    OwnerDestroyed,
    RoundEnd,
    Scripted,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TerrainRemovedReason {
    TimeExpired,
    RoundExpired,
    GridDepleted,
    Cleared,
    Unknown,
}

/// Why a buff left the actor it was on: the caller of
/// `BuffManager.RemoveBuff(Buff)`, the one method a buff leaves through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BuffRemovedReason {
    /// Its time ran out, in `BuffManager.Update`.
    Expired,
    /// A skill or a technology took it off by its data,
    /// `RemoveBuff(IBuffData)`.
    Removed,
    /// Its actor changed side, `RemoveBuffEffect`.
    TeamChanged,
    /// Its actor's technologies were disabled,
    /// `ClearSelfResourceBuffByDisableTech`.
    TechnologyDisabled,
    /// Its actor was torn down, `Clear`.
    Cleared,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ShieldState {
    pub active: bool,
    #[serde(default)]
    pub active_order: Option<u32>,
    pub energy: GaugeI32,
    #[serde(default)]
    pub owner: Option<ObjectRef>,
    pub position: QVec3,
    pub radius: i64,
    pub round_policy: ShieldRoundPolicy,
    pub shield_id: u64,
    pub source_kind: ShieldSourceKind,
    pub team_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TerrainType {
    Fire,
    Oil,
    Fog,
    Acid,
    RecoveryZone,
    FogSand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TerrainGridState {
    pub origin_x: i64,
    pub origin_y: i64,
    pub rows: Vec<u32>,
    pub size_x: u32,
    pub size_y: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TerrainLogicLifetime {
    pub elapsed: i32,
    pub limit: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TerrainApplicationState {
    #[serde(default)]
    pub periodic_clock: Option<TerrainEffectClock>,
    pub unit_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TerrainEffectClock {
    pub duration: i32,
    pub elapsed: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TerrainState {
    #[serde(default)]
    pub applications: Vec<TerrainApplicationState>,
    #[serde(default)]
    pub grid: Option<TerrainGridState>,
    #[serde(default)]
    pub logic_lifetime: Option<TerrainLogicLifetime>,
    pub position: QVec3,
    pub radius: i64,
    #[serde(default)]
    pub remaining_rounds: Option<u32>,
    #[serde(default)]
    pub team_id: Option<u32>,
    pub terrain_id: u64,
    pub terrain_type: TerrainType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TransitionEvents {
    #[serde(default)]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub payload: EventPayload,
    #[serde(default)]
    pub source: Option<ObjectRef>,
    #[serde(default)]
    pub source_team_id: Option<u32>,
    #[serde(default)]
    pub subject: Option<ObjectRef>,
    #[serde(default)]
    pub target: Option<ObjectRef>,
}

impl Event {
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        self.payload.kind()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    ProjectileReleased,
    ProjectileRemoved,
    Damage,
    UnitCreated,
    UnitDied,
    BuildingDestroyed,
    UnitTeamChanged,
    ShieldCreated,
    ShieldDestroyed,
    TerrainCreated,
    TerrainRemoved,
    TerrainConverted,
    Healing,
    BuffApplied,
    BuffRemoved,
    TeamScored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventPayload {
    ProjectileReleased {
        skill_slot: Option<u16>,
        weapon_index: Option<i32>,
    },
    ProjectileRemoved {
        #[serde(default)]
        absorbed_by: Option<ObjectRef>,
        intercepted: bool,
        position: QVec3,
    },
    Damage {
        amount: i32,
        /// The index in the source's `GetSkills()` of the skill that dealt
        /// it; null for damage no skill deals, such as a death explosion, a
        /// commander skill, an air drop or ground fire.
        #[serde(default)]
        skill_slot: Option<u16>,
    },
    UnitCreated {
        formation_id: u64,
        position: QVec3,
        team_id: u32,
        unit_type_id: u32,
    },
    UnitDied {
        position: QVec3,
    },
    BuildingDestroyed {
        position: QVec3,
    },
    UnitTeamChanged {
        new_team_id: u32,
        previous_team_id: u32,
    },
    ShieldCreated {
        position: QVec3,
        source_kind: ShieldSourceKind,
        team_id: u32,
    },
    ShieldDestroyed {
        position: QVec3,
        reason: ShieldDestroyedReason,
    },
    TerrainCreated {
        position: QVec3,
        radius: i64,
        team_id: Option<u32>,
        terrain_type: TerrainType,
    },
    TerrainRemoved {
        position: QVec3,
        reason: TerrainRemovedReason,
    },
    TerrainConverted {
        position: QVec3,
    },
    Healing {
        amount: i32,
    },
    /// A buff put on, or put on again, the target. `buff_id` is its data's
    /// `GetID()`; `duration` is the ticks left on it once applied.
    BuffApplied {
        buff_id: u32,
        duration: i32,
    },
    BuffRemoved {
        buff_id: u32,
        reason: BuffRemovedReason,
    },
    /// What a side's units standing at the fight's end score, on the
    /// fight's last tick: `FightResultController.CalculateScore` of the
    /// side's team, alive.
    TeamScored {
        amount: i32,
        team_id: u32,
    },
}

impl EventPayload {
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        match self {
            Self::ProjectileReleased { .. } => EventKind::ProjectileReleased,
            Self::ProjectileRemoved { .. } => EventKind::ProjectileRemoved,
            Self::Damage { .. } => EventKind::Damage,
            Self::UnitCreated { .. } => EventKind::UnitCreated,
            Self::UnitDied { .. } => EventKind::UnitDied,
            Self::BuildingDestroyed { .. } => EventKind::BuildingDestroyed,
            Self::UnitTeamChanged { .. } => EventKind::UnitTeamChanged,
            Self::ShieldCreated { .. } => EventKind::ShieldCreated,
            Self::ShieldDestroyed { .. } => EventKind::ShieldDestroyed,
            Self::TerrainCreated { .. } => EventKind::TerrainCreated,
            Self::TerrainRemoved { .. } => EventKind::TerrainRemoved,
            Self::TerrainConverted { .. } => EventKind::TerrainConverted,
            Self::Healing { .. } => EventKind::Healing,
            Self::BuffApplied { .. } => EventKind::BuffApplied,
            Self::BuffRemoved { .. } => EventKind::BuffRemoved,
            Self::TeamScored { .. } => EventKind::TeamScored,
        }
    }
}
