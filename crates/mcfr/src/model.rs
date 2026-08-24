use std::{cmp::Ordering, collections::BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Error, Result, canonical};

pub const MCFR_FORMAT: &str = "mechcore.mcfr";
pub const MCFR_SCHEMA_VERSION: u32 = 2;
pub const MCFR_CONTAINER_VERSION: u32 = 2;
pub const INSTRUMENTATION_FORMAT: &str = "mechcore.mcfr.instrumentation";
pub const INSTRUMENTATION_CONTAINER_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hashes {
    pub scenario_hash: String,
    pub result_hash: String,
}

impl Hashes {
    pub(crate) fn from_raw(
        scenario: [u8; canonical::HASH_BYTES],
        result: [u8; canonical::HASH_BYTES],
    ) -> Self {
        Self {
            scenario_hash: canonical::hex(&scenario),
            result_hash: canonical::hex(&result),
        }
    }

    pub(crate) fn validate_encoding(&self) -> Result<()> {
        canonical::parse_hex(&self.scenario_hash, "scenario_hash")?;
        canonical::parse_hex(&self.result_hash, "result_hash")?;
        Ok(())
    }
}

/// One end-of-logical-tick comparison unit.
///
/// `events` are the native events observed while advancing from the preceding
/// snapshot to this tick's `state`. Tick zero therefore has an empty event batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickSlice {
    pub tick: u64,
    pub state: WorldSnapshot,
    pub events: TransitionEvents,
    pub tick_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableContext {
    pub schema_version: u32,
    pub game_build: String,
    pub logic_step: Rational,
    pub numeric_convention: NumericConvention,
    pub combat_round: u32,
    pub match_seed: i32,
    pub identity_contract: IdentityContract,
}

impl DurableContext {
    /// Validates the durable context against the schema implemented by this crate.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported schema versions, missing build identity, or invalid
    /// timing and numeric ratios.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != MCFR_SCHEMA_VERSION {
            return Err(Error::invalid(format!(
                "unsupported MCFR schema version {}",
                self.schema_version
            )));
        }
        require_text(&self.game_build, "game_build")?;
        if self.combat_round == 0 {
            return Err(Error::invalid("combat_round must be positive"));
        }
        self.logic_step.validate("logic_step")?;
        self.numeric_convention.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumericConvention {
    pub distance_units_per_meter: u64,
    pub rotation_units_per_degree: u64,
    pub time_units_per_second: u64,
}

