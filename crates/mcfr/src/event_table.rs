//! `events.parquet`: every event of the recording, one row each.
//!
//! The columns an event of any kind has come first; each field a payload can
//! carry has a column of its own, null on the rows of every kind that does not
//! carry it. A row holds exactly the fields of its kind, which the reader
//! checks column by column, as it checked the keys of the JSON lines before.

use std::sync::Arc;

use arrow_array::{
    Array, ArrayRef, BooleanArray, Int32Array, Int64Array, PrimitiveArray, RecordBatch,
    StructArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array, types::ArrowPrimitiveType,
};
use arrow_buffer::NullBuffer;
use arrow_schema::{DataType, Field, Fields, Schema, SchemaRef};

use crate::{
    Error, Event, EventKind, EventPayload, ObjectRef, QVec3, Result, ShieldDestroyedReason,
    TerrainRemovedReason,
    parquet_storage::{
        decode_kind as decode_object_kind, decode_shield_source, decode_terrain_type,
        encode_kind as encode_object_kind, encode_shield_source, encode_terrain_type,
    },
};

pub(crate) fn event_schema() -> SchemaRef {
    let object_ref = || {
        Fields::from(vec![
            Field::new("kind", DataType::UInt8, false),
            Field::new("id", DataType::UInt64, false),
        ])
    };
    let vec3 = Fields::from(vec![
        Field::new("x", DataType::Int64, false),
        Field::new("y", DataType::Int64, false),
        Field::new("z", DataType::Int64, false),
    ]);
    Arc::new(Schema::new(vec![
        Field::new("tick", DataType::UInt32, false),
        Field::new("ordinal", DataType::UInt32, false),
        Field::new("type", DataType::UInt8, false),
        Field::new("object", DataType::Struct(object_ref()), true),
        Field::new("source", DataType::Struct(object_ref()), true),
        Field::new("source_team_id", DataType::UInt32, true),
        Field::new("target", DataType::Struct(object_ref()), true),
        Field::new("skill_slot", DataType::UInt16, true),
        Field::new("weapon_index", DataType::Int32, true),
        Field::new("position", DataType::Struct(vec3), true),
        Field::new("intercepted", DataType::Boolean, true),
        Field::new("absorbed_by", DataType::Struct(object_ref()), true),
        Field::new("amount", DataType::Int32, true),
        Field::new("team_id", DataType::UInt32, true),
        Field::new("formation_id", DataType::UInt64, true),
        Field::new("unit_type_id", DataType::UInt32, true),
        Field::new("previous_team_id", DataType::UInt32, true),
        Field::new("new_team_id", DataType::UInt32, true),
        Field::new("source_kind", DataType::UInt8, true),
        Field::new("reason", DataType::UInt8, true),
        Field::new("terrain_type", DataType::UInt8, true),
        Field::new("radius", DataType::Int64, true),
    ]))
}

/// The payload fields of one event, each `None` unless its kind carries it.
#[derive(Default)]
struct Payload {
    skill_slot: Option<u16>,
    weapon_index: Option<i32>,
    position: Option<QVec3>,
    intercepted: Option<bool>,
    absorbed_by: Option<ObjectRef>,
    amount: Option<i32>,
    team_id: Option<u32>,
    formation_id: Option<u64>,
    unit_type_id: Option<u32>,
    previous_team_id: Option<u32>,
    new_team_id: Option<u32>,
    source_kind: Option<u8>,
    reason: Option<u8>,
    terrain_type: Option<u8>,
    radius: Option<i64>,
}

