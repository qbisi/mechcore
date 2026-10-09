//! A recording's members as the Arrow tables they are stored as, for a reader
//! that asks its own questions of them rather than the fight they describe.
//!
//! [`McfrTables`] opens a recording without decoding it: a table is read only
//! when it is asked for, so a question about the events never pays for the
//! instrument channels beside them. What it answers is the stored form, with
//! enum tags as the integers the format writes; [`column_tags`] names them.

use std::{collections::BTreeMap, io::Read, path::Path};

use arrow_array::RecordBatch;
use arrow_schema::{Fields, Schema, SchemaRef};
use parquet::{arrow::arrow_reader::ParquetRecordBatchReaderBuilder, file::reader::ChunkReader};
use serde::Serialize;

use super::{
    ATTACK_PHASES, BUFF_DATA_KINDS, INSTRUMENT_DIRECTORY, MemberSlice, RECORDER_KINDS,
    SKILL_MACHINE_STATES, TABLE_MEMBERS, building_schema, checked_builder, decode_domain,
    decode_kind, decode_motion, decode_shield_round_policy, decode_shield_source,
    decode_terrain_type, decode_visibility, formation_schema, instrument_channel,
    object_ref_fields, open_members, projectile_schema, rebirth_schema, shield_schema,
    statistic_schema, terrain_schema, tick_fields, unit_schema,
};
use crate::{Error, MCFR_FORMAT, Result, event_table};

/// One recording's tables, read on demand.
pub struct McfrTables {
    members: BTreeMap<String, MemberSlice>,
    metadata: BTreeMap<String, String>,
    layout_yaml: String,
}

/// Where a table's rows come from, and whether the content hash reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableOrigin {
    /// A per-tick table the content hash reads.
    Hashed,
    /// An instrument channel, outside the hash.
    Instrument,
}

impl McfrTables {
    /// Opens a recording of the format this crate writes, reading its member
    /// set and `ticks.parquet`'s file metadata and nothing else.
    ///
    /// # Errors
    ///
    /// Returns an error for a file that is not a recording, or one of another
    /// format: its tables are not the ones this crate names.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let members = open_members(path.as_ref())?;
        let ticks = members
            .get("ticks.parquet")
            .cloned()
            .ok_or_else(|| Error::invalid("missing ticks.parquet"))?;
        let metadata = ParquetRecordBatchReaderBuilder::try_new(ticks)?
            .schema()
            .metadata()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        match metadata.get("format") {
            Some(format) if format == MCFR_FORMAT => {}
            Some(format) => {
                return Err(Error::invalid(format!(
                    "unsupported MCFR format {format:?}; this reader reads {MCFR_FORMAT}"
                )));
            }
            None => return Err(Error::invalid("ticks.parquet names no format")),
        }
        let layout = members
            .get("layout.yaml")
            .cloned()
            .ok_or_else(|| Error::invalid("missing layout.yaml"))?;
        let mut bytes = Vec::new();
        layout.get_read(0)?.read_to_end(&mut bytes)?;
        let layout_yaml =
            String::from_utf8(bytes).map_err(|_| Error::invalid("layout.yaml is not UTF-8"))?;
        Ok(Self {
            members,
            metadata,
            layout_yaml,
        })
    }

    /// `ticks.parquet`'s key-value metadata, by key.
    #[must_use]
    pub const fn metadata(&self) -> &BTreeMap<String, String> {
        &self.metadata
    }

    /// The embedded layout, as stored.
    #[must_use]
    pub fn layout_yaml(&self) -> &str {
        &self.layout_yaml
    }

    /// Every table the recording answers, in container order: `ticks`, the
    /// nine per-tick tables whether stored or not, then each instrument
    /// channel present as `instrument/<channel>`.
    #[must_use]
    pub fn tables(&self) -> Vec<String> {
        let mut tables = vec!["ticks".to_owned()];
        tables.extend(
            TABLE_MEMBERS
                .iter()
                .map(|member| member.trim_end_matches(".parquet").to_owned()),
        );
        tables.extend(
            self.members
                .keys()
                .filter_map(|member| instrument_channel(member))
                .map(|channel| format!("{INSTRUMENT_DIRECTORY}/{channel}")),
        );
        tables
    }

    /// Whether the content hash reads a table.
    ///
    /// # Errors
    ///
    /// Returns an error for a table the recording does not answer.
    pub fn origin(&self, table: &str) -> Result<TableOrigin> {
        if self.instrument_member(table).is_some() {
            Ok(TableOrigin::Instrument)
        } else if fixed_schema(table).is_some() {
            Ok(TableOrigin::Hashed)
        } else {
            Err(unknown(table))
        }
    }

    /// A table's Arrow schema. A per-tick table's is the format's own, stored
    /// or not; an instrument channel's is read from its footer.
    ///
    /// # Errors
    ///
    /// Returns an error for a table the recording does not answer, or a
    /// channel whose footer does not read.
    pub fn schema(&self, table: &str) -> Result<SchemaRef> {
        if let Some(member) = self.instrument_member(table) {
            return Ok(ParquetRecordBatchReaderBuilder::try_new(member)?
                .schema()
                .clone());
        }
        fixed_schema(table).ok_or_else(|| unknown(table))
    }

    /// Every row of a table, in stored order. A per-tick table the recording
    /// leaves out has none.
    ///
    /// # Errors
    ///
    /// Returns an error for a table the recording does not answer, or one
    /// whose stored schema is not the format's.
    pub fn batches(&self, table: &str) -> Result<Vec<RecordBatch>> {
        let builder = if let Some(member) = self.instrument_member(table) {
            ParquetRecordBatchReaderBuilder::try_new(member)?
        } else {
            let schema = fixed_schema(table).ok_or_else(|| unknown(table))?;
            let Some(member) = self.members.get(&format!("{table}.parquet")).cloned() else {
                return Ok(Vec::new());
            };
            checked_builder(member, &schema, table)?
        };
        Ok(builder
            .build()?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// A per-tick table's schema, the format's own whether a recording
    /// stores the table or not; `None` for any other name.
    #[must_use]
    pub fn schema_of(table: &str) -> Option<SchemaRef> {
        fixed_schema(table)
    }

    fn instrument_member(&self, table: &str) -> Option<MemberSlice> {
        table
            .strip_prefix(INSTRUMENT_DIRECTORY)?
            .strip_prefix('/')?;
        self.members.get(&format!("{table}.parquet")).cloned()
    }
}

fn unknown(table: &str) -> Error {
    Error::invalid(format!("the recording has no table {table:?}"))
}

fn fixed_schema(table: &str) -> Option<SchemaRef> {
    Some(match table {
        "ticks" => std::sync::Arc::new(Schema::new(tick_fields())),
        "units" => unit_schema(),
        "rebirths" => rebirth_schema(),
        "projectiles" => projectile_schema(),
        "buildings" => building_schema(),
        "shields" => shield_schema(),
        "terrains" => terrain_schema(),
        "statistics" => statistic_schema(),
        "formations" => formation_schema(),
        "events" => event_table::event_schema(),
        _ => return None,
    })
}

/// The names of a per-tick table column's enum tags, by tag, or `None` for a
/// column that holds no enum this crate names.
///
/// `path` is the column's field names from the table down, a list's element
/// standing in for its list; `parent` is the fields of the struct the column
/// sits in, the table's own at the top. An `ObjectRef`'s `kind` names its
/// `ObjectKind` wherever the reference sits. `events.reason` is no single
/// enum, since which one depends on the event's type, and has none.
#[must_use]
pub fn column_tags(table: &str, path: &[&str], parent: &Fields) -> Option<Vec<(u8, String)>> {
    if path.last() == Some(&"kind") && *parent == object_ref_fields() {
        return Some(decoded(decode_kind));
    }
    Some(match (table, path) {
        ("units", ["domain"]) => decoded(decode_domain),
        ("units", ["motion_state"]) => decoded(decode_motion),
        ("units", ["visibility"]) => decoded(decode_visibility),
        ("units", ["buffs", "data", "kind"]) => positional(&BUFF_DATA_KINDS),
        ("units", ["skills", "enabled", "state"]) => positional(&SKILL_MACHINE_STATES),
        ("units", ["skills", "enabled", "attack_phase"]) => positional(&ATTACK_PHASES),
        ("statistics", ["recorder"]) => positional(&RECORDER_KINDS),
        ("shields" | "events", ["source_kind"]) => decoded(decode_shield_source),
        ("shields", ["round_policy"]) => decoded(decode_shield_round_policy),
        ("terrains" | "events", ["terrain_type"]) => decoded(decode_terrain_type),
        ("events", ["type"]) => event_table::KINDS
            .iter()
            .zip(0..)
            .map(|(kind, tag)| (tag, event_table::kind_name(*kind).to_owned()))
            .collect(),
        _ => return None,
    })
}

fn name<E: Serialize>(value: &E) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => unreachable!("a format enum serializes as its name"),
    }
}

