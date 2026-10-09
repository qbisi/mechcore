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
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use mechcore_mcfr::{McfrTables, TableOrigin};
use rusqlite::{
    Connection, TransactionBehavior,
    functions::FunctionFlags,
    hooks::{AuthAction, AuthContext, Authorization},
    types::Value as Sql,
};
use serde::Serialize;
use serde_json::Value;

use crate::cli::{Args, Failure, Format, Outcome, Verdict};
use crate::kind::Kind;

mod events;
mod flatten;
mod layout;

use flatten::{Column, Table};

/// The named queries the binary carries, by name.
const QUERIES: &[(&str, &str)] = &[
    ("death-timeline", include_str!("query/death-timeline.sql")),
    ("divergence", include_str!("query/divergence.sql")),
    ("events-window", include_str!("query/events-window.sql")),
    (
        "kills-by-formation",
        include_str!("query/kills-by-formation.sql"),
    ),
    ("travel-distance", include_str!("query/travel-distance.sql")),
    ("unit-at", include_str!("query/unit-at.sql")),
    ("units-diff", include_str!("query/units-diff.sql")),
];

/// Reads `query <file>... (--sql <sql> | --sql-file <path> | --query <name> |
/// --schema) [--no-cache]` off a command line.
///
/// # Errors
///
/// Returns a usage failure for a command the contract does not define, a
/// refusal for a kind `query` does not take, and a failure for a recording
/// that does not read or a query SQLite does not run.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let sql = arguments.value("--sql")?;
    let sql_file = arguments.value("--sql-file")?.map(PathBuf::from);
    let named = arguments.value("--query")?;
    let schema = arguments.flag("--schema")?;
    let store = if arguments.flag("--no-cache")? {
        Store::Memory
    } else {
        Store::Cache
    };
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
    let operands = arguments.operands()?;
    let inputs = Inputs::of(&operands)?;
    let asked = Asked::of(sql, sql_file.as_deref(), named, schema)?;
    if format == Format::Text {
        print!("{}", answer_text(&inputs, store, asked, &parameters)?);
        return Ok(Verdict::Yes);
    }
    crate::cli::emit(&answer(&inputs, store, asked, &parameters)?, format)?;
    Ok(Verdict::Yes)
}

/// The recordings a query reads: one, whose tables go unprefixed, or
/// several, each under its own name.
pub(crate) enum Inputs {
    One(PathBuf),
    Named(Vec<(String, PathBuf)>),
}

impl Inputs {
    /// Reads the operands: one path; two paths, which are `left` and
    /// `right`; or `name=path` each, under any number of names.
    pub(crate) fn of(operands: &[String]) -> Result<Self, Failure> {
        let named = operands
            .iter()
            .map(|operand| {
                operand
                    .split_once('=')
                    .filter(|(name, _)| schema_name(name))
                    .map(|(name, path)| (name.to_owned(), PathBuf::from(path)))
            })
            .collect::<Vec<_>>();
        match (operands, named.iter().all(Option::is_some)) {
            ([], _) => Err(Failure::usage("expected a recording to query")),
            (_, true) => Self::named(named.into_iter().flatten().collect()),
            ([one], false) if named[0].is_none() => Ok(Self::One(PathBuf::from(one))),
            ([left, right], false) if named.iter().all(Option::is_none) => Self::named(vec![
                ("left".to_owned(), PathBuf::from(left)),
                ("right".to_owned(), PathBuf::from(right)),
            ]),
            _ => Err(Failure::usage(
                "expected one recording, two (left and right), or name=path for each",
            )),
        }
    }

    fn named(named: Vec<(String, PathBuf)>) -> Result<Self, Failure> {
        let mut seen = BTreeSet::new();
        for (name, _) in &named {
            if !seen.insert(name.as_str()) {
                return Err(Failure::usage(format!(
                    "recording name {name} is given twice"
                )));
            }
        }
        Ok(Self::Named(named))
    }
}

/// Whether a word names an attached recording: lowercase, digits and `_`,
/// starting with a letter, and not one of SQLite's own schemas.
fn schema_name(name: &str) -> bool {
    name.starts_with(|first: char| first.is_ascii_lowercase())
        && name
            .chars()
            .all(|next| next.is_ascii_lowercase() || next.is_ascii_digit() || next == '_')
        && !matches!(name, "main" | "temp")
}

/// What a query asks of a recording.
pub(crate) enum Asked {
    Sql(String),
    Schema,
}

