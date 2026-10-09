//! One recording member laid out as relational tables, by one rule read off
//! its Arrow schema.
//!
//! - A scalar field is a column, under its own name.
//! - A struct's fields are columns of the row that holds it, named
//!   `<struct>__<field>`: `__` separates a path's steps in a column's name and
//!   a table's, and no field the format names holds it, so a laid-out name is
//!   never another's. A struct that may be null adds `has_<struct>`, so a null
//!   struct is told from one whose fields are null.
//! - A list is a table of its own, `<table>__<list>`, whose rows are its
//!   elements: the key of the row that holds it, `ordinal`, the element's place
//!   in the list from 0, then the element's columns, or `value` for an element
//!   that is a scalar. A deeper list's table names its parent's `ordinal` after
//!   that list, `<list>_ordinal`.
//! - An enum tag is its name, as [`column_tags`] gives it.
//!
//! A member's own table is keyed by `tick` and the identity the format orders
//! its rows by; one with no such identity, an instrument channel, by `row`, its
//! place in the member.

use std::collections::{BTreeMap, BTreeSet};

use arrow_array::{
    Array, ArrayRef, ListArray, RecordBatch, StructArray,
    cast::AsArray,
    types::{
        Float32Type, Float64Type, Int8Type, Int16Type, Int32Type, Int64Type, UInt8Type, UInt16Type,
        UInt32Type, UInt64Type,
    },
};
use arrow_schema::{DataType, Fields, Schema};
use mechcore_mcfr::column_tags;
use rusqlite::{Transaction, types::Value as Sql};

/// One relational table: its columns, which of them key it, and the tables of
/// the lists its rows hold.
pub(crate) struct Table {
    pub(crate) name: String,
    columns: Vec<Column>,
    key: Vec<usize>,
    /// Each list's table, by the path from this table's element to the list.
    children: Vec<(Vec<usize>, Self)>,
}

/// One column, and where its value is read from.
pub(crate) struct Column {
    pub(crate) name: String,
    pub(crate) nullable: bool,
    /// Where it sits in the member: `units.parquet:skills[].enabled.state`.
    pub(crate) path: String,
    data_type: DataType,
    tags: Option<BTreeMap<u8, String>>,
    source: Source,
}

enum Source {
    /// The value of a key column of the row that holds the list.
    Inherited(usize),
    /// The element's place in its list.
    Ordinal,
    /// The row's place in the member.
    Row,
    /// A scalar, by the struct children from the element down; an empty path
    /// is the element itself.
    Field(Vec<usize>),
    /// Whether the struct at this path is present.
    Has(Vec<usize>),
}

impl Column {
    pub(crate) const fn sql_type(&self) -> &'static str {
        if self.tags.is_some() {
            return "TEXT";
        }
        match self.data_type {
            DataType::Float16 | DataType::Float32 | DataType::Float64 => "REAL",
            DataType::Utf8
            | DataType::LargeUtf8
            | DataType::FixedSizeBinary(_)
            | DataType::Binary
            | DataType::LargeBinary => "TEXT",
            _ => "INTEGER",
        }
    }

    pub(crate) fn tag_names(&self) -> Option<Vec<String>> {
        self.tags.as_ref().map(|tags| {
            tags.iter()
                .map(|(tag, name)| format!("{tag}={name}"))
                .collect()
        })
    }
}

impl Table {
    pub(crate) fn columns(&self) -> impl Iterator<Item = &Column> {
        self.columns.iter()
    }

    pub(crate) fn key(&self) -> impl Iterator<Item = &Column> {
        self.key.iter().map(|index| &self.columns[*index])
    }