impl Payload {
    fn of(payload: &EventPayload) -> Self {
        let mut flat = Self::default();
        match *payload {
            EventPayload::ProjectileReleased {
                skill_slot,
                weapon_index,
            } => {
                flat.skill_slot = skill_slot;
                flat.weapon_index = weapon_index;
            }
            EventPayload::ProjectileRemoved {
                position,
                intercepted,
                absorbed_by,
            } => {
                flat.position = Some(position);
                flat.intercepted = Some(intercepted);
                flat.absorbed_by = absorbed_by;
            }
            EventPayload::Damage { amount, skill_slot } => {
                flat.amount = Some(amount);
                flat.skill_slot = skill_slot;
            }
            EventPayload::Healing { amount } => {
                flat.amount = Some(amount);
            }
            EventPayload::UnitCreated {
                team_id,
                formation_id,
                unit_type_id,
                position,
            } => {
                flat.team_id = Some(team_id);
                flat.formation_id = Some(formation_id);
                flat.unit_type_id = Some(unit_type_id);
                flat.position = Some(position);
            }
            EventPayload::UnitDied { position }
            | EventPayload::BuildingDestroyed { position }
            | EventPayload::TerrainConverted { position } => flat.position = Some(position),
            EventPayload::UnitTeamChanged {
                previous_team_id,
                new_team_id,
            } => {
                flat.previous_team_id = Some(previous_team_id);
                flat.new_team_id = Some(new_team_id);
            }
            EventPayload::ShieldCreated {
                team_id,
                source_kind,
                position,
            } => {
                flat.team_id = Some(team_id);
                flat.source_kind = Some(encode_shield_source(source_kind));
                flat.position = Some(position);
            }
            EventPayload::ShieldDestroyed { position, reason } => {
                flat.position = Some(position);
                flat.reason = Some(encode_shield_reason(reason));
            }
            EventPayload::TerrainCreated {
                team_id,
                terrain_type,
                position,
                radius,
            } => {
                flat.team_id = team_id;
                flat.terrain_type = Some(encode_terrain_type(terrain_type));
                flat.position = Some(position);
                flat.radius = Some(radius);
            }
            EventPayload::TerrainRemoved { position, reason } => {
                flat.position = Some(position);
                flat.reason = Some(encode_terrain_reason(reason));
            }
        }
        flat
    }

    /// The payload of `kind`, taking the fields it carries and refusing any
    /// other field that is set.
    fn into_payload(mut self, kind: EventKind) -> Result<EventPayload> {
        let label = kind_name(kind);
        let payload = match kind {
            EventKind::ProjectileReleased => EventPayload::ProjectileReleased {
                skill_slot: self.skill_slot.take(),
                weapon_index: self.weapon_index.take(),
            },
            EventKind::ProjectileRemoved => EventPayload::ProjectileRemoved {
                position: required(self.position.take(), label, "position")?,
                intercepted: required(self.intercepted.take(), label, "intercepted")?,
                absorbed_by: self.absorbed_by.take(),
            },
            EventKind::Damage => EventPayload::Damage {
                amount: required(self.amount.take(), label, "amount")?,
                skill_slot: self.skill_slot.take(),
            },
            EventKind::Healing => EventPayload::Healing {
                amount: required(self.amount.take(), label, "amount")?,
            },
            EventKind::UnitCreated => EventPayload::UnitCreated {
                team_id: required(self.team_id.take(), label, "team_id")?,
                formation_id: required(self.formation_id.take(), label, "formation_id")?,
                unit_type_id: required(self.unit_type_id.take(), label, "unit_type_id")?,
                position: required(self.position.take(), label, "position")?,
            },
            EventKind::UnitDied => EventPayload::UnitDied {
                position: required(self.position.take(), label, "position")?,
            },
            EventKind::BuildingDestroyed => EventPayload::BuildingDestroyed {
                position: required(self.position.take(), label, "position")?,
            },
            EventKind::TerrainConverted => EventPayload::TerrainConverted {
                position: required(self.position.take(), label, "position")?,
            },
            EventKind::UnitTeamChanged => EventPayload::UnitTeamChanged {
                previous_team_id: required(
                    self.previous_team_id.take(),
                    label,
                    "previous_team_id",
                )?,
                new_team_id: required(self.new_team_id.take(), label, "new_team_id")?,
            },
            EventKind::ShieldCreated => EventPayload::ShieldCreated {
                team_id: required(self.team_id.take(), label, "team_id")?,
                source_kind: decode_shield_source(required(
                    self.source_kind.take(),
                    label,
                    "source_kind",
                )?)?,
                position: required(self.position.take(), label, "position")?,
            },
            EventKind::ShieldDestroyed => EventPayload::ShieldDestroyed {
                position: required(self.position.take(), label, "position")?,
                reason: decode_shield_reason(required(self.reason.take(), label, "reason")?)?,
            },
            EventKind::TerrainCreated => EventPayload::TerrainCreated {
                team_id: self.team_id.take(),
                terrain_type: decode_terrain_type(required(
                    self.terrain_type.take(),
                    label,
                    "terrain_type",
                )?)?,
                position: required(self.position.take(), label, "position")?,
                radius: required(self.radius.take(), label, "radius")?,
            },
            EventKind::TerrainRemoved => EventPayload::TerrainRemoved {
                position: required(self.position.take(), label, "position")?,
                reason: decode_terrain_reason(required(self.reason.take(), label, "reason")?)?,
            },
        };
        let stray = [
            ("skill_slot", self.skill_slot.is_some()),
            ("weapon_index", self.weapon_index.is_some()),
            ("position", self.position.is_some()),
            ("intercepted", self.intercepted.is_some()),
            ("absorbed_by", self.absorbed_by.is_some()),
            ("amount", self.amount.is_some()),
            ("team_id", self.team_id.is_some()),
            ("formation_id", self.formation_id.is_some()),
            ("unit_type_id", self.unit_type_id.is_some()),
            ("previous_team_id", self.previous_team_id.is_some()),
            ("new_team_id", self.new_team_id.is_some()),
            ("source_kind", self.source_kind.is_some()),
            ("reason", self.reason.is_some()),
            ("terrain_type", self.terrain_type.is_some()),
            ("radius", self.radius.is_some()),
        ];
        if let Some((field, _)) = stray.iter().find(|(_, set)| *set) {
            return Err(Error::invalid(format!(
                "a {label} event does not carry {field}"
            )));
        }
        Ok(payload)
    }
}