impl Asked {
    /// The question exactly one of `--sql`, `--sql-file`, `--query` and
    /// `--schema` asks.
    pub(crate) fn of(
        sql: Option<String>,
        sql_file: Option<&Path>,
        named: Option<String>,
        schema: bool,
    ) -> Result<Self, Failure> {
        let sql = match (sql, sql_file) {
            (Some(_), Some(_)) => Some(None),
            (sql, None) => sql.map(Some),
            (None, Some(path)) => {
                Some(Some(std::fs::read_to_string(path).map_err(|error| {
                    Failure::failed(format!("{}: {error}", path.display()))
                })?))
            }
        };
        match (sql, named, schema) {
            (Some(Some(sql)), None, false) => Ok(Self::Sql(sql)),
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
                "expected exactly one of --sql <sql>, --sql-file <path>, --query <name> and \
                 --schema",
            )),
        }
    }
}

/// The answer to one question of the recordings, as its result object.
///
/// # Errors
///
/// As [`run`], for everything but the command line.
pub(crate) fn answer(
    inputs: &Inputs,
    store: Store,
    asked: Asked,
    parameters: &BTreeMap<String, String>,
) -> Result<Value, Failure> {
    let database = open(inputs, store, &asked, parameters)?;
    match asked {
        Asked::Schema => serde_json::to_value(database.schema()?)
            .map_err(|error| Failure::failed(format!("cannot write the result: {error}"))),
        Asked::Sql(sql) => Ok(database.query(&sql, parameters)?.json()),
    }
}

fn answer_text(
    inputs: &Inputs,
    store: Store,
    asked: Asked,
    parameters: &BTreeMap<String, String>,
) -> Result<String, Failure> {
    let database = open(inputs, store, &asked, parameters)?;
    match asked {
        Asked::Schema => Ok(schema_text(&database.schema()?)),
        Asked::Sql(sql) => Ok(database.query(&sql, parameters)?.text()),
    }
}

fn open(
    inputs: &Inputs,
    store: Store,
    asked: &Asked,
    parameters: &BTreeMap<String, String>,
) -> Result<Database, Failure> {
    if matches!(asked, Asked::Schema) && !parameters.is_empty() {
        return Err(Failure::usage("--param belongs to --sql and --query"));
    }
    Database::open(inputs, store)
}

/// Where a recording's tables are kept.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Store {
    /// In a database file of the cache, which every later query of the same
    /// recording by the same binary opens again with what it filled.
    Cache,
    /// In memory, gone with the query.
    Memory,
}

/// The directory a recording's cached tables are kept in:
/// `MECHCORE_QUERY_CACHE`, or `/tmp/mechcore-query`, which the system clears,
/// so a cache lives no longer than it is used; a platform without `/tmp` uses
/// its own temporary directory.
fn cache_directory() -> PathBuf {
    std::env::var_os("MECHCORE_QUERY_CACHE").map_or_else(
        || {
            let tmp = Path::new("/tmp");
            if cfg!(unix) && tmp.is_dir() {
                tmp.join("mechcore-query")
            } else {
                std::env::temp_dir().join("mechcore-query")
            }
        },
        PathBuf::from,
    )
}

/// The cache file of one recording's tables: named by the recording's content
/// and by the binary that lays it out, its size and modification time, so a
/// moved recording finds its tables and another build lays them out afresh.
fn cache_file(input: &Path) -> Result<PathBuf, String> {
    let mut hasher = blake3::Hasher::new();
    let mut file = std::fs::File::open(input).map_err(|error| error.to_string())?;
    std::io::copy(&mut file, &mut hasher).map_err(|error| error.to_string())?;
    let content = hasher.finalize().to_hex();
    let binary = std::env::current_exe()
        .and_then(std::fs::metadata)
        .map_err(|error| error.to_string())?;
    let modified = binary
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    let build = blake3::hash(format!("{}:{modified}", binary.len()).as_bytes()).to_hex();
    let directory = cache_directory();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory.join(format!("{}-{}.sqlite", &content[..32], &build[..16])))
}

