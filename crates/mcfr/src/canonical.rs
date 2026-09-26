use serde::{Serialize, de::DeserializeOwned};

use crate::{
    Domain, DurableContext, Error, EventPayload, ObjectKind, ObjectRef, QPose, QVec3, Result,
    TerrainGridState, TerrainType, TransitionEvents, WorldSnapshot,
};

pub(crate) const HASH_BYTES: usize = 32;

/// The canonical bytes of `value`: its JSON with every object's keys in byte
/// order. `serde_json` keeps a `Value`'s object as a `BTreeMap` unless its
/// `preserve_order` feature is on, so the keys come out sorted as they are
/// serialized; the test below holds that.
pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&serde_json::to_value(value)?)?)
}

pub(crate) fn decode<T: DeserializeOwned + Serialize>(bytes: &[u8], label: &str) -> Result<T> {
    let value: T = serde_json::from_slice(bytes)?;
    if encode(&value)? != bytes {
        return Err(Error::invalid(format!(
            "{label} is not encoded in canonical form"
        )));
    }
    Ok(value)
}

pub(crate) struct CanonicalHasher(blake3::Hasher);

impl CanonicalHasher {
    pub(crate) fn new(domain: &str) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"mechcore.mcfr.canonical\0");
        feed(&mut hasher, domain.as_bytes());
        Self(hasher)
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        feed(&mut self.0, bytes);
    }

    pub(crate) fn finalize(self) -> [u8; HASH_BYTES] {
        *self.0.finalize().as_bytes()
    }
}