    pub(crate) fn create(&self) -> String {
        let columns = self
            .columns
            .iter()
            .map(|column| {
                format!(
                    "\"{}\" {}{}",
                    column.name,
                    column.sql_type(),
                    if column.nullable { "" } else { " NOT NULL" }
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("CREATE TABLE \"{}\" ({columns});", self.name)
    }

    pub(crate) fn index(&self) -> String {
        let key = self
            .key()
            .map(|column| format!("\"{}\"", column.name))
            .collect::<Vec<_>>()
            .join(", ");
        format!("CREATE INDEX \"{0}__key\" ON \"{0}\" ({key});", self.name)
    }

    /// The table and every list's table under it, each after the table that
    /// holds it.
    pub(crate) fn all(&self) -> Vec<&Self> {
        let mut tables = vec![self];
        for (_, child) in &self.children {
            tables.extend(child.all());
        }
        tables
    }
}

/// The SQL name of a member's own table: `instrument/rvo_vo` is
/// `instrument_rvo_vo`.
fn table_name(member: &str) -> String {
    member.replace('/', "_")
}

/// The columns a member's own table is keyed by, after `tick`.
fn identity(member: &str) -> Option<&'static [&'static str]> {
    Some(match member {
        "ticks" => &[],
        "units" | "rebirths" => &["unit_id"],
        "projectiles" => &["projectile_id"],
        "buildings" => &["building_id"],
        "shields" => &["shield_id"],
        "terrains" => &["terrain_id"],
        "statistics" => &["team_id", "recorder", "recorder_id"],
        "formations" => &["formation_id"],
        "events" => &["ordinal"],
        _ => return None,
    })
}

/// A member's own table, holding the tables of its lists.
pub(crate) fn tables(member: &str, schema: &Schema) -> Result<Table, String> {
    let label = format!("{member}.parquet");
    let mut columns = Vec::new();
    let keyed = identity(member);
    if keyed.is_none() {
        columns.push(Column {
            name: "row".to_owned(),
            nullable: false,
            path: format!("{label}:<row>"),
            data_type: DataType::UInt64,
            tags: None,
            source: Source::Row,
        });
    }
    let mut table = element_table(
        member,
        table_name(member),
        &label,
        columns,
        schema.fields(),
        &Layout::default(),
    )?;
    table.key = match keyed {
        Some(identity) => ["tick"]
            .iter()
            .chain(identity)
            .map(|name| {
                table
                    .columns
                    .iter()
                    .position(|column| column.name == *name)
                    .ok_or_else(|| format!("{label} has no key column {name}"))
            })
            .collect::<Result<_, _>>()?,
        None => vec![0],
    };
    // The key is known only now, so the lists' tables inherit it now.
    for (_, child) in &mut table.children {
        inherit(child, &table.columns, &table.key, None)?;
    }
    Ok(table)
}

/// Where a struct's fields sit as they are laid out: the indices from the
/// element down, the column name's prefix, the field names the enum tags are
/// looked up by, the path shown, and whether a struct above may be null.
#[derive(Default, Clone)]
struct Layout {
    indices: Vec<usize>,
    prefix: String,
    names: Vec<String>,
    shown: String,
    nullable: bool,
}

fn element_table(
    member: &str,
    name: String,
    label: &str,
    mut columns: Vec<Column>,
    fields: &Fields,
    layout: &Layout,
) -> Result<Table, String> {
    let mut children = Vec::new();
    lay_out(
        member,
        &name,
        label,
        fields,
        layout,
        &mut columns,
        &mut children,
    )?;
    let mut seen = BTreeSet::new();
    for column in &columns {
        if !seen.insert(column.name.as_str()) {
            return Err(format!("{name} lays out column {} twice", column.name));
        }
    }
    Ok(Table {
        name,
        columns,
        key: Vec::new(),
        children,
    })
}

fn lay_out(
    member: &str,
    table: &str,
    label: &str,
    fields: &Fields,
    layout: &Layout,
    columns: &mut Vec<Column>,
    children: &mut Vec<(Vec<usize>, Table)>,
) -> Result<(), String> {
    for (index, field) in fields.iter().enumerate() {
        if field.name().contains("__") {
            return Err(format!(
                "{label} names a field {:?}, which holds the path separator",
                field.name()
            ));
        }
        let mut here = layout.clone();
        here.indices.push(index);
        here.names.push(field.name().clone());
        let name = format!("{}{}", layout.prefix, field.name());
        let shown = if layout.shown.is_empty() {
            field.name().clone()
        } else {
            format!("{}.{}", layout.shown, field.name())
        };
        match field.data_type() {
            DataType::Struct(inner) => {
                if field.is_nullable() {
                    columns.push(Column {
                        name: format!("has_{name}"),
                        nullable: layout.nullable,
                        path: format!("{label}:{shown}"),
                        data_type: DataType::Boolean,
                        tags: None,
                        source: Source::Has(here.indices.clone()),
                    });
                }
                here.prefix = format!("{name}__");
                here.shown = shown;
                here.nullable |= field.is_nullable();
                lay_out(member, table, label, inner, &here, columns, children)?;
            }
            DataType::List(item) => {
                let child_name = format!("{table}__{name}");
                let element = Layout {
                    indices: Vec::new(),
                    prefix: String::new(),
                    names: here.names.clone(),
                    shown: format!("{shown}[]"),
                    nullable: false,
                };
                let ordinal = Column {
                    name: "ordinal".to_owned(),
                    nullable: false,
                    path: format!("{label}:{shown}[]#"),
                    data_type: DataType::UInt32,
                    tags: None,
                    source: Source::Ordinal,
                };
                let child = match item.data_type() {
                    DataType::Struct(inner) => {
                        element_table(member, child_name, label, vec![ordinal], inner, &element)?
                    }
                    DataType::List(_) => {
                        return Err(format!("{label}:{shown} is a list of lists"));
                    }
                    scalar => {
                        let names = element.names.iter().map(String::as_str).collect::<Vec<_>>();
                        Table {
                            name: child_name,
                            columns: vec![
                                ordinal,
                                Column {
                                    name: "value".to_owned(),
                                    nullable: item.is_nullable(),
                                    path: format!("{label}:{shown}[]"),
                                    data_type: scalar.clone(),
                                    tags: tags(member, &names, &Fields::empty()),
                                    source: Source::Field(Vec::new()),
                                },
                            ],
                            key: Vec::new(),
                            children: Vec::new(),
                        }
                    }
                };
                children.push((here.indices.clone(), child));
            }
            DataType::LargeList(_) | DataType::FixedSizeList(..) | DataType::Map(..) => {
                return Err(format!(
                    "{label}:{shown} has a type the layout does not take"
                ));
            }
            scalar => {
                let names = here.names.iter().map(String::as_str).collect::<Vec<_>>();
                columns.push(Column {
                    name,
                    nullable: field.is_nullable() || layout.nullable,
                    path: format!("{label}:{shown}"),
                    data_type: scalar.clone(),
                    tags: tags(member, &names, fields),
                    source: Source::Field(here.indices.clone()),
                });
            }
        }
    }
    Ok(())
}

fn tags(member: &str, names: &[&str], parent: &Fields) -> Option<BTreeMap<u8, String>> {
    column_tags(member, names, parent).map(|tags| tags.into_iter().collect())
}

/// Puts the key of the table that holds a list at the front of the list's
/// table, its `ordinal` named after that table's own list, and keys the list's
/// table by it and its own `ordinal`.
fn inherit(
    table: &mut Table,
    parent_columns: &[Column],
    parent_key: &[usize],
    parent_list: Option<&str>,
) -> Result<(), String> {
    let inherited = parent_key
        .iter()
        .enumerate()
        .map(|(at, index)| {
            let column = &parent_columns[*index];
            let name = match (column.name.as_str(), parent_list) {
                ("ordinal", Some(list)) => format!("{list}_ordinal"),
                (name, _) => name.to_owned(),
            };
            Column {
                name,
                nullable: false,
                path: column.path.clone(),
                data_type: column.data_type.clone(),
                tags: column.tags.clone(),
                source: Source::Inherited(at),
            }
        })
        .collect::<Vec<_>>();
    let count = inherited.len();
    let mut columns = inherited;
    columns.append(&mut table.columns);
    table.columns = columns;
    table.key = (0..=count).collect();
    let mut seen = BTreeSet::new();
    for column in &table.columns {
        if !seen.insert(column.name.as_str()) {
            return Err(format!(
                "{} lays out column {} twice",
                table.name, column.name
            ));
        }
    }
    let list = table
        .name
        .rsplit("__")
        .next()
        .map(str::to_owned)
        .unwrap_or_default();
    let (columns, key) = (&table.columns, &table.key);
    for (_, child) in &mut table.children {
        inherit(child, columns, key, Some(&list))?;
    }
    Ok(())
}

/// Inserts a member's rows into the tables of `wanted`, its own table being
/// `table`.
pub(crate) fn insert(
    transaction: &Transaction<'_>,
    table: &Table,
    batches: &[RecordBatch],
    wanted: &BTreeSet<String>,
) -> Result<(), String> {
    let mut statements = BTreeMap::new();
    for each in table.all() {
        if wanted.contains(&each.name) {
            let marks = (1..=each.columns.len())
                .map(|index| format!("?{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            let statement = transaction
                .prepare(&format!("INSERT INTO \"{}\" VALUES ({marks})", each.name))
                .map_err(|error| error.to_string())?;
            statements.insert(each.name.clone(), statement);
        }
    }
    let mut writer = Writer {
        statements,
        wanted,
        row: 0,
    };
    for batch in batches {
        let element: ArrayRef = std::sync::Arc::new(StructArray::from(batch.clone()));
        for index in 0..batch.num_rows() {
            writer.row(table, &element, index, &[], 0)?;
            writer.row += 1;
        }
    }
    Ok(())
}

struct Writer<'t, 'w> {
    statements: BTreeMap<String, rusqlite::Statement<'t>>,
    wanted: &'w BTreeSet<String>,
    row: u64,
}

impl Writer<'_, '_> {
    fn row(
        &mut self,
        table: &Table,
        element: &ArrayRef,
        index: usize,
        inherited: &[Sql],
        ordinal: usize,
    ) -> Result<(), String> {
        let values = table
            .columns
            .iter()
            .map(|column| self.value(column, element, index, inherited, ordinal))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(statement) = self.statements.get_mut(&table.name) {
            statement
                .execute(rusqlite::params_from_iter(&values))
                .map_err(|error| error.to_string())?;
        }
        let key = table
            .key
            .iter()
            .map(|at| values[*at].clone())
            .collect::<Vec<_>>();
        for (path, child) in &table.children {
            if !child
                .all()
                .iter()
                .any(|each| self.wanted.contains(&each.name))
            {
                continue;
            }
            let Some(list) = walk(element, index, path) else {
                continue;
            };
            let list = list
                .as_any()
                .downcast_ref::<ListArray>()
                .ok_or("a list column is not a list")?;
            let offsets = list.value_offsets();
            let (start, end) = (offsets[index], offsets[index + 1]);
            let values = list.values();
            for at in start..end {
                let at = usize::try_from(at).map_err(|_| "a list offset is negative")?;
                let start = usize::try_from(start).map_err(|_| "a list offset is negative")?;
                self.row(child, values, at, &key, at - start)?;
            }
        }
        Ok(())
    }

    fn value(
        &self,
        column: &Column,
        element: &ArrayRef,
        index: usize,
        inherited: &[Sql],
        ordinal: usize,
    ) -> Result<Sql, String> {
        Ok(match &column.source {
            Source::Inherited(at) => inherited[*at].clone(),
            Source::Ordinal => Sql::Integer(i64::try_from(ordinal).map_err(|_| "ordinal")?),
            Source::Row => Sql::Integer(i64::try_from(self.row).map_err(|_| "row")?),
            Source::Has(path) => Sql::Integer(i64::from(walk(element, index, path).is_some())),
            Source::Field(path) => match walk(element, index, path) {
                None => Sql::Null,
                Some(array) => {
                    let value = scalar(&array, index)?;
                    match (&column.tags, value) {
                        (Some(tags), Sql::Integer(tag)) => u8::try_from(tag)
                            .ok()
                            .and_then(|tag| tags.get(&tag))
                            .map_or_else(
                                || Sql::Text(format!("<{tag}>")),
                                |name| Sql::Text(name.clone()),
                            ),
                        (_, value) => value,
                    }
                }
            },
        })
    }
}

/// The array at `path` from `element`, or `None` where it or a struct above
/// it is null at `index`.
fn walk(element: &ArrayRef, index: usize, path: &[usize]) -> Option<ArrayRef> {
    let mut array = element.clone();
    for child in path {
        if array.is_null(index) {
            return None;
        }
        array = array.as_struct().column(*child).clone();
    }
    (!array.is_null(index)).then_some(array)
}

fn scalar(array: &ArrayRef, index: usize) -> Result<Sql, String> {
    let integer = |value: i64| Sql::Integer(value);
    Ok(match array.data_type() {
        DataType::Boolean => integer(i64::from(array.as_boolean().value(index))),
        DataType::Int8 => integer(i64::from(array.as_primitive::<Int8Type>().value(index))),
        DataType::Int16 => integer(i64::from(array.as_primitive::<Int16Type>().value(index))),
        DataType::Int32 => integer(i64::from(array.as_primitive::<Int32Type>().value(index))),
        DataType::Int64 => integer(array.as_primitive::<Int64Type>().value(index)),
        DataType::UInt8 => integer(i64::from(array.as_primitive::<UInt8Type>().value(index))),
        DataType::UInt16 => integer(i64::from(array.as_primitive::<UInt16Type>().value(index))),
        DataType::UInt32 => integer(i64::from(array.as_primitive::<UInt32Type>().value(index))),
        // SQLite's integers are 64-bit and signed: a value past `i64::MAX`, a
        // full status mask, keeps its bits.
        #[allow(clippy::cast_possible_wrap)]
        DataType::UInt64 => integer(array.as_primitive::<UInt64Type>().value(index) as i64),
        DataType::Float32 => Sql::Real(f64::from(array.as_primitive::<Float32Type>().value(index))),
        DataType::Float64 => Sql::Real(array.as_primitive::<Float64Type>().value(index)),
        DataType::Utf8 => Sql::Text(array.as_string::<i32>().value(index).to_owned()),
        DataType::LargeUtf8 => Sql::Text(array.as_string::<i64>().value(index).to_owned()),
        DataType::FixedSizeBinary(_) => {
            Sql::Text(super::hex(array.as_fixed_size_binary().value(index)))
        }
        DataType::Binary => Sql::Text(super::hex(array.as_binary::<i32>().value(index))),
        other => return Err(format!("a {other} column has no SQL value")),
    })
}

#[cfg(test)]
mod tests {
    use mechcore_mcfr::McfrTables;

    use super::*;

    fn layouts() -> Vec<(String, Table)> {
        [
            "ticks",
            "units",
            "rebirths",
            "projectiles",
            "buildings",
            "shields",
            "terrains",
            "statistics",
            "formations",
            "events",
        ]
        .into_iter()
        .map(|member| {
            let schema = McfrTables::schema_of(member).expect("a per-tick table");
            (
                member.to_owned(),
                tables(member, &schema).expect("lays out"),
            )
        })
        .collect()
    }

    #[test]
    fn every_member_lays_out() {
        let names = layouts()
            .into_iter()
            .flat_map(|(_, table)| {
                table
                    .all()
                    .into_iter()
                    .map(|each| each.name.clone())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        for expected in [
            "units",
            "units__buffs",
            "units__skills",
            "units__skills__enabled__weapons",
            "units__control__sources",
            "terrains__grid__rows",
            "events",
        ] {
            assert!(
                names.contains(&expected.to_owned()),
                "{expected} in {names:?}"
            );
        }
    }

    #[test]
    fn a_deeper_list_names_its_parents_ordinal() {
        let units = layouts()
            .into_iter()
            .find(|(member, _)| member == "units")
            .unwrap()
            .1;
        let units = units.all();
        let weapons = units
            .iter()
            .find(|table| table.name == "units__skills__enabled__weapons")
            .unwrap();
        assert_eq!(
            weapons
                .key()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            ["tick", "unit_id", "skills_ordinal", "ordinal"]
        );
        let skills = units
            .iter()
            .find(|table| table.name == "units__skills")
            .unwrap();
        let columns = skills
            .columns()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>();
        assert!(columns.contains(&"has_enabled"));
        assert!(columns.contains(&"enabled__lock_target__kind"));
        let state = skills
            .columns()
            .find(|column| column.name == "enabled__state")
            .unwrap();
        assert!(state.nullable && state.tags.is_some());
    }
}