/// A connection to the database a recording's tables are kept in: its cache
/// file, or memory where the cache cannot be written, which standard error
/// says. Several recordings each need a database the query's connection can
/// attach, so one kept in memory is shared under a name of this process's.
fn store_connection(
    input: &Path,
    store: Store,
    name: Option<&str>,
) -> Result<(Connection, String), Failure> {
    if store == Store::Cache {
        match cache_file(input) {
            Ok(path) => {
                let connection = Connection::open(&path).map_err(sqlite)?;
                connection
                    .busy_timeout(std::time::Duration::from_secs(300))
                    .map_err(sqlite)?;
                return Ok((connection, path.display().to_string()));
            }
            Err(error) => {
                eprintln!("query: the cache cannot be written ({error}); reading in memory");
            }
        }
    }
    match name {
        None => Ok((
            Connection::open_in_memory().map_err(sqlite)?,
            ":memory:".to_owned(),
        )),
        Some(name) => {
            let uri = format!(
                "file:mechcore-query-{}-{name}?mode=memory&cache=shared",
                std::process::id()
            );
            Ok((Connection::open(&uri).map_err(sqlite)?, uri))
        }
    }
}

/// The recordings a query reads, and the connection it runs on.
///
/// Each recording's tables live in a database of their own, a cache file or
/// memory. One recording's query runs on its database's connection; several
/// are attached to the query's connection under their names, so a
/// recording's tables are made and filled the same way however many are read.
struct Database {
    /// The connection several recordings are attached to; one recording's
    /// query runs on its own connection.
    attached: Option<Connection>,
    recordings: Vec<Attached>,
    /// The tables the statement being prepared reads, by schema.
    read: Arc<Mutex<BTreeSet<(String, String)>>>,
}

/// One recording laid out as SQLite tables, each filled once a query reads it.
struct Attached {
    /// The schema the query reads it under: `main` alone, its name attached.
    schema: String,
    connection: Connection,
    recording: McfrTables,
    /// Each member's own table, holding its lists' tables, by member.
    members: BTreeMap<String, Table>,
    filled: BTreeSet<String>,
}

impl Database {
    fn open(inputs: &Inputs, store: Store) -> Result<Self, Failure> {
        let (attached, recordings) = match inputs {
            Inputs::One(input) => {
                let (connection, _) = store_connection(input, store, None)?;
                (None, vec![Attached::open(input, "main", connection)?])
            }
            Inputs::Named(named) => {
                let query = Connection::open_in_memory().map_err(sqlite)?;
                query
                    .busy_timeout(std::time::Duration::from_secs(300))
                    .map_err(sqlite)?;
                let mut recordings = Vec::new();
                for (name, input) in named {
                    let (connection, location) = store_connection(input, store, Some(name))?;
                    recordings.push(Attached::open(input, name, connection)?);
                    query
                        .execute(&format!("ATTACH DATABASE ?1 AS \"{name}\""), [&location])
                        .map_err(sqlite)?;
                }
                (Some(query), recordings)
            }
        };
        let database = Self {
            attached,
            recordings,
            read: Arc::new(Mutex::new(BTreeSet::new())),
        };
        let connection = database.connection();
        functions(connection)?;
        let seen = Arc::clone(&database.read);
        connection
            .authorizer(Some(move |context: AuthContext<'_>| {
                if let AuthAction::Read { table_name, .. } = context.action {
                    seen.lock()
                        .expect("the authorizer runs on this thread")
                        .insert((
                            context.database_name.unwrap_or("main").to_owned(),
                            table_name.to_owned(),
                        ));
                }
                Authorization::Allow
            }))
            .map_err(sqlite)?;
        Ok(database)
    }

    /// The connection a query runs on.
    fn connection(&self) -> &Connection {
        self.attached
            .as_ref()
            .unwrap_or(&self.recordings[0].connection)
    }

    /// Runs one statement, first filling every table it reads.
    /// Runs the statements of `sql` in order, each once every table it reads
    /// is filled, and answers the last one's rows. A statement may read what
    /// an earlier one made, a temporary view or table, since each is prepared
    /// only once the ones before it have run.
    fn query(
        mut self,
        sql: &str,
        parameters: &BTreeMap<String, String>,
    ) -> Result<Answer, Failure> {
        let statements = statements(sql);
        let Some((last, earlier)) = statements.split_last() else {
            return Err(Failure::usage("the query holds no statement"));
        };
        let mut unused = parameters.keys().cloned().collect::<BTreeSet<_>>();
        for statement in earlier {
            self.fill_for(statement)?;
            let mut prepared = self.connection().prepare(statement).map_err(sqlite)?;
            bind(&mut prepared, parameters, &mut unused)?;
            // An earlier statement's rows, if it has any, are not the answer.
            let mut rows = prepared.raw_query();
            while rows.next().map_err(sqlite)?.is_some() {}
        }
        self.fill_for(last)?;
        let mut statement = self.connection().prepare(last).map_err(sqlite)?;
        bind(&mut statement, parameters, &mut unused)?;
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

    /// Fills every table one statement reads, which preparing it names to
    /// the authorizer, through the views it reads too.
    fn fill_for(&mut self, statement: &str) -> Result<(), Failure> {
        self.read.lock().expect("one thread").clear();
        drop(self.connection().prepare(statement).map_err(sqlite)?);
        let read = std::mem::take(&mut *self.read.lock().expect("one thread"));
        for recording in &mut self.recordings {
            let tables = read
                .iter()
                .filter(|(schema, _)| *schema == recording.schema)
                .map(|(_, table)| table.clone())
                .collect();
            recording.fill(&tables)?;
        }
        Ok(())
    }

    fn schema(&self) -> Result<Schema, Failure> {
        let mut tables = Vec::new();
        for recording in &self.recordings {
            tables.extend(recording.schema(self.attached.is_some())?);
        }
        Ok(Schema {
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
        })
    }
}