fn decoded<E: Serialize>(decode: fn(u8) -> Result<E>) -> Vec<(u8, String)> {
    (0..=u8::MAX)
        .filter_map(|tag| decode(tag).ok().map(|value| (tag, name(&value))))
        .collect()
}

fn positional<E: Serialize>(variants: &[E]) -> Vec<(u8, String)> {
    variants
        .iter()
        .zip(0..)
        .map(|(value, tag)| (tag, name(value)))
        .collect()
}

#[cfg(test)]
mod tests {
    use arrow_schema::DataType;

    use super::*;

    /// Every `UInt8` column of a per-tick table is an enum with named tags,
    /// apart from `events.reason`, which is several.
    #[test]
    fn every_tag_column_is_named() {
        fn walk(table: &str, path: &mut Vec<String>, fields: &Fields, missing: &mut Vec<String>) {
            for field in fields {
                path.push(field.name().clone());
                let mut data_type = field.data_type();
                while let DataType::List(item) = data_type {
                    data_type = item.data_type();
                }
                match data_type {
                    DataType::Struct(children) => walk(table, path, children, missing),
                    DataType::UInt8 => {
                        let parts = path.iter().map(String::as_str).collect::<Vec<_>>();
                        let named =
                            column_tags(table, &parts, fields).is_some_and(|tags| !tags.is_empty());
                        if !named && (table != "events" || parts != ["reason"]) {
                            missing.push(format!("{table}.{}", parts.join(".")));
                        }
                    }
                    _ => {}
                }
                path.pop();
            }
        }
        let mut missing = Vec::new();
        for table in ["ticks"]
            .into_iter()
            .chain(TABLE_MEMBERS.map(|member| member.trim_end_matches(".parquet")))
        {
            let schema = fixed_schema(table).expect("a per-tick table has a schema");
            walk(table, &mut Vec::new(), schema.fields(), &mut missing);
        }
        assert!(missing.is_empty(), "unnamed tag columns: {missing:?}");
    }
}