impl NumericConvention {
    fn validate(self) -> Result<()> {
        if self.distance_units_per_meter == 0
            || self.rotation_units_per_degree == 0
            || self.time_units_per_second == 0
        {
            return Err(Error::invalid("numeric convention scales must be positive"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityContract {
    TeamZxSequentialV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rational {
    pub numerator: u64,
    pub denominator: u64,
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
#[serde(deny_unknown_fields)]
pub struct WorldSnapshot {
    #[serde(default)]
    pub units: Vec<UnitState>,
    #[serde(default)]
    pub projectiles: Vec<ProjectileState>,
    #[serde(default)]
    pub buildings: Vec<BuildingState>,
    #[serde(default)]
    pub statuses: Vec<StatusState>,
}

impl WorldSnapshot {
    pub fn canonicalize(&mut self) {
        self.units.sort_by_key(|value| value.unit_id);
        self.projectiles.sort_by_key(|value| value.projectile_id);
        self.buildings.sort_by_key(|value| value.building_id);
        self.statuses.sort_by_key(|value| value.status_id);
    }

    /// Returns all top-level object identities in the snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when an identity is zero or duplicated within its object kind.
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
        Ok(keys)
    }

    fn iter_object_keys(&self) -> impl Iterator<Item = ObjectRef> + '_ {
        self.units
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
                self.statuses
                    .iter()
                    .map(|value| ObjectRef::new(ObjectKind::Status, value.status_id)),
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Unit,
    Projectile,
    Building,
    Status,
}

impl ObjectKind {
    const COUNT: usize = 4;
    const ALL: [Self; Self::COUNT] = [Self::Unit, Self::Projectile, Self::Building, Self::Status];

    const fn index(self) -> usize {
        match self {
            Self::Unit => 0,
            Self::Projectile => 1,
            Self::Building => 2,
            Self::Status => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectRef {
    pub kind: ObjectKind,
    pub id: u64,
}

impl ObjectRef {
    #[must_use]
    pub const fn new(kind: ObjectKind, id: u64) -> Self {
        Self { kind, id }
    }
}

/// Allocates source-neutral identities for MCFR objects and formations.
///
/// Object identities use independent namespaces per [`ObjectKind`]. Formations use a separate
/// namespace. Every namespace starts at one and advances without gaps.
#[derive(Debug, Clone)]
pub struct IdentityAllocator {
    next_object_ids: [u64; ObjectKind::COUNT],
    next_formation_id: u64,
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
        allocator.observe_formations(snapshot)?;
        Ok(allocator)
    }

    pub(crate) fn observe_formations(&mut self, snapshot: &WorldSnapshot) -> Result<()> {
        let formation_ids = snapshot
            .units
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
#[serde(deny_unknown_fields)]
pub struct Vec3 {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    pub position: Vec3,
    pub rotation: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Ground,
    Air,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionState {
    Idle,
    Moving,
    Attacking,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Normal,
    Disappear,
    Stealth,
    Hide,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitState {
    pub unit_id: u64,
    pub team_id: u32,
    pub formation_id: u64,
    pub unit_type_id: u32,
    pub domain: Domain,
    pub position: Vec3,
    pub body_rotation: i64,
    pub aim_pose: Pose,
    pub velocity: Vec3,
    pub motion_state: MotionState,
    pub collision_radius: i64,
    pub life: i64,
    pub max_life: i64,
    pub alive: bool,
    pub active: bool,
    pub targetable: bool,
    pub visibility: Visibility,
    pub personal_shield: PersonalShieldState,
}

/// Compares initial units in build-2227 `FightTeam.PrepareActors` order.
///
/// Teams are visited first; members within one team use ascending world `z`, then ascending
/// world `x`. Equal positions within one team are not ordered by a synthetic tie-breaker.
#[must_use]
pub fn compare_initial_unit_order(
    left_team: u32,
    left_position: &Vec3,
    right_team: u32,
    right_position: &Vec3,
) -> Ordering {
    left_team
        .cmp(&right_team)
        .then_with(|| left_position.z.cmp(&right_position.z))
        .then_with(|| left_position.x.cmp(&right_position.x))
}

fn validate_initial_unit_order(snapshot: &WorldSnapshot) -> Result<()> {
    for pair in snapshot.units.windows(2) {
        let ordering = compare_initial_unit_order(
            pair[0].team_id,
            &pair[0].position,
            pair[1].team_id,
            &pair[1].position,
        );
        if ordering != Ordering::Less {
            return Err(Error::invalid(
                "initial unit identities must follow ascending team, world z, then world x",
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonalShieldState {
    pub active: bool,
    pub enabled: bool,
    pub energy: i64,
    pub max_energy: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectileState {
    pub projectile_id: u64,
    pub team_id: u32,
    #[serde(default)]
    pub owner: Option<ObjectRef>,
    pub position: Vec3,
    pub orientation: i64,
    #[serde(default)]
    pub target: Option<ObjectRef>,
    pub cached_target_position: Vec3,
    pub cached_target_radius: i64,
    pub released: bool,
    pub life: Gauge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gauge {
    pub current: i64,
    pub maximum: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // These are independent game-observable flags.
pub struct BuildingState {
    pub building_id: u64,
    pub team_id: u32,
    pub building_type_id: u32,
    pub position: Vec3,
    pub rotation: i64,
    pub bounds_width: i64,
    pub bounds_height: i64,
    pub life: i64,
    pub max_life: i64,
    pub alive: bool,
    pub destroyed: bool,
    pub available: bool,
    pub targetable: bool,
    pub collision_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusState {
    pub status_id: u64,
    pub status_type_id: u32,
    #[serde(default)]
    pub source: Option<ObjectRef>,
    pub target: ObjectRef,
    pub additive_stack: i32,
    pub duration_time: i32,
    pub max_duration_time: i32,
    pub step_time: i32,
    pub step_time_config: i32,
    pub finished: bool,
    pub frozen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionEvents {
    #[serde(default)]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    #[serde(default)]
    pub subject: Option<ObjectRef>,
    #[serde(default)]
    pub source: Option<ObjectRef>,
    #[serde(default)]
    pub target: Option<ObjectRef>,
    pub payload: EventPayload,
}

impl Event {
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        self.payload.kind()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    ProjectileReleased,
    ProjectileRemoved,
    Damage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventPayload {
    ProjectileReleased,
    ProjectileRemoved { position: Vec3, intercepted: bool },
    Damage { amount: i64 },
}

impl EventPayload {
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        match self {
            Self::ProjectileReleased => EventKind::ProjectileReleased,
            Self::ProjectileRemoved { .. } => EventKind::ProjectileRemoved,
            Self::Damage { .. } => EventKind::Damage,
        }
    }
}

fn require_text(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::invalid(format!("{label} must not be empty")));
    }
    Ok(())
}
