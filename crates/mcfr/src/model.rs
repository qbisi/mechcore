use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result, canonical};

pub const MCFR_FORMAT: &str = "mechcore.mcfr";
pub const MCFR_SCHEMA_VERSION: u32 = 1;
pub const MCFR_CONTAINER_VERSION: u32 = 1;
pub const INSTRUMENTATION_FORMAT: &str = "mechcore.mcfr.instrumentation";
pub const INSTRUMENTATION_CONTAINER_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hashes {
    pub scenario_hash: String,
    pub state_hash: String,
    pub event_hash: String,
    pub result_hash: String,
}

impl Hashes {
    pub(crate) fn from_raw(
        scenario: [u8; canonical::HASH_BYTES],
        state: [u8; canonical::HASH_BYTES],
        event: [u8; canonical::HASH_BYTES],
        result: [u8; canonical::HASH_BYTES],
    ) -> Self {
        Self {
            scenario_hash: canonical::hex(&scenario),
            state_hash: canonical::hex(&state),
            event_hash: canonical::hex(&event),
            result_hash: canonical::hex(&result),
        }
    }

    pub(crate) fn validate_encoding(&self) -> Result<()> {
        canonical::parse_hex(&self.scenario_hash, "scenario_hash")?;
        canonical::parse_hex(&self.state_hash, "state_hash")?;
        canonical::parse_hex(&self.event_hash, "event_hash")?;
        canonical::parse_hex(&self.result_hash, "result_hash")?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableContext {
    pub schema_version: u32,
    pub game_build: String,
    pub rules_fingerprint: String,
    pub logic_step: Rational,
    pub numeric_convention: String,
    pub rng_state: Value,
    pub identity_contract: String,
    pub update_order_contract: String,
    #[serde(default)]
    pub durable_commands: Vec<Value>,
}

impl DurableContext {
    /// Validates the durable context against the schema implemented by this crate.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported schema versions, missing contract identifiers, or an
    /// invalid logic-step ratio.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != MCFR_SCHEMA_VERSION {
            return Err(Error::invalid(format!(
                "unsupported MCFR schema version {}",
                self.schema_version
            )));
        }
        require_text(&self.game_build, "game_build")?;
        require_text(&self.rules_fingerprint, "rules_fingerprint")?;
        require_text(&self.numeric_convention, "numeric_convention")?;
        require_text(&self.identity_contract, "identity_contract")?;
        require_text(&self.update_order_contract, "update_order_contract")?;
        self.logic_step.validate("logic_step")
    }

    pub(crate) fn canonicalize(&mut self) {
        canonical::normalize(&mut self.rng_state);
        for command in &mut self.durable_commands {
            canonical::normalize(command);
        }
    }
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
    pub area_shields: Vec<AreaShieldState>,
    #[serde(default)]
    pub dynamic_terrain: Vec<DynamicTerrainState>,
    #[serde(default)]
    pub statuses: Vec<StatusState>,
}

