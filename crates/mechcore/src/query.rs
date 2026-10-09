//! `query`: answers an SQL query over a recording's tables.
//!
//! A recording's members are Arrow tables, nested where a row holds a struct
//! or a list. [`flatten`] lays each one out as relational tables by one rule,
//! read off the schema: a struct's fields become columns of the row that holds
//! it, a list becomes a table of its own keyed by the row that holds it and the
//! element's place in it. The tables are made in an in-memory SQLite database
//! empty, and a table is filled only once a query reads it, so a question about
//! the events never decodes the instrument channels.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    path::Path,
    sync::{Arc, Mutex},
};

use mechcore_mcfr::{McfrTables, TableOrigin};
use rusqlite::{
    Connection,
    functions::FunctionFlags,
    hooks::{AuthAction, AuthContext, Authorization},
    types::Value as Sql,
};
use serde::Serialize;
use serde_json::Value;

use crate::cli::{Args, Failure, Format, Outcome, Verdict};
use crate::kind::Kind;

mod flatten;

use flatten::{Column, Table};

/// The named queries the binary carries, by name.
const QUERIES: &[(&str, &str)] = &[
    ("death-timeline", include_str!("query/death-timeline.sql")),
    ("events-window", include_str!("query/events-window.sql")),
    (
        "kills-by-formation",
        include_str!("query/kills-by-formation.sql"),
    ),
];