fn required<T>(value: Option<T>, kind: &str, field: &str) -> Result<T> {
    value.ok_or_else(|| Error::invalid(format!("a {kind} event requires {field}")))
}

pub(crate) fn event_batch(rows: &[(u32, u32, Event)]) -> Result<Option<RecordBatch>> {
    if rows.is_empty() {
        return Ok(None);
    }
    let payloads = rows
        .iter()
        .map(|(_, _, event)| Payload::of(&event.payload))
        .collect::<Vec<_>>();
    let refs = |pick: fn(&Event) -> Option<ObjectRef>| {
        object_refs(rows.iter().map(|(_, _, event)| pick(event)))
    };
    let columns: Vec<ArrayRef> = vec![
        Arc::new(UInt32Array::from_iter_values(rows.iter().map(|row| row.0))),
        Arc::new(UInt32Array::from_iter_values(rows.iter().map(|row| row.1))),
        Arc::new(UInt8Array::from_iter_values(
            rows.iter()
                .map(|(_, _, event)| encode_kind_tag(event.payload.kind())),
        )),
        refs(|event| event.subject),
        refs(|event| event.source),
        Arc::new(UInt32Array::from_iter(
            rows.iter().map(|(_, _, event)| event.source_team_id),
        )),
        refs(|event| event.target),
        Arc::new(UInt16Array::from_iter(
            payloads.iter().map(|p| p.skill_slot),
        )),
        Arc::new(Int32Array::from_iter(
            payloads.iter().map(|p| p.weapon_index),
        )),
        vec3s(payloads.iter().map(|p| p.position)),
        Arc::new(BooleanArray::from_iter(
            payloads.iter().map(|p| p.intercepted),
        )),
        object_refs(payloads.iter().map(|p| p.absorbed_by)),
        Arc::new(Int32Array::from_iter(payloads.iter().map(|p| p.amount))),
        Arc::new(UInt32Array::from_iter(payloads.iter().map(|p| p.team_id))),
        Arc::new(UInt64Array::from_iter(
            payloads.iter().map(|p| p.formation_id),
        )),
        Arc::new(UInt32Array::from_iter(
            payloads.iter().map(|p| p.unit_type_id),
        )),
        Arc::new(UInt32Array::from_iter(
            payloads.iter().map(|p| p.previous_team_id),
        )),
        Arc::new(UInt32Array::from_iter(
            payloads.iter().map(|p| p.new_team_id),
        )),
        Arc::new(UInt8Array::from_iter(
            payloads.iter().map(|p| p.source_kind),
        )),
        Arc::new(UInt8Array::from_iter(payloads.iter().map(|p| p.reason))),
        Arc::new(UInt8Array::from_iter(
            payloads.iter().map(|p| p.terrain_type),
        )),
        Arc::new(Int64Array::from_iter(payloads.iter().map(|p| p.radius))),
    ];
    Ok(Some(RecordBatch::try_new(event_schema(), columns)?))
}