/// The functions a query may call beside SQLite's own.
fn functions(connection: &Connection) -> Result<(), Failure> {
    let numeric = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_scalar_function("q32", 1, numeric, |context| {
            let raw = context.get::<Option<i64>>(0)?;
            #[allow(clippy::cast_precision_loss)]
            Ok(raw.map(|raw| raw as f64 / 4_294_967_296.0))
        })
        .map_err(sqlite)?;
    connection
        .create_scalar_function("sqrt", 1, numeric, |context| {
            Ok(context.get::<Option<f64>>(0)?.map(f64::sqrt))
        })
        .map_err(sqlite)?;
    connection
        .create_scalar_function("hypot", 2, numeric, |context| {
            let x = context.get::<Option<f64>>(0)?;
            let y = context.get::<Option<f64>>(1)?;
            Ok(x.zip(y).map(|(x, y)| x.hypot(y)))
        })
        .map_err(sqlite)?;
    Ok(())
}

/// Makes one recording's tables, views and `meta`, every table empty.
fn make(
    connection: &Connection,
    input: &Path,
    recording: &McfrTables,
    members: &BTreeMap<String, Table>,
) -> Result<(), Failure> {
    for table in members.values() {
        for table in table.all() {
            connection.execute_batch(&table.create()).map_err(sqlite)?;
        }
    }
    connection
        .execute_batch(
            "CREATE TABLE mechcore_filled (name TEXT PRIMARY KEY);
             CREATE TABLE meta (key TEXT NOT NULL, value TEXT NOT NULL);
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
    connection.execute_batch(layout::CREATE).map_err(sqlite)?;
    if let Some(events) = members.get("events") {
        for view in events::views(events) {
            connection.execute_batch(&view.create).map_err(sqlite)?;
        }
    }
    let mut insert = connection
        .prepare("INSERT INTO meta (key, value) VALUES (?1, ?2)")
        .map_err(sqlite)?;
    for (key, value) in recording.metadata() {
        insert.execute((key, value)).map_err(sqlite)?;
    }
    insert
        .execute(("layout.yaml", recording.layout_yaml()))
        .map_err(sqlite)?;
    insert
        .execute(("path", input.display().to_string()))
        .map_err(sqlite)?;
    Ok(())
}

/// The tables a database records filled.
fn filled(connection: &Connection) -> Result<BTreeSet<String>, Failure> {
    let mut statement = connection
        .prepare("SELECT name FROM mechcore_filled")
        .map_err(sqlite)?;
    let names = statement
        .query_map([], |row| row.get(0))
        .map_err(sqlite)?
        .collect::<Result<_, _>>()
        .map_err(sqlite)?;
    Ok(names)
}

fn record_filled(connection: &Connection, tables: &BTreeSet<String>) -> Result<(), Failure> {
    let mut insert = connection
        .prepare("INSERT OR IGNORE INTO mechcore_filled (name) VALUES (?1)")
        .map_err(sqlite)?;
    for table in tables {
        insert.execute([table]).map_err(sqlite)?;
    }
    Ok(())
}

impl Attached {
    /// Opens one recording's tables in `connection`'s database, making them
    /// empty the first time: a cache file another query made already holds
    /// them, and what that query filled.
    fn open(input: &Path, schema: &str, mut connection: Connection) -> Result<Self, Failure> {
        let (kind, _) = Kind::read(input)?;
        kind.require("query")?;
        let recording = McfrTables::open(input)
            .map_err(|error| Failure::failed(format!("{}: {error}", input.display())))?;
        let mut members = BTreeMap::new();
        for member in recording.tables() {
            let table_schema = recording
                .schema(&member)
                .map_err(|error| Failure::failed(format!("{member}: {error}")))?;
            let table = flatten::tables(&member, &table_schema)
                .map_err(|error| Failure::failed(format!("{member}: {error}")))?;
            members.insert(member, table);
        }
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite)?;
        let made: bool = transaction
            .query_row(
                "SELECT count(*) > 0 FROM sqlite_master WHERE name = 'mechcore_filled'",
                [],
                |row| row.get(0),
            )
            .map_err(sqlite)?;
        if made {
            transaction
                .execute(
                    "UPDATE meta SET value = ?1 WHERE key = 'path'",
                    [input.display().to_string()],
                )
                .map_err(sqlite)?;
        } else {
            make(&transaction, input, &recording, &members)?;
        }
        transaction.commit().map_err(sqlite)?;
        let filled = filled(&connection)?;
        Ok(Self {
            schema: schema.to_owned(),
            connection,
            recording,
            members,
            filled,
        })
    }

    /// Fills the tables of `read` not filled yet, decoding each member once,
    /// and records them filled. Another query of the same cache file may
    /// have filled some since this one opened it, which the write lock each
    /// fill takes lets it see.
    fn fill(&mut self, read: &BTreeSet<String>) -> Result<(), Failure> {
        if read.iter().all(|table| self.filled.contains(table)) {
            return Ok(());
        }
        self.filled = filled(&self.connection)?;
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
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sqlite)?;
            let wanted = wanted
                .difference(&filled(&transaction)?)
                .cloned()
                .collect::<BTreeSet<_>>();
            flatten::insert(&transaction, table, &batches, &wanted)
                .map_err(|error| Failure::failed(format!("{member}: {error}")))?;
            for table in tables.iter().filter(|table| wanted.contains(&table.name)) {
                transaction.execute_batch(&table.index()).map_err(sqlite)?;
            }
            record_filled(&transaction, &wanted)?;
            transaction.commit().map_err(sqlite)?;
            self.filled.extend(wanted);
        }
        if layout::TABLES
            .iter()
            .any(|table| read.contains(*table) && !self.filled.contains(*table))
        {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sqlite)?;
            let tables = layout::TABLES
                .iter()
                .map(|table| (*table).to_owned())
                .collect::<BTreeSet<_>>();
            if filled(&transaction)?.is_disjoint(&tables) {
                layout::fill(&transaction, &self.recording)
                    .map_err(|error| Failure::failed(format!("layout.yaml: {error}")))?;
                record_filled(&transaction, &tables)?;
            }
            transaction.commit().map_err(sqlite)?;
            self.filled.extend(tables);
        }
        Ok(())
    }

    /// Its tables, each named under its schema when several are attached.
    fn schema(&self, attached: bool) -> Result<Vec<SchemaTable>, Failure> {
        let database = attached.then(|| self.schema.clone());
        let mut tables: Vec<SchemaTable> = self
            .members
            .iter()
            .flat_map(|(member, table)| {
                let origin = match self.recording.origin(member) {
                    Ok(TableOrigin::Instrument) => "instrument",
                    _ => "hashed",
                };
                let database = database.clone();
                table.all().into_iter().map(move |table| SchemaTable {
                    database: database.clone(),
                    name: table.name.clone(),
                    member: format!("{member}.parquet"),
                    origin,
                    key: table.key().map(|column| column.name.clone()).collect(),
                    columns: table.columns().map(SchemaColumn::of).collect(),
                })
            })
            .collect();
        if let Some(events) = self.members.get("events") {
            for view in events::views(events) {
                tables.push(SchemaTable {
                    database: database.clone(),
                    name: view.name,
                    member: "events.parquet".to_owned(),
                    origin: "view",
                    key: vec!["tick".to_owned(), "ordinal".to_owned()],
                    columns: view
                        .columns
                        .into_iter()
                        .map(|column| SchemaColumn {
                            name: column.name,
                            sql_type: column.sql_type,
                            nullable: column.nullable,
                            path: column.path,
                            tags: column.tags,
                        })
                        .collect(),
                });
            }
        }
        for table in layout::TABLES {
            let mut statement = self
                .connection
                .prepare(&format!("PRAGMA table_info(\"{table}\")"))
                .map_err(sqlite)?;
            let columns = statement
                .query_map([], |row| {
                    Ok(SchemaColumn {
                        name: row.get(1)?,
                        sql_type: match row.get::<_, String>(2)?.as_str() {
                            "INTEGER" => "INTEGER",
                            "TEXT" => "TEXT",
                            _ => "ANY",
                        },
                        nullable: row.get::<_, i64>(3)? == 0,
                        path: "layout.yaml".to_owned(),
                        tags: None,
                    })
                })
                .map_err(sqlite)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sqlite)?;
            tables.push(SchemaTable {
                database: database.clone(),
                name: (*table).to_owned(),
                member: "layout.yaml".to_owned(),
                origin: "layout",
                key: layout::key(table)
                    .iter()
                    .map(|name| (*name).to_owned())
                    .collect(),
                columns,
            });
        }
        Ok(tables)
    }
}