impl WorldSnapshot {
    pub fn canonicalize(&mut self) {
        self.units.sort_by_key(|value| value.unit_id);
        self.projectiles.sort_by_key(|value| value.projectile_id);
        self.buildings.sort_by_key(|value| value.building_id);
        self.area_shields.sort_by_key(|value| value.shield_id);
        self.dynamic_terrain.sort_by_key(|value| value.terrain_id);
        for terrain in &mut self.dynamic_terrain {
            if let TerrainRegion::Grid { cells, .. } = &mut terrain.region {
                cells.sort_by_key(|cell| (cell.x, cell.y));
            }
        }
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

    pub(crate) fn validate(&self, known: &BTreeSet<ObjectRef>) -> Result<()> {
        let current = self.object_keys()?;
        let resolvable = known.union(&current).copied().collect::<BTreeSet<_>>();
        for unit in &self.units {
            if unit.max_life < 0 || unit.life < 0 || unit.life > unit.max_life {
                return Err(Error::invalid(format!(
                    "unit {} has an invalid life gauge",
                    unit.unit_id
                )));
            }
            if unit.collision_radius < 0 {
                return Err(Error::invalid(format!(
                    "unit {} has a negative collision radius",
                    unit.unit_id
                )));
            }
            if let Some(parent) = unit.parent_unit_id {
                resolve(
                    ObjectRef::new(ObjectKind::Unit, parent),
                    &resolvable,
                    "unit parent",
                )?;
            }
            if let Some(shield) = &unit.personal_shield {
                shield.validate("personal shield")?;
            }
        }
        for projectile in &self.projectiles {
            if projectile.cached_target_radius < 0 {
                return Err(Error::invalid(format!(
                    "projectile {} has a negative cached target radius",
                    projectile.projectile_id
                )));
            }
            if let Some(life) = &projectile.life {
                life.validate("projectile life")?;
            }
            for reference in [projectile.owner, projectile.source, projectile.target]
                .into_iter()
                .flatten()
            {
                resolve(reference, &resolvable, "projectile reference")?;
            }
        }
        for building in &self.buildings {
            if building.max_life < 0 || building.life < 0 || building.life > building.max_life {
                return Err(Error::invalid(format!(
                    "building {} has an invalid life gauge",
                    building.building_id
                )));
            }
            building.collision.validate()?;
        }
        for shield in &self.area_shields {
            shield.energy.validate("area shield energy")?;
            if shield.radius < 0 || shield.height.is_some_and(|height| height < 0) {
                return Err(Error::invalid(format!(
                    "area shield {} has invalid geometry",
                    shield.shield_id
                )));
            }
            for reference in [shield.owner, shield.source].into_iter().flatten() {
                resolve(reference, &resolvable, "area shield reference")?;
            }
        }
        for terrain in &self.dynamic_terrain {
            terrain.region.validate()?;
            if let Some(source) = terrain.source {
                resolve(source, &resolvable, "dynamic terrain source")?;
            }
        }
        for status in &self.statuses {
            if status.stack == 0 {
                return Err(Error::invalid(format!(
                    "status {} has a zero stack",
                    status.status_id
                )));
            }
            if status.elapsed > status.max_duration || status.remaining > status.max_duration {
                return Err(Error::invalid(format!(
                    "status {} has an invalid duration",
                    status.status_id
                )));
            }
            resolve(status.target, &resolvable, "status target")?;
            if let Some(source) = status.source {
                resolve(source, &resolvable, "status source")?;
            }
        }
        Ok(())
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
                self.area_shields
                    .iter()
                    .map(|value| ObjectRef::new(ObjectKind::AreaShield, value.shield_id)),
            )
            .chain(
                self.dynamic_terrain
                    .iter()
                    .map(|value| ObjectRef::new(ObjectKind::DynamicTerrain, value.terrain_id)),
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
    AreaShield,
    DynamicTerrain,
    Status,
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
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Visible,
    Hidden,
    Stealth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitState {
    pub unit_id: u64,
    pub team_id: u32,
    pub formation_id: u64,
    pub unit_type_id: u32,
    #[serde(default)]
    pub parent_unit_id: Option<u64>,
    pub domain: Domain,
    pub position: Vec3,
    pub body_rotation: i64,
    #[serde(default)]
    pub aim_pose: Option<Pose>,
    pub velocity: Vec3,
    pub motion_state: MotionState,
    pub collision_radius: i64,
    pub life: i64,
    pub max_life: i64,
    pub alive: bool,
    pub active: bool,
    pub targetable: bool,
    pub visibility: Visibility,
    #[serde(default)]
    pub personal_shield: Option<PersonalShieldState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonalShieldState {
    pub active: bool,
    pub enabled: bool,
    pub energy: i64,
    pub max_energy: i64,
}

impl PersonalShieldState {
    fn validate(self, label: &str) -> Result<()> {
        Gauge {
            current: self.energy,
            maximum: self.max_energy,
        }
        .validate(label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectileState {
    pub projectile_id: u64,
    pub team_id: u32,
    #[serde(default)]
    pub owner: Option<ObjectRef>,
    #[serde(default)]
    pub source: Option<ObjectRef>,
    pub projectile_type_id: u32,
    pub position: Vec3,
    #[serde(default)]
    pub orientation: Option<i64>,
    #[serde(default)]
    pub velocity: Option<Vec3>,
    #[serde(default)]
    pub target: Option<ObjectRef>,
    pub cached_target_position: Vec3,
    pub cached_target_radius: i64,
    pub active: bool,
    #[serde(default)]
    pub life: Option<Gauge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gauge {
    pub current: i64,
    pub maximum: i64,
}

impl Gauge {
    fn validate(self, label: &str) -> Result<()> {
        if self.current < 0 || self.maximum < 0 || self.current > self.maximum {
            return Err(Error::invalid(format!("{label} has an invalid gauge")));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // These are independent game-observable flags.
pub struct BuildingState {
    pub building_id: u64,
    pub team_id: u32,
    #[serde(default)]
    pub formation_id: Option<u64>,
    pub building_type_id: u32,
    pub position: Vec3,
    pub rotation: i64,
    pub collision: CollisionBoundary,
    pub life: i64,
    pub max_life: i64,
    pub alive: bool,
    pub active: bool,
    pub available: bool,
    pub targetable: bool,
    pub collision_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CollisionBoundary {
    Circle { radius: i64 },
    Rectangle { half_width: i64, half_height: i64 },
}

impl CollisionBoundary {
    fn validate(&self) -> Result<()> {
        let valid = match self {
            Self::Circle { radius } => *radius >= 0,
            Self::Rectangle {
                half_width,
                half_height,
            } => *half_width >= 0 && *half_height >= 0,
        };
        if !valid {
            return Err(Error::invalid("collision boundary has negative geometry"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreaShieldState {
    pub shield_id: u64,
    pub team_id: u32,
    #[serde(default)]
    pub owner: Option<ObjectRef>,
    #[serde(default)]
    pub source: Option<ObjectRef>,
    pub position: Vec3,
    pub radius: i64,
    #[serde(default)]
    pub height: Option<i64>,
    pub energy: Gauge,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicTerrainState {
    pub terrain_id: u64,
    pub team_id: u32,
    #[serde(default)]
    pub source: Option<ObjectRef>,
    pub terrain_type_id: u32,
    pub position: Vec3,
    pub region: TerrainRegion,
    pub active: bool,
    pub elapsed: u64,
    pub remaining: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerrainRegion {
    Circle {
        radius: i64,
    },
    Grid {
        cell_size: i64,
        cells: Vec<GridCell>,
    },
}

impl TerrainRegion {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Circle { radius } if *radius >= 0 => Ok(()),
            Self::Grid { cell_size, cells } if *cell_size > 0 => {
                let unique = cells.iter().copied().collect::<HashSet<_>>();
                if unique.len() != cells.len() {
                    return Err(Error::invalid("terrain grid contains duplicate cells"));
                }
                Ok(())
            }
            _ => Err(Error::invalid("dynamic terrain has invalid geometry")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridCell {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusState {
    pub status_id: u64,
    pub status_type_id: u32,
    #[serde(default)]
    pub source: Option<ObjectRef>,
    pub target: ObjectRef,
    pub stack: u32,
    pub elapsed: u64,
    pub remaining: u64,
    pub max_duration: u64,
    pub active: bool,
    #[serde(default)]
    pub periodic_clock: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionEvents {
    #[serde(default)]
    pub events: Vec<Event>,
}

impl TransitionEvents {
    pub fn canonicalize(&mut self) {
        for event in &mut self.events {
            canonical::normalize(&mut event.payload);
        }
    }

    pub(crate) fn validate(&self, known_before: &BTreeSet<ObjectRef>) -> Result<EventValidation> {
        let mut known = known_before.clone();
        let mut created = BTreeSet::new();
        let mut removed = BTreeSet::new();
        for (expected, event) in self.events.iter().enumerate() {
            let expected_seq = u32::try_from(expected)
                .map_err(|_| Error::invalid("event sequence exceeds u32"))?;
            if event.event_seq != expected_seq {
                return Err(Error::invalid(format!(
                    "event sequence {} is not contiguous at index {expected}",
                    event.event_seq
                )));
            }
            if event.kind.is_creation() {
                let subject = event.subject.ok_or_else(|| {
                    Error::invalid(format!("{:?} event lacks a subject", event.kind))
                })?;
                if subject.id == 0 || known.contains(&subject) || !created.insert(subject) {
                    return Err(Error::invalid(format!(
                        "{:?} creates an invalid or reused object {:?}",
                        event.kind, subject
                    )));
                }
                known.insert(subject);
            }
            for (label, reference) in [
                ("event subject", event.subject),
                ("event source", event.source),
                ("event target", event.target),
            ] {
                if let Some(reference) = reference {
                    resolve(reference, &known, label)?;
                }
            }
            if event.kind.is_removal() {
                let subject = event.subject.ok_or_else(|| {
                    Error::invalid(format!("{:?} event lacks a subject", event.kind))
                })?;
                if !removed.insert(subject) {
                    return Err(Error::invalid(format!(
                        "object {subject:?} is removed more than once in one transition"
                    )));
                }
            }
        }
        Ok(EventValidation {
            known_after: known,
            created,
            removed,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub event_seq: u32,
    pub kind: EventKind,
    #[serde(default)]
    pub subject: Option<ObjectRef>,
    #[serde(default)]
    pub source: Option<ObjectRef>,
    #[serde(default)]
    pub target: Option<ObjectRef>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    ObjectCreated,
    ObjectRemoved,
    ActionStarted,
    ActionReleased,
    ProjectileReleased,
    ProjectileImpacted,
    ProjectileIntercepted,
    ProjectileRemoved,
    ShieldHit,
    ShieldDeactivated,
    StatusApplied,
    StatusRefreshed,
    StatusExtended,
    StatusRemoved,
    DynamicTerrainCreated,
    DynamicTerrainRegionChanged,
    DynamicTerrainLifetimeReset,
    DynamicTerrainRemoved,
    DynamicTerrainEffect,
    Damage,
    Healing,
    Death,
}

impl EventKind {
    fn is_creation(self) -> bool {
        matches!(
            self,
            Self::ObjectCreated
                | Self::ProjectileReleased
                | Self::StatusApplied
                | Self::DynamicTerrainCreated
        )
    }

    fn is_removal(self) -> bool {
        matches!(
            self,
            Self::ObjectRemoved
                | Self::ProjectileRemoved
                | Self::StatusRemoved
                | Self::DynamicTerrainRemoved
        )
    }
}

pub(crate) struct EventValidation {
    pub(crate) known_after: BTreeSet<ObjectRef>,
    pub(crate) created: BTreeSet<ObjectRef>,
    pub(crate) removed: BTreeSet<ObjectRef>,
}

pub(crate) fn validate_transition(
    current: &WorldSnapshot,
    events: &TransitionEvents,
    next: &WorldSnapshot,
    known: &BTreeSet<ObjectRef>,
) -> Result<BTreeSet<ObjectRef>> {
    let current_keys = current.object_keys()?;
    let validation = events.validate(known)?;
    next.validate(&validation.known_after)?;
    let next_keys = next.object_keys()?;
    let appeared = next_keys
        .difference(&current_keys)
        .copied()
        .collect::<BTreeSet<_>>();
    let disappeared = current_keys
        .difference(&next_keys)
        .copied()
        .collect::<BTreeSet<_>>();
    if !appeared.is_subset(&validation.created) {
        let missing = appeared
            .difference(&validation.created)
            .next()
            .copied()
            .expect("non-subset has a member");
        return Err(Error::invalid(format!(
            "object {missing:?} appears without a creation event"
        )));
    }
    if !disappeared.is_subset(&validation.removed) {
        let missing = disappeared
            .difference(&validation.removed)
            .next()
            .copied()
            .expect("non-subset has a member");
        return Err(Error::invalid(format!(
            "object {missing:?} disappears without a removal event"
        )));
    }
    Ok(validation.known_after.union(&next_keys).copied().collect())
}

fn resolve(reference: ObjectRef, known: &BTreeSet<ObjectRef>, label: &str) -> Result<()> {
    if !known.contains(&reference) {
        return Err(Error::invalid(format!(
            "{label} refers to unknown object {reference:?}"
        )));
    }
    Ok(())
}

fn require_text(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::invalid(format!("{label} must not be empty")));
    }
    Ok(())
}