/// Every row of one stored batch, as `(tick, ordinal, event)`.
pub(crate) fn batch_events(batch: &RecordBatch) -> Result<Vec<(u32, u32, Event)>> {
    let tick = primitive::<UInt32Array>(batch, "tick")?;
    let ordinal = primitive::<UInt32Array>(batch, "ordinal")?;
    let kind = primitive::<UInt8Array>(batch, "type")?;
    let object = structs(batch, "object")?;
    let source = structs(batch, "source")?;
    let source_team_id = primitive::<UInt32Array>(batch, "source_team_id")?;
    let target = structs(batch, "target")?;
    let skill_slot = primitive::<UInt16Array>(batch, "skill_slot")?;
    let weapon_index = primitive::<Int32Array>(batch, "weapon_index")?;
    let position = structs(batch, "position")?;
    let intercepted = primitive::<BooleanArray>(batch, "intercepted")?;
    let absorbed_by = structs(batch, "absorbed_by")?;
    let amount = primitive::<Int32Array>(batch, "amount")?;
    let team_id = primitive::<UInt32Array>(batch, "team_id")?;
    let formation_id = primitive::<UInt64Array>(batch, "formation_id")?;
    let unit_type_id = primitive::<UInt32Array>(batch, "unit_type_id")?;
    let previous_team_id = primitive::<UInt32Array>(batch, "previous_team_id")?;
    let new_team_id = primitive::<UInt32Array>(batch, "new_team_id")?;
    let source_kind = primitive::<UInt8Array>(batch, "source_kind")?;
    let reason = primitive::<UInt8Array>(batch, "reason")?;
    let terrain_type = primitive::<UInt8Array>(batch, "terrain_type")?;
    let radius = primitive::<Int64Array>(batch, "radius")?;
    let mut rows = Vec::with_capacity(batch.num_rows());
    for index in 0..batch.num_rows() {
        if tick.is_null(index) || ordinal.is_null(index) || kind.is_null(index) {
            return Err(Error::invalid(
                "events.parquet tick, ordinal and type are required",
            ));
        }
        let payload = Payload {
            skill_slot: value(skill_slot, index),
            weapon_index: value(weapon_index, index),
            position: vec3(position, index)?,
            intercepted: intercepted
                .is_valid(index)
                .then(|| intercepted.value(index)),
            absorbed_by: object_ref(absorbed_by, index)?,
            amount: value(amount, index),
            team_id: value(team_id, index),
            formation_id: value(formation_id, index),
            unit_type_id: value(unit_type_id, index),
            previous_team_id: value(previous_team_id, index),
            new_team_id: value(new_team_id, index),
            source_kind: value(source_kind, index),
            reason: value(reason, index),
            terrain_type: value(terrain_type, index),
            radius: value(radius, index),
        }
        .into_payload(decode_kind_tag(kind.value(index))?)?;
        rows.push((
            tick.value(index),
            ordinal.value(index),
            Event {
                subject: object_ref(object, index)?,
                source: object_ref(source, index)?,
                source_team_id: value(source_team_id, index),
                target: object_ref(target, index)?,
                payload,
            },
        ));
    }
    Ok(rows)
}

fn object_refs(values: impl IntoIterator<Item = Option<ObjectRef>>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    let fields = Fields::from(vec![
        Field::new("kind", DataType::UInt8, false),
        Field::new("id", DataType::UInt64, false),
    ]);
    Arc::new(StructArray::new(
        fields,
        vec![
            Arc::new(UInt8Array::from_iter_values(values.iter().map(|value| {
                value.map_or(0, |value| encode_object_kind(value.kind))
            }))),
            Arc::new(UInt64Array::from_iter_values(
                values.iter().map(|value| value.map_or(0, |value| value.id)),
            )),
        ],
        Some(values.iter().map(Option::is_some).collect::<NullBuffer>()),
    ))
}

fn vec3s(values: impl IntoIterator<Item = Option<QVec3>>) -> ArrayRef {
    let values = values.into_iter().collect::<Vec<_>>();
    let fields = Fields::from(vec![
        Field::new("x", DataType::Int64, false),
        Field::new("y", DataType::Int64, false),
        Field::new("z", DataType::Int64, false),
    ]);
    let axis = |pick: fn(QVec3) -> i64| -> ArrayRef {
        Arc::new(Int64Array::from_iter_values(
            values.iter().map(|value| value.map_or(0, pick)),
        ))
    };
    Arc::new(StructArray::new(
        fields,
        vec![axis(|v| v.x), axis(|v| v.y), axis(|v| v.z)],
        Some(values.iter().map(Option::is_some).collect::<NullBuffer>()),
    ))
}

