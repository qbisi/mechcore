use std::{cmp::Ordering, collections::BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Error, Result, canonical};

pub const MCFR_FORMAT: &str = "0.7.0";
pub const PHYSICS_HASH_PROFILE: &str = "battle-physics-v2";
pub const CONTENT_HASH_PROFILE: &str = "mcfr-content-0.7.0";
pub const INSTRUMENTATION_FORMAT: &str = "mechcore.mcfr.instrumentation";
pub const INSTRUMENTATION_CONTAINER_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hashes {
    pub physics_result_hash: String,
    pub content_result_hash: String,
}

impl Hashes {
    pub(crate) fn from_raw(
        physics: [u8; canonical::HASH_BYTES],
        content: [u8; canonical::HASH_BYTES],
    ) -> Self {
        Self {
            physics_result_hash: canonical::hex(&physics),
            content_result_hash: canonical::hex(&content),
        }
    }

    pub(crate) fn validate_encoding(&self) -> Result<()> {
        canonical::parse_hex(&self.physics_result_hash, "physics_result_hash")?;
        canonical::parse_hex(&self.content_result_hash, "content_result_hash")?;
        Ok(())
    }
}

/// One end-of-logical-tick comparison unit.
///
/// `events` are the native events observed while advancing to this tick's
/// `state`. MCFR begins at tick one; `S(0)` and `E(0)` are not stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickSlice {
    pub tick: u32,
    pub state: WorldSnapshot,
    pub events: TransitionEvents,
    pub physics_tick_hash: String,
    pub content_tick_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickHashes {
    pub physics_tick_hash: String,
    pub content_tick_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableContext {
    pub logic_step: Rational,
    pub time_units_per_second: u32,
    pub combat_round: u32,
    pub match_seed: i32,
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
#[serde(deny_unknown_fields)]
pub struct Rational {
    pub numerator: u32,
    pub denominator: u32,
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
    pub live_units: Vec<LiveUnitState>,
    #[serde(default)]
    pub projectiles: Vec<ProjectileState>,
    #[serde(default)]
    pub buildings: Vec<BuildingState>,
    #[serde(default)]
    pub shields: Vec<ShieldState>,
    #[serde(default)]
    pub terrains: Vec<TerrainState>,
}

impl WorldSnapshot {
    pub fn canonicalize(&mut self) {
        for unit in &mut self.live_units {
            unit.skill_dynamic_modifiers
                .retain(|skill| !skill.modifiers.is_zero());
        }
        self.live_units.sort_by_key(|value| value.unit_id);
        self.projectiles.sort_by_key(|value| value.projectile_id);
        self.buildings.sort_by_key(|value| value.building_id);
        self.shields.sort_by_key(|value| value.shield_id);
        self.terrains.sort_by_key(|value| value.terrain_id);
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
            if unit.status_mask & !0x0f != 0 {
                return Err(Error::invalid(format!(
                    "unit {} status_mask uses reserved bits",
                    unit.unit_id
                )));
            }
            unit.buff_modifiers.validate("buff_modifiers")?;
            unit.unit_dynamic_modifiers.validate()?;
            let mut previous_slot = None;
            for skill in &unit.skill_dynamic_modifiers {
                if skill.modifiers.is_zero() {
                    return Err(Error::invalid(format!(
                        "unit {} zero skill modifiers must be omitted",
                        unit.unit_id
                    )));
                }
                if previous_slot.is_some_and(|previous| skill.skill_slot <= previous) {
                    return Err(Error::invalid(format!(
                        "unit {} skill modifiers are not strictly ordered by skill_slot",
                        unit.unit_id
                    )));
                }
                skill.modifiers.validate()?;
                previous_slot = Some(skill.skill_slot);
            }
            let mut previous_weapon = None;
            for weapon in &unit.weapon_aims {
                let key = (weapon.skill_slot, weapon.weapon_index);
                if previous_weapon.is_some_and(|previous| key <= previous) {
                    return Err(Error::invalid(format!(
                        "unit {} weapon aims are not strictly ordered",
                        unit.unit_id
                    )));
                }
                previous_weapon = Some(key);
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
#[serde(deny_unknown_fields)]
pub struct QVec3 {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QPose {
    pub position: QVec3,
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
    /// Between two states while a move ability runs: `MotionFSM` holds its
    /// `TransitionState` until the ability hands it the next one.
    Transitioning,
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
pub struct LiveUnitState {
    pub unit_id: u64,
    pub team_id: u32,
    pub original_team_id: u32,
    pub formation_id: u64,
    pub unit_type_id: u32,
    pub domain: Domain,
    pub position: QVec3,
    pub body_rotation: i64,
    /// The rotation of the unit's turret, `FightMech.mechBody`'s
    /// `FightTransform`: what a unit with a body turns toward its attack
    /// target and measures its attack angle from, while `body_rotation`
    /// keeps the chassis. Null for a unit without a body.
    pub turret_rotation: Option<i64>,
    pub velocity: QVec3,
    pub motion_state: MotionState,
    pub mech_lock_target: Option<ObjectRef>,
    pub collision_radius: i64,
    pub life: GaugeI32,
    pub active: bool,
    pub targetable: bool,
    pub visibility: Visibility,
    pub status_mask: u64,
    pub buff_modifiers: BuffModifierSet,
    pub unit_dynamic_modifiers: UnitDynamicModifierSet,
    #[serde(default)]
    pub skill_dynamic_modifiers: Vec<SkillNumericModifierState>,
    pub personal_shield: PersonalShieldState,
    #[serde(default)]
    pub weapon_aims: Vec<WeaponAimState>,
    /// The numbers the fight reads, after every correction on them.
    #[serde(default)]
    pub derived: DerivedStats,
}

/// A unit's derived numbers, as the build's own properties answer them.
///
/// The modifier sets beside these say what was *written onto* a unit; these
/// say what the build then *computed* from them, which is the other half of
/// any measurement of how a correction composes. A recording that carries both
/// answers `(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)` in one tick,
/// rather than by arranging a fight whose outcome happens to distinguish the
/// candidates.
///
/// Only numbers that are exactly representable on both sides are here, which
/// is why an interval is the build's own integer rather than the `FPoint`
/// seconds its property answers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedStats {
    /// `MoveSpeedProperty`, `FPoint` raw.
    pub move_speed: i64,
    /// `AttackRangeProperty` of the skill the simulator models, `FPoint` raw.
    pub attack_range: i64,
    /// `DamageProperty.GetDamage()`, which the build keeps as a plain integer.
    pub attack_damage: i32,
    /// `FightSkill.GetCurrentAttackInterval()`: the interval **this cycle**
    /// was scheduled with, in whole logic ticks.
    ///
    /// It is not the description's interval and not a constant. Every cycle
    /// draws a stagger from the team's random stream and this carries the
    /// result, so one unit reads a different number from one cycle to the
    /// next and two units of a kind read different numbers at the same tick.
    /// `docs/rules/combat.md` measures the draw.
    pub current_attack_interval: i32,
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
#[serde(deny_unknown_fields)]
pub struct PersonalShieldState {
    pub active: bool,
    pub enabled: bool,
    pub energy: GaugeI32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponAimState {
    pub skill_slot: u16,
    pub weapon_index: i32,
    #[serde(default)]
    pub attack_target: Option<ObjectRef>,
    #[serde(default)]
    pub pose: Option<QPose>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RateModifier {
    pub add: i64,
    pub reduce: i64,
}

impl RateModifier {
    #[must_use]
    pub fn is_zero(self) -> bool {
        self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ValueModifier {
    pub add: i32,
    pub reduce: i32,
}

impl ValueModifier {
    #[must_use]
    pub fn is_zero(self) -> bool {
        self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BuffModifierSet {
    pub move_speed_rate: RateModifier,
    pub move_speed_value: ValueModifier,
    pub damage_rate: RateModifier,
    pub attack_interval_rate: RateModifier,
    pub extra_attack_interval_rate: RateModifier,
    pub amplify_damage_rate: RateModifier,
    pub attack_range_value: ValueModifier,
    pub extra_attack_range_value: ValueModifier,
    pub attack_range_rate: RateModifier,
    pub extra_attack_range_rate: RateModifier,
}

impl BuffModifierSet {
    #[must_use]
    pub fn is_zero(self) -> bool {
        self == Self::default()
    }

    fn validate(self, label: &str) -> Result<()> {
        let rates = [
            self.move_speed_rate,
            self.damage_rate,
            self.attack_interval_rate,
            self.extra_attack_interval_rate,
            self.amplify_damage_rate,
            self.attack_range_rate,
            self.extra_attack_range_rate,
        ];
        if rates
            .iter()
            .any(|modifier| modifier.add < 0 || modifier.reduce < 0)
        {
            return Err(Error::invalid(format!(
                "{label} rate add/reduce values must be nonnegative"
            )));
        }
        // A Value field is the native signed aggregate, which a round has
        // shown negative.
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct UnitDynamicModifierSet {
    pub gf_range_value: i64,
    pub gf_life_time_value: i64,
    pub mech_group_distance: i64,
    pub life_rate: RateModifier,
    pub life_rate_by_kill_count: RateModifier,
    pub reduce_damage_from_remote: RateModifier,
    pub move_ability_exit_time_change_rate: RateModifier,
    pub move_speed_change_rate: RateModifier,
    pub amplify_damage_rate: RateModifier,
    pub move_speed_value: i32,
    pub reduce_damage_value: i32,
    pub child_inherit_technology_effect: i32,
}

impl UnitDynamicModifierSet {
    #[must_use]
    pub fn is_zero(self) -> bool {
        self == Self::default()
    }

    fn validate(self) -> Result<()> {
        validate_rates(
            &[
                self.life_rate,
                self.life_rate_by_kill_count,
                self.reduce_damage_from_remote,
                self.move_ability_exit_time_change_rate,
                self.move_speed_change_rate,
                self.amplify_damage_rate,
            ],
            "unit_dynamic_modifiers",
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SkillDynamicModifierSet {
    pub min_attack_range_value: i64,
    pub attack_range_value: i64,
    pub attack_air_range_add_value: i64,
    pub attack_ground_range_add_value: i64,
    pub attack_interval_value: i64,
    pub damage_change_rate_ground: i64,
    pub damage_change_rate_air: i64,
    pub splash_range_value: i64,
    pub cb_life_recovery_rate: i64,
    pub projectile_speed_value: i64,
    pub attack_point_change_value: i64,
    pub projectile_duration_value: i64,
    pub projectile_random_range: i64,
    pub additional_damage_by_target_life: i64,
    pub damage_rate: RateModifier,
    pub damage_rate_by_kill_count: RateModifier,
    pub attack_range_rate: RateModifier,
    pub attack_interval_rate: RateModifier,
    pub damage_reduce_rate_base: RateModifier,
    pub projectile_life_rate: RateModifier,
    pub projectile_count_value: i32,
    pub air_attack_value: i32,
    pub ground_attack_value: i32,
    pub attack_range_value_air: i32,
    pub attack_range_value_ground: i32,
    pub is_lock_target: i32,
}

impl SkillDynamicModifierSet {
    #[must_use]
    pub fn is_zero(self) -> bool {
        self == Self::default()
    }

    fn validate(self) -> Result<()> {
        validate_rates(
            &[
                self.damage_rate,
                self.damage_rate_by_kill_count,
                self.attack_range_rate,
                self.attack_interval_rate,
                self.damage_reduce_rate_base,
                self.projectile_life_rate,
            ],
            "skill_dynamic_modifiers",
        )
    }
}

fn validate_rates(rates: &[RateModifier], label: &str) -> Result<()> {
    if rates
        .iter()
        .any(|modifier| modifier.add < 0 || modifier.reduce < 0)
    {
        return Err(Error::invalid(format!(
            "{label} rate add/reduce values must be nonnegative"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillNumericModifierState {
    pub skill_slot: u16,
    pub modifiers: SkillDynamicModifierSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectileState {
    pub projectile_id: u64,
    pub team_id: u32,
    #[serde(default)]
    pub owner: Option<ObjectRef>,
    pub position: QVec3,
    pub orientation: i64,
    #[serde(default)]
    pub target: Option<ObjectRef>,
    pub cached_target_position: QVec3,
    pub cached_target_radius: i64,
    pub released: bool,
    pub life: GaugeI32,
    #[serde(default)]
    pub spawn_containing_shields: Vec<ObjectRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GaugeI32 {
    pub current: i32,
    pub maximum: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildingState {
    pub building_id: u64,
    pub team_id: u32,
    pub building_type_id: u32,
    pub position: QVec3,
    pub bounds_width: i64,
    pub bounds_height: i64,
    pub life: GaugeI32,
    pub available: bool,
    pub targetable: bool,
    pub collision_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShieldSourceKind {
    Contraption,
    CommanderSkill,
    OwnerAdvanced,
    SpawnedTemporary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShieldRoundPolicy {
    DestroyAtRoundEnd,
    ResetToMax,
    RetainState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShieldDestroyedReason {
    EnergyDepleted,
    OwnerDestroyed,
    RoundEnd,
    Scripted,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerrainRemovedReason {
    TimeExpired,
    RoundExpired,
    GridDepleted,
    Cleared,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShieldState {
    pub shield_id: u64,
    pub team_id: u32,
    pub source_kind: ShieldSourceKind,
    #[serde(default)]
    pub owner: Option<ObjectRef>,
    pub position: QVec3,
    pub radius: i64,
    pub energy: GaugeI32,
    pub round_policy: ShieldRoundPolicy,
    pub active: bool,
    #[serde(default)]
    pub active_order: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[serde(deny_unknown_fields)]
pub struct TerrainGridState {
    pub origin_x: i64,
    pub origin_y: i64,
    pub size_x: u32,
    pub size_y: u32,
    pub rows: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainLogicLifetime {
    pub elapsed: i32,
    pub limit: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainApplicationState {
    pub unit_id: u64,
    #[serde(default)]
    pub periodic_clock: Option<TerrainEffectClock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainEffectClock {
    pub elapsed: i32,
    pub duration: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainState {
    pub terrain_id: u64,
    #[serde(default)]
    pub team_id: Option<u32>,
    pub terrain_type: TerrainType,
    pub position: QVec3,
    pub radius: i64,
    #[serde(default)]
    pub grid: Option<TerrainGridState>,
    #[serde(default)]
    pub remaining_rounds: Option<u32>,
    #[serde(default)]
    pub logic_lifetime: Option<TerrainLogicLifetime>,
    #[serde(default)]
    pub applications: Vec<TerrainApplicationState>,
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
    pub source_team_id: Option<u32>,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventPayload {
    ProjectileReleased {
        skill_slot: Option<u16>,
        weapon_index: Option<i32>,
    },
    ProjectileRemoved {
        position: QVec3,
        intercepted: bool,
        #[serde(default)]
        absorbed_by: Option<ObjectRef>,
    },
    Damage {
        amount: i32,
    },
    UnitCreated {
        team_id: u32,
        formation_id: u64,
        unit_type_id: u32,
        position: QVec3,
    },
    UnitDied {
        position: QVec3,
    },
    BuildingDestroyed {
        position: QVec3,
    },
    UnitTeamChanged {
        previous_team_id: u32,
        new_team_id: u32,
    },
    ShieldCreated {
        team_id: u32,
        source_kind: ShieldSourceKind,
        position: QVec3,
    },
    ShieldDestroyed {
        position: QVec3,
        reason: ShieldDestroyedReason,
    },
    TerrainCreated {
        team_id: Option<u32>,
        terrain_type: TerrainType,
        position: QVec3,
        radius: i64,
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
        }
    }
}