fn feed(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

pub(crate) fn content_tick_hash(tick: u32, state: &[u8], events: &[u8]) -> [u8; HASH_BYTES] {
    let mut hasher = CanonicalHasher::new("content-tick-0.7.0");
    hasher.update(&tick.to_le_bytes());
    hasher.update(state);
    hasher.update(events);
    hasher.finalize()
}

pub(crate) fn physics_result_hash(tick_hashes: &[[u8; HASH_BYTES]]) -> [u8; HASH_BYTES] {
    result_hash("battle-physics-result-v1", tick_hashes)
}

pub(crate) fn content_result_hash(tick_hashes: &[[u8; HASH_BYTES]]) -> [u8; HASH_BYTES] {
    result_hash("content-result-0.7.0", tick_hashes)
}

fn result_hash(domain: &str, tick_hashes: &[[u8; HASH_BYTES]]) -> [u8; HASH_BYTES] {
    let mut hasher = CanonicalHasher::new(domain);
    let tick_count = u32::try_from(tick_hashes.len()).expect("tick hash count exceeds u32");
    hasher.update(&tick_count.to_le_bytes());
    for tick_hash in tick_hashes {
        hasher.update(tick_hash);
    }
    hasher.finalize()
}

pub(crate) fn physics_tick_hash(
    context: &DurableContext,
    tick: u32,
    state: &WorldSnapshot,
    events: &TransitionEvents,
) -> [u8; HASH_BYTES] {
    let kinematics = kinematics_hash(state);
    let vitals = vitals_hash(state);
    let interactions = interactions_hash(events);
    let divisor = gcd(context.logic_step.numerator, context.logic_step.denominator);
    let mut hasher = PhysicsHasher::new("battle-physics-tick-v1");
    hasher.u32(context.logic_step.numerator / divisor);
    hasher.u32(context.logic_step.denominator / divisor);
    hasher.u32(context.time_units_per_second);
    hasher.u32(tick);
    hasher.bytes(&kinematics);
    hasher.bytes(&vitals);
    hasher.bytes(&interactions);
    hasher.finish()
}

fn kinematics_hash(state: &WorldSnapshot) -> [u8; HASH_BYTES] {
    let mut hasher = PhysicsHasher::new("battle-physics-kinematics-v2");
    hasher.len(state.live_units.len());
    for unit in &state.live_units {
        hasher.u64(unit.unit_id);
        hasher.qvec3(unit.position);
        hasher.angle(unit.body_rotation);
        hasher.optional_angle(unit.turret_rotation);
        hasher.qvec3(unit.velocity);
        hasher.len(
            unit.weapon_aims
                .iter()
                .filter(|weapon| weapon.pose.is_some())
                .count(),
        );
        for weapon in &unit.weapon_aims {
            if let Some(pose) = weapon.pose {
                hasher.u16(weapon.skill_slot);
                hasher.i32(weapon.weapon_index);
                hasher.qpose(pose);
            }
        }
    }
    hasher.len(state.projectiles.len());
    for projectile in &state.projectiles {
        hasher.u64(projectile.projectile_id);
        hasher.qvec3(projectile.position);
        hasher.angle(projectile.orientation);
    }
    hasher.len(state.buildings.len());
    for building in &state.buildings {
        hasher.u64(building.building_id);
        hasher.qvec3(building.position);
    }
    hasher.len(state.shields.len());
    for shield in &state.shields {
        hasher.u64(shield.shield_id);
        hasher.qvec3(shield.position);
        hasher.i64(shield.radius);
    }
    hasher.len(state.terrains.len());
    for terrain in &state.terrains {
        hasher.u64(terrain.terrain_id);
        hasher.qvec3(terrain.position);
        hasher.i64(terrain.radius);
        hasher.optional_grid(terrain.grid.as_ref());
    }
    hasher.finish()
}

fn vitals_hash(state: &WorldSnapshot) -> [u8; HASH_BYTES] {
    let mut hasher = PhysicsHasher::new("battle-physics-vitals-v1");
    hasher.len(state.live_units.len());
    for unit in &state.live_units {
        hasher.u64(unit.unit_id);
        hasher.u32(unit.unit_type_id);
        hasher.u32(unit.team_id);
        hasher.u8(domain_tag(unit.domain));
        hasher.i64(unit.collision_radius);
        hasher.i32(unit.life.current);
        hasher.i32(unit.life.maximum);
        hasher.boolean(unit.personal_shield.active);
        hasher.i32(unit.personal_shield.energy.current);
        hasher.i32(unit.personal_shield.energy.maximum);
    }
    hasher.len(state.projectiles.len());
    for projectile in &state.projectiles {
        hasher.u64(projectile.projectile_id);
        hasher.u32(projectile.team_id);
        hasher.optional_ref(projectile.owner);
        hasher.boolean(projectile.released);
        hasher.i32(projectile.life.current);
        hasher.i32(projectile.life.maximum);
    }
    hasher.len(state.buildings.len());
    for building in &state.buildings {
        hasher.u64(building.building_id);
        hasher.u32(building.building_type_id);
        hasher.u32(building.team_id);
        hasher.i64(building.bounds_width);
        hasher.i64(building.bounds_height);
        hasher.i32(building.life.current);
        hasher.i32(building.life.maximum);
        hasher.boolean(building.available);
        hasher.boolean(building.targetable);
        hasher.boolean(building.collision_enabled);
    }
    hasher.len(state.shields.len());
    for shield in &state.shields {
        hasher.u64(shield.shield_id);
        hasher.u32(shield.team_id);
        hasher.optional_ref(shield.owner);
        hasher.i64(shield.radius);
        hasher.i32(shield.energy.current);
        hasher.i32(shield.energy.maximum);
        hasher.boolean(shield.active);
    }
    hasher.len(state.terrains.len());
    for terrain in &state.terrains {
        hasher.u64(terrain.terrain_id);
        hasher.optional_u32(terrain.team_id);
        hasher.u8(terrain_type_tag(terrain.terrain_type));
        hasher.i64(terrain.radius);
    }
    hasher.finish()
}

#[allow(clippy::too_many_lines)]
fn interactions_hash(events: &TransitionEvents) -> [u8; HASH_BYTES] {
    let mut hasher = PhysicsHasher::new("battle-physics-interactions-v1");
    hasher.len(events.events.len());
    for (ordinal, event) in events.events.iter().enumerate() {
        hasher.len(ordinal);
        hasher.optional_ref(event.subject);
        hasher.optional_ref(event.source);
        hasher.optional_u32(event.source_team_id);
        hasher.optional_ref(event.target);
        match &event.payload {
            EventPayload::ProjectileReleased {
                skill_slot,
                weapon_index,
            } => {
                hasher.u8(0);
                hasher.optional_u16(*skill_slot);
                hasher.optional_i32(*weapon_index);
            }
            EventPayload::ProjectileRemoved {
                position,
                intercepted,
                absorbed_by,
            } => {
                hasher.u8(1);
                hasher.qvec3(*position);
                hasher.boolean(*intercepted);
                hasher.optional_ref(*absorbed_by);
            }
            EventPayload::Damage { amount } => {
                hasher.u8(2);
                hasher.i32(*amount);
            }
            EventPayload::UnitCreated {
                team_id,
                formation_id: _,
                unit_type_id,
                position,
            } => {
                hasher.u8(3);
                hasher.u32(*team_id);
                hasher.u32(*unit_type_id);
                hasher.qvec3(*position);
            }
            EventPayload::UnitDied { position } => {
                hasher.u8(4);
                hasher.qvec3(*position);
            }
            EventPayload::BuildingDestroyed { position } => {
                hasher.u8(5);
                hasher.qvec3(*position);
            }
            EventPayload::UnitTeamChanged {
                previous_team_id,
                new_team_id,
            } => {
                hasher.u8(6);
                hasher.u32(*previous_team_id);
                hasher.u32(*new_team_id);
            }
            EventPayload::ShieldCreated {
                team_id,
                source_kind: _,
                position,
            } => {
                hasher.u8(7);
                hasher.u32(*team_id);
                hasher.qvec3(*position);
            }
            EventPayload::ShieldDestroyed {
                position,
                reason: _,
            } => {
                hasher.u8(8);
                hasher.qvec3(*position);
            }
            EventPayload::TerrainCreated {
                team_id,
                terrain_type,
                position,
                radius,
            } => {
                hasher.u8(9);
                hasher.optional_u32(*team_id);
                hasher.u8(terrain_type_tag(*terrain_type));
                hasher.qvec3(*position);
                hasher.i64(*radius);
            }
            EventPayload::TerrainRemoved {
                position,
                reason: _,
            } => {
                hasher.u8(10);
                hasher.qvec3(*position);
            }
            EventPayload::TerrainConverted { position } => {
                hasher.u8(11);
                hasher.qvec3(*position);
            }
            EventPayload::Healing { amount } => {
                hasher.u8(12);
                hasher.i32(*amount);
            }
        }
    }
    hasher.finish()
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

const fn domain_tag(domain: Domain) -> u8 {
    match domain {
        Domain::Ground => 0,
        Domain::Air => 1,
    }
}

const fn object_kind_tag(kind: ObjectKind) -> u8 {
    match kind {
        ObjectKind::Unit => 0,
        ObjectKind::Projectile => 1,
        ObjectKind::Building => 2,
        ObjectKind::Shield => 3,
        ObjectKind::Terrain => 4,
    }
}

const fn terrain_type_tag(terrain_type: TerrainType) -> u8 {
    match terrain_type {
        TerrainType::Fire => 0,
        TerrainType::Oil => 1,
        TerrainType::Fog => 2,
        TerrainType::Acid => 3,
        TerrainType::RecoveryZone => 4,
        TerrainType::FogSand => 5,
    }
}

struct PhysicsHasher(CanonicalHasher);

impl PhysicsHasher {
    fn new(domain: &str) -> Self {
        Self(CanonicalHasher::new(domain))
    }

    fn finish(self) -> [u8; HASH_BYTES] {
        self.0.finalize()
    }

    fn bytes(&mut self, value: &[u8]) {
        self.0.update(value);
    }

    fn boolean(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.bytes(&value.to_le_bytes());
    }

    fn len(&mut self, value: usize) {
        self.u64(u64::try_from(value).expect("physics hash list length exceeds u64"));
    }

    fn angle(&mut self, value: i64) {
        self.i64(value.rem_euclid(360_i64 << 32));
    }

    fn qvec3(&mut self, value: QVec3) {
        self.i64(value.x);
        self.i64(value.y);
        self.i64(value.z);
    }

    fn qpose(&mut self, value: QPose) {
        self.qvec3(value.position);
        self.angle(value.rotation);
    }

    fn object_ref(&mut self, value: ObjectRef) {
        self.u8(object_kind_tag(value.kind));
        self.u64(value.id);
    }

    fn optional_ref(&mut self, value: Option<ObjectRef>) {
        self.boolean(value.is_some());
        if let Some(value) = value {
            self.object_ref(value);
        }
    }

    fn optional_u16(&mut self, value: Option<u16>) {
        self.boolean(value.is_some());
        if let Some(value) = value {
            self.u16(value);
        }
    }

    fn optional_u32(&mut self, value: Option<u32>) {
        self.boolean(value.is_some());
        if let Some(value) = value {
            self.u32(value);
        }
    }

    fn optional_i32(&mut self, value: Option<i32>) {
        self.boolean(value.is_some());
        if let Some(value) = value {
            self.i32(value);
        }
    }

    fn optional_angle(&mut self, value: Option<i64>) {
        self.boolean(value.is_some());
        if let Some(value) = value {
            self.angle(value);
        }
    }

    fn optional_grid(&mut self, value: Option<&TerrainGridState>) {
        self.boolean(value.is_some());
        if let Some(value) = value {
            self.i64(value.origin_x);
            self.i64(value.origin_y);
            self.u32(value.size_x);
            self.u32(value.size_y);
            self.len(value.rows.len());
            for row in &value.rows {
                self.u32(*row);
            }
        }
    }
}

pub(crate) fn hex(bytes: &[u8; HASH_BYTES]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(HASH_BYTES * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

pub(crate) fn parse_hex(value: &str, label: &str) -> Result<[u8; HASH_BYTES]> {
    if value.len() != HASH_BYTES * 2 {
        return Err(Error::invalid(format!("{label} is not a 64-digit hash")));
    }
    let mut output = [0; HASH_BYTES];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        output[index] = (nibble(pair[0], label)? << 4) | nibble(pair[1], label)?;
    }
    Ok(output)
}

fn nibble(value: u8, label: &str) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(Error::invalid(format!(
            "{label} contains a non-canonical hexadecimal digit"
        ))),
    }
}

#[cfg(test)]
mod tests {
    /// A struct's fields come out in byte order, whatever order it declares
    /// them in, which is what makes [`super::encode`] canonical without
    /// sorting anything itself.
    #[test]
    fn a_struct_encodes_its_keys_in_byte_order() {
        #[derive(serde::Serialize)]
        struct Declared {
            zeta: u8,
            alpha: u8,
            #[serde(rename = "Beta")]
            beta: u8,
        }
        let declared = Declared {
            zeta: 1,
            alpha: 2,
            beta: 3,
        };
        assert_eq!(
            super::encode(&declared).unwrap(),
            br#"{"Beta":3,"alpha":2,"zeta":1}"#
        );
    }
}