/// Binds a statement's `:name` parameters from `parameters`, striking each
/// it takes from `unused`.
fn bind(
    statement: &mut rusqlite::Statement<'_>,
    parameters: &BTreeMap<String, String>,
    unused: &mut BTreeSet<String>,
) -> Result<(), Failure> {
    for index in 1..=statement.parameter_count() {
        let name = statement
            .parameter_name(index)
            .ok_or_else(|| Failure::usage("a query's parameters are named, :name"))?;
        let key = name.trim_start_matches([':', '@', '$']).to_owned();
        let value = parameters
            .get(&key)
            .ok_or_else(|| Failure::usage(format!("the query takes --param {key}=<value>")))?;
        statement
            .raw_bind_parameter(index, parameter(value))
            .map_err(sqlite)?;
        unused.remove(&key);
    }
    Ok(())
}

/// The statements of a query in order: split at each `;` outside a quoted
/// string or name and outside a comment, each kept with its own comments, and
/// none that holds only whitespace and comments.
fn statements(sql: &str) -> Vec<&str> {
    let bytes = sql.as_bytes();
    let mut statements = Vec::new();
    let (mut start, mut at) = (0, 0);
    while at < bytes.len() {
        match bytes[at] {
            quote @ (b'\'' | b'"' | b'`') => {
                at += 1;
                while at < bytes.len() {
                    if bytes[at] == quote {
                        // A doubled quote stands for itself.
                        if bytes.get(at + 1) == Some(&quote) {
                            at += 1;
                        } else {
                            break;
                        }
                    }
                    at += 1;
                }
            }
            b'[' => {
                while at < bytes.len() && bytes[at] != b']' {
                    at += 1;
                }
            }
            b'-' if bytes.get(at + 1) == Some(&b'-') => {
                while at < bytes.len() && bytes[at] != b'\n' {
                    at += 1;
                }
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                at += 2;
                while at + 1 < bytes.len() && !(bytes[at] == b'*' && bytes[at + 1] == b'/') {
                    at += 1;
                }
                at += 1;
            }
            b';' => {
                statements.push(&sql[start..at]);
                start = at + 1;
            }
            _ => {}
        }
        at += 1;
    }
    statements.push(&sql[start.min(sql.len())..]);
    statements
        .into_iter()
        .filter(|statement| !is_blank(statement))
        .collect()
}