/// Reads `query <file> (--sql <sql> | --query <name> | --schema)` off a
/// command line.
///
/// # Errors
///
/// Returns a usage failure for a command the contract does not define, a
/// refusal for a kind `query` does not take, and a failure for a recording
/// that does not read or a query SQLite does not run.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let sql = arguments.value("--sql")?;
    let named = arguments.value("--query")?;
    let schema = arguments.flag("--schema")?;
    let parameters = arguments
        .values("--param")?
        .into_iter()
        .map(|parameter| {
            parameter
                .split_once('=')
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .ok_or_else(|| Failure::usage(format!("--param {parameter:?} is not name=value")))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let input = arguments.path("a recording to query")?;
    arguments.finish()?;
    let asked = Asked::of(sql, named, schema)?;
    if format == Format::Text {
        print!("{}", answer_text(&input, asked, &parameters)?);
        return Ok(Verdict::Yes);
    }
    crate::cli::emit(&answer(&input, asked, &parameters)?, format)?;
    Ok(Verdict::Yes)
}

/// What a query asks of a recording.
pub(crate) enum Asked {
    Sql(String),
    Schema,
}

impl Asked {
    /// The question exactly one of `--sql`, `--query` and `--schema` asks.
    pub(crate) fn of(
        sql: Option<String>,
        named: Option<String>,
        schema: bool,
    ) -> Result<Self, Failure> {
        match (sql, named, schema) {
            (Some(sql), None, false) => Ok(Self::Sql(sql)),
            (None, Some(name), false) => QUERIES
                .iter()
                .find(|(known, _)| *known == name)
                .map(|(_, sql)| Self::Sql((*sql).to_owned()))
                .ok_or_else(|| {
                    Failure::usage(format!(
                        "no query {name:?}; the binary carries {}",
                        QUERIES
                            .iter()
                            .map(|(name, _)| *name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                }),
            (None, None, true) => Ok(Self::Schema),
            _ => Err(Failure::usage(
                "expected exactly one of --sql <sql>, --query <name> and --schema",
            )),
        }
    }
}

/// The answer to one question of one recording, as its result object.
///
/// # Errors
///
/// As [`run`], for everything but the command line.
pub(crate) fn answer(
    input: &Path,
    asked: Asked,
    parameters: &BTreeMap<String, String>,
) -> Result<Value, Failure> {
    let database = open(input, &asked, parameters)?;
    match asked {
        Asked::Schema => serde_json::to_value(database.schema())
            .map_err(|error| Failure::failed(format!("cannot write the result: {error}"))),
        Asked::Sql(sql) => Ok(database.query(&sql, parameters)?.json()),
    }
}

fn answer_text(
    input: &Path,
    asked: Asked,
    parameters: &BTreeMap<String, String>,
) -> Result<String, Failure> {
    let database = open(input, &asked, parameters)?;
    match asked {
        Asked::Schema => Ok(schema_text(&database.schema())),
        Asked::Sql(sql) => Ok(database.query(&sql, parameters)?.text()),
    }
}

fn open(
    input: &Path,
    asked: &Asked,
    parameters: &BTreeMap<String, String>,
) -> Result<Database, Failure> {
    if matches!(asked, Asked::Schema) && !parameters.is_empty() {
        return Err(Failure::usage("--param belongs to --sql and --query"));
    }
    Database::open(input)
}

/// A recording laid out as SQLite tables, each filled once a query reads it.
struct Database {
    connection: Connection,
    recording: McfrTables,
    /// Each member's own table, holding its lists' tables, by member.
    members: BTreeMap<String, Table>,
    filled: BTreeSet<String>,
    read: Arc<Mutex<BTreeSet<String>>>,
}

impl Database {
    fn open(input: &Path) -> Result<Self, Failure> {
        let (kind, _) = Kind::read(input)?;
        kind.require("query")?;
        let recording = McfrTables::open(input)
            .map_err(|error| Failure::failed(format!("{}: {error}", input.display())))?;
        let connection = Connection::open_in_memory().map_err(sqlite)?;
        connection
            .create_scalar_function(
                "q32",
                1,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                |context| {
                    let raw = context.get::<Option<i64>>(0)?;
                    #[allow(clippy::cast_precision_loss)]
                    Ok(raw.map(|raw| raw as f64 / 4_294_967_296.0))
                },
            )
            .map_err(sqlite)?;
        let mut members = BTreeMap::new();
        for member in recording.tables() {
            let schema = recording
                .schema(&member)
                .map_err(|error| Failure::failed(format!("{member}: {error}")))?;
            let table = flatten::tables(&member, &schema)
                .map_err(|error| Failure::failed(format!("{member}: {error}")))?;
            for table in table.all() {
                connection.execute_batch(&table.create()).map_err(sqlite)?;
            }
            members.insert(member, table);
        }
        connection
            .execute_batch(
                "CREATE TABLE meta (key TEXT NOT NULL, value TEXT NOT NULL);
                 CREATE VIEW fight AS SELECT
                   (SELECT value FROM meta WHERE key = 'producer') AS producer,
                   (SELECT value FROM meta WHERE key = 'game_build') AS game_build,
                   (SELECT value FROM meta WHERE key = 'format') AS format,
                   (SELECT value FROM meta WHERE key = 'result_hash') AS result_hash,
                   CAST((SELECT value FROM meta WHERE key = 'tick_count') AS INTEGER) AS tick_count,
                   CAST((SELECT value FROM meta WHERE key = 'terminal_tick') AS INTEGER) AS terminal_tick,
                   json_extract((SELECT value FROM meta WHERE key = 'durable_context'), '$.combat_round') AS combat_round;",
            )
            .map_err(sqlite)?;
        {
            let mut insert = connection
                .prepare("INSERT INTO meta (key, value) VALUES (?1, ?2)")
                .map_err(sqlite)?;
            for (key, value) in recording.metadata() {
                insert.execute((key, value)).map_err(sqlite)?;
            }
            insert
                .execute(("layout.yaml", recording.layout_yaml()))
                .map_err(sqlite)?;
        }
        let read = Arc::new(Mutex::new(BTreeSet::new()));
        let seen = Arc::clone(&read);
        connection
            .authorizer(Some(move |context: AuthContext<'_>| {
                if let AuthAction::Read { table_name, .. } = context.action {
                    seen.lock()
                        .expect("the authorizer runs on this thread")
                        .insert(table_name.to_owned());
                }
                Authorization::Allow
            }))
            .map_err(sqlite)?;
        Ok(Self {
            connection,
            recording,
            members,
            filled: BTreeSet::new(),
            read,
        })
    }

    /// Runs one statement, first filling every table it reads.
    fn query(
        mut self,
        sql: &str,
        parameters: &BTreeMap<String, String>,
    ) -> Result<Answer, Failure> {
        self.read.lock().expect("one thread").clear();
        // Preparing names every table the statement reads, through the views
        // it reads too, to the authorizer.
        drop(self.connection.prepare(sql).map_err(sqlite)?);
        let read = std::mem::take(&mut *self.read.lock().expect("one thread"));
        self.fill(&read)?;
        let mut statement = self.connection.prepare(sql).map_err(sqlite)?;
        let mut unused = parameters.keys().collect::<BTreeSet<_>>();
        for index in 1..=statement.parameter_count() {
            let name = statement
                .parameter_name(index)
                .ok_or_else(|| Failure::usage("a query's parameters are named, :name"))?;
            let key = name.trim_start_matches([':', '@', '$']);
            let value = parameters
                .get(key)
                .ok_or_else(|| Failure::usage(format!("the query takes --param {key}=<value>")))?;
            unused.remove(&key.to_owned());
            statement
                .raw_bind_parameter(index, parameter(value))
                .map_err(sqlite)?;
        }
        if let Some(name) = unused.first() {
            return Err(Failure::usage(format!(
                "the query takes no parameter {name}"
            )));
        }
        let columns = statement
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut rows = Vec::new();
        let mut cursor = statement.raw_query();
        while let Some(row) = cursor.next().map_err(sqlite)? {
            rows.push(
                (0..columns.len())
                    .map(|index| row.get::<_, Sql>(index))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(sqlite)?,
            );
        }
        Ok(Answer { columns, rows })
    }

    /// Fills the tables of `read` not filled yet, decoding each member once.
    fn fill(&mut self, read: &BTreeSet<String>) -> Result<(), Failure> {
        for (member, table) in &self.members {
            let tables = table.all();
            let wanted = tables
                .iter()
                .map(|table| table.name.clone())
                .filter(|name| read.contains(name) && !self.filled.contains(name))
                .collect::<BTreeSet<_>>();
            if wanted.is_empty() {
                continue;
            }
            let batches = self
                .recording
                .batches(member)
                .map_err(|error| Failure::failed(format!("{member}: {error}")))?;
            let transaction = self.connection.transaction().map_err(sqlite)?;
            flatten::insert(&transaction, table, &batches, &wanted)
                .map_err(|error| Failure::failed(format!("{member}: {error}")))?;
            for table in tables.iter().filter(|table| wanted.contains(&table.name)) {
                transaction.execute_batch(&table.index()).map_err(sqlite)?;
            }
            transaction.commit().map_err(sqlite)?;
            self.filled.extend(wanted);
        }
        Ok(())
    }

    fn schema(&self) -> Schema {
        let tables = self
            .members
            .iter()
            .flat_map(|(member, table)| {
                let origin = match self.recording.origin(member) {
                    Ok(TableOrigin::Instrument) => "instrument",
                    _ => "hashed",
                };
                table.all().into_iter().map(move |table| SchemaTable {
                    name: table.name.clone(),
                    member: format!("{member}.parquet"),
                    origin,
                    key: table.key().map(|column| column.name.clone()).collect(),
                    columns: table.columns().map(SchemaColumn::of).collect(),
                })
            })
            .collect();
        Schema {
            schema: "mechcore.query.schema.v1",
            tables,
            queries: QUERIES
                .iter()
                .map(|(name, sql)| SchemaQuery {
                    name,
                    description: sql
                        .lines()
                        .map_while(|line| line.strip_prefix("--"))
                        .map(str::trim)
                        .collect::<Vec<_>>()
                        .join(" "),
                    parameters: parameter_names(sql),
                })
                .collect(),
        }
    }
}

#[allow(clippy::needless_pass_by_value)] // `map_err` hands the error over
fn sqlite(error: rusqlite::Error) -> Failure {
    Failure::failed(format!("sqlite: {error}"))
}

/// A parameter's value: an integer or a number when it reads as one, text
/// otherwise.
fn parameter(value: &str) -> Sql {
    value
        .parse::<i64>()
        .map(Sql::Integer)
        .or_else(|_| value.parse::<f64>().map(Sql::Real))
        .unwrap_or_else(|_| Sql::Text(value.to_owned()))
}

/// The `:name` parameters a query takes, in the order it first names them.
fn parameter_names(sql: &str) -> Vec<String> {
    let mut names = Vec::new();
    let code = sql
        .lines()
        .map(|line| line.split_once("--").map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n");
    for (at, _) in code.match_indices(':') {
        let name = code[at + 1..]
            .chars()
            .take_while(|next| next.is_ascii_alphanumeric() || *next == '_')
            .collect::<String>();
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// What a statement answered: its columns, and its rows in the order it
/// answered them.
struct Answer {
    columns: Vec<String>,
    rows: Vec<Vec<Sql>>,
}

impl Answer {
    fn json(&self) -> Value {
        let cell = |value: &Sql| match value {
            Sql::Null => Value::Null,
            Sql::Integer(integer) => Value::from(*integer),
            Sql::Real(real) => Value::from(*real),
            Sql::Text(text) => Value::from(text.clone()),
            Sql::Blob(blob) => Value::from(hex(blob)),
        };
        serde_json::json!({
            "schema": "mechcore.query.v1",
            "columns": self.columns,
            "rows": self.rows.iter().map(|row| row.iter().map(cell).collect::<Vec<_>>()).collect::<Vec<_>>(),
        })
    }

    /// The rows as aligned columns under a header, numbers right-aligned.
    fn text(&self) -> String {
        let cells = self
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|value| match value {
                        Sql::Null => (String::new(), false),
                        Sql::Integer(integer) => (integer.to_string(), true),
                        Sql::Real(real) => (real.to_string(), true),
                        Sql::Text(text) => (text.clone(), false),
                        Sql::Blob(blob) => (hex(blob), false),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let widths = self
            .columns
            .iter()
            .enumerate()
            .map(|(index, name)| {
                cells
                    .iter()
                    .map(|row| row[index].0.chars().count())
                    .chain([name.chars().count()])
                    .max()
                    .unwrap_or(0)
            })
            .collect::<Vec<_>>();
        let line = |values: Vec<(String, bool)>| {
            values
                .iter()
                .zip(&widths)
                .map(|((value, right), width)| {
                    if *right {
                        format!("{value:>width$}")
                    } else {
                        format!("{value:<width$}")
                    }
                })
                .collect::<Vec<_>>()
                .join("  ")
                .trim_end()
                .to_owned()
        };
        let mut text = line(
            self.columns
                .iter()
                .map(|name| (name.clone(), false))
                .collect(),
        );
        text.push('\n');
        for row in cells {
            text.push_str(&line(row));
            text.push('\n');
        }
        text
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[derive(Serialize)]
#[allow(clippy::struct_field_names)] // `schema` names the result kind, as every result does
struct Schema {
    schema: &'static str,
    tables: Vec<SchemaTable>,
    queries: Vec<SchemaQuery>,
}

#[derive(Serialize)]
struct SchemaTable {
    name: String,
    member: String,
    origin: &'static str,
    key: Vec<String>,
    columns: Vec<SchemaColumn>,
}

#[derive(Serialize)]
struct SchemaColumn {
    name: String,
    #[serde(rename = "type")]
    sql_type: &'static str,
    nullable: bool,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
}

impl SchemaColumn {
    fn of(column: &Column) -> Self {
        Self {
            name: column.name.clone(),
            sql_type: column.sql_type(),
            nullable: column.nullable,
            path: column.path.clone(),
            tags: column.tag_names(),
        }
    }
}

#[derive(Serialize)]
struct SchemaQuery {
    name: &'static str,
    description: String,
    parameters: Vec<String>,
}

fn schema_text(schema: &Schema) -> String {
    let mut text = String::new();
    for table in &schema.tables {
        let _ = writeln!(
            text,
            "{} ({}, {}) key {}",
            table.name,
            table.member,
            table.origin,
            table.key.join(", ")
        );
        for column in &table.columns {
            let _ = writeln!(
                text,
                "  {} {}{}{}",
                column.name,
                column.sql_type,
                if column.nullable { " null" } else { "" },
                column
                    .tags
                    .as_ref()
                    .map(|tags| format!(" [{}]", tags.join(" ")))
                    .unwrap_or_default()
            );
        }
    }
    text.push_str("queries\n");
    for query in &schema.queries {
        let _ = write!(text, "  {}", query.name);
        for parameter in &query.parameters {
            let _ = write!(text, " --param {parameter}=<value>");
        }
        let _ = writeln!(text, "\n    {}", query.description);
    }
    text
}