fn primitive<'a, T: Array + 'static>(batch: &'a RecordBatch, name: &str) -> Result<&'a T> {
    batch
        .column_by_name(name)
        .ok_or_else(|| Error::invalid(format!("events.parquet lacks {name}")))?
        .as_any()
        .downcast_ref::<T>()
        .ok_or_else(|| Error::invalid(format!("events.parquet {name} has the wrong type")))
}

fn structs<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a StructArray> {
    primitive::<StructArray>(batch, name)
}

fn value<T: ArrowPrimitiveType>(array: &PrimitiveArray<T>, index: usize) -> Option<T::Native> {
    array.is_valid(index).then(|| array.value(index))
}

fn child<'a, T: Array + 'static>(array: &'a StructArray, name: &str) -> Result<&'a T> {
    array
        .column_by_name(name)
        .and_then(|column| column.as_any().downcast_ref::<T>())
        .ok_or_else(|| Error::invalid(format!("events.parquet struct field {name} is missing")))
}

fn object_ref(array: &StructArray, index: usize) -> Result<Option<ObjectRef>> {
    if array.is_null(index) {
        return Ok(None);
    }
    Ok(Some(ObjectRef::new(
        decode_object_kind(child::<UInt8Array>(array, "kind")?.value(index))?,
        child::<UInt64Array>(array, "id")?.value(index),
    )))
}

fn vec3(array: &StructArray, index: usize) -> Result<Option<QVec3>> {
    if array.is_null(index) {
        return Ok(None);
    }
    Ok(Some(QVec3 {
        x: child::<Int64Array>(array, "x")?.value(index),
        y: child::<Int64Array>(array, "y")?.value(index),
        z: child::<Int64Array>(array, "z")?.value(index),
    }))
}

const KINDS: [EventKind; 13] = [
    EventKind::ProjectileReleased,
    EventKind::ProjectileRemoved,
    EventKind::Damage,
    EventKind::UnitCreated,
    EventKind::UnitDied,
    EventKind::BuildingDestroyed,
    EventKind::UnitTeamChanged,
    EventKind::ShieldCreated,
    EventKind::ShieldDestroyed,
    EventKind::TerrainCreated,
    EventKind::TerrainRemoved,
    EventKind::TerrainConverted,
    EventKind::Healing,
];

fn encode_kind_tag(kind: EventKind) -> u8 {
    KINDS
        .iter()
        .position(|candidate| *candidate == kind)
        .and_then(|index| u8::try_from(index).ok())
        .expect("every event kind has a tag")
}

fn decode_kind_tag(tag: u8) -> Result<EventKind> {
    KINDS
        .get(usize::from(tag))
        .copied()
        .ok_or_else(|| Error::invalid(format!("invalid event type tag {tag}")))
}

fn kind_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::ProjectileReleased => "projectile_released",
        EventKind::ProjectileRemoved => "projectile_removed",
        EventKind::Damage => "damage",
        EventKind::UnitCreated => "unit_created",
        EventKind::UnitDied => "unit_died",
        EventKind::BuildingDestroyed => "building_destroyed",
        EventKind::UnitTeamChanged => "unit_team_changed",
        EventKind::ShieldCreated => "shield_created",
        EventKind::ShieldDestroyed => "shield_destroyed",
        EventKind::TerrainCreated => "terrain_created",
        EventKind::TerrainRemoved => "terrain_removed",
        EventKind::TerrainConverted => "terrain_converted",
        EventKind::Healing => "healing",
    }
}

const fn encode_shield_reason(reason: ShieldDestroyedReason) -> u8 {
    match reason {
        ShieldDestroyedReason::EnergyDepleted => 0,
        ShieldDestroyedReason::OwnerDestroyed => 1,
        ShieldDestroyedReason::RoundEnd => 2,
        ShieldDestroyedReason::Scripted => 3,
        ShieldDestroyedReason::Unknown => 4,
    }
}

fn decode_shield_reason(tag: u8) -> Result<ShieldDestroyedReason> {
    match tag {
        0 => Ok(ShieldDestroyedReason::EnergyDepleted),
        1 => Ok(ShieldDestroyedReason::OwnerDestroyed),
        2 => Ok(ShieldDestroyedReason::RoundEnd),
        3 => Ok(ShieldDestroyedReason::Scripted),
        4 => Ok(ShieldDestroyedReason::Unknown),
        _ => Err(Error::invalid(format!(
            "invalid ShieldDestroyedReason tag {tag}"
        ))),
    }
}

const fn encode_terrain_reason(reason: TerrainRemovedReason) -> u8 {
    match reason {
        TerrainRemovedReason::TimeExpired => 0,
        TerrainRemovedReason::RoundExpired => 1,
        TerrainRemovedReason::GridDepleted => 2,
        TerrainRemovedReason::Cleared => 3,
        TerrainRemovedReason::Unknown => 4,
    }
}

fn decode_terrain_reason(tag: u8) -> Result<TerrainRemovedReason> {
    match tag {
        0 => Ok(TerrainRemovedReason::TimeExpired),
        1 => Ok(TerrainRemovedReason::RoundExpired),
        2 => Ok(TerrainRemovedReason::GridDepleted),
        3 => Ok(TerrainRemovedReason::Cleared),
        4 => Ok(TerrainRemovedReason::Unknown),
        _ => Err(Error::invalid(format!(
            "invalid TerrainRemovedReason tag {tag}"
        ))),
    }
}