/// Whether a piece of SQL holds nothing but whitespace and comments.
fn is_blank(sql: &str) -> bool {
    let mut rest = sql.trim_start();
    loop {
        if let Some(line) = rest.strip_prefix("--") {
            rest = line
                .split_once('\n')
                .map_or("", |(_, after)| after)
                .trim_start();
        } else if let Some(block) = rest.strip_prefix("/*") {
            rest = block
                .split_once("*/")
                .map_or("", |(_, after)| after)
                .trim_start();
        } else {
            return rest.is_empty();
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
    #[serde(skip_serializing_if = "Option::is_none")]
    database: Option<String>,
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
            "{}{} ({}, {}) key {}",
            table
                .database
                .as_ref()
                .map(|database| format!("{database}."))
                .unwrap_or_default(),
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

#[cfg(test)]
mod tests {
    use super::statements;

    #[test]
    fn statements_split_at_semicolons_outside_strings_and_comments() {
        assert_eq!(
            statements("SELECT 1; SELECT ';', \"a;b\" -- c;d\n; /* e; */ SELECT 2;\n-- tail\n"),
            [
                "SELECT 1",
                " SELECT ';', \"a;b\" -- c;d\n",
                " /* e; */ SELECT 2"
            ]
        );
        assert_eq!(statements("SELECT 'it''s; fine'"), ["SELECT 'it''s; fine'"]);
        assert!(statements("-- nothing\n/* at all */ ;").is_empty());
    }
}
