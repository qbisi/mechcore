//! A view of the events table per event kind, `ev_<kind>`.
//!
//! The events table holds every kind's payload in columns of its own, null
//! where a kind does not carry them. A kind's view keeps its rows and the
//! columns it may set: the event's place, its references, and the payload
//! columns [`event_kinds`] names for it, `reason` by its tag's name, which the
//! table cannot give since the enum it holds depends on the kind.

use mechcore_mcfr::event_kinds;

use super::flatten::{Column, Table};

/// The columns every kind's view keeps, ahead of its payload.
const COMMON: &[&str] = &[
    "tick",
    "ordinal",
    "object__kind",
    "object__id",
    "source__kind",
    "source__id",
    "source_team_id",
    "target__kind",
    "target__id",
];

/// One kind's view: its name, its columns with what each is in the events
/// table, and the statement that makes it.
pub(super) struct View {
    pub(super) name: String,
    pub(super) columns: Vec<ViewColumn>,
    pub(super) create: String,
}

pub(super) struct ViewColumn {
    pub(super) name: String,
    pub(super) sql_type: &'static str,
    pub(super) nullable: bool,
    pub(super) path: String,
    pub(super) tags: Option<Vec<String>>,
}

/// Every kind's view of `events`, the events table as laid out.
pub(super) fn views(events: &Table) -> Vec<View> {
    let column = |name: &str| events.columns().find(|column| column.name == name);
    event_kinds()
        .into_iter()
        .map(|kind| {
            let name = format!("ev_{}", kind.name);
            let mut columns = Vec::new();
            let mut select = Vec::new();
            for common in COMMON {
                if let Some(column) = column(common) {
                    select.push(format!("\"{}\"", column.name));
                    columns.push(ViewColumn::of(column, column.nullable, column.tag_names()));
                }
            }
            for field in kind.payload {
                let carried = events.columns().filter(|column| {
                    column.name == *field
                        || column.name == format!("has_{field}")
                        || column
                            .name
                            .strip_prefix(*field)
                            .is_some_and(|rest| rest.starts_with("__"))
                });
                for column in carried {
                    if let (Some(tags), "reason") = (&kind.reason_tags, column.name.as_str()) {
                        let cases = tags
                            .iter()
                            .map(|(tag, name)| format!("WHEN {tag} THEN '{name}'"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        select.push(format!("CASE reason {cases} ELSE reason END AS reason"));
                        columns.push(ViewColumn {
                            sql_type: "TEXT",
                            ..ViewColumn::of(
                                column,
                                false,
                                Some(
                                    tags.iter()
                                        .map(|(tag, name)| format!("{tag}={name}"))
                                        .collect(),
                                ),
                            )
                        });
                    } else {
                        select.push(format!("\"{}\"", column.name));
                        columns.push(ViewColumn::of(column, column.nullable, column.tag_names()));
                    }
                }
            }
            let create = format!(
                "CREATE VIEW \"{name}\" AS SELECT {} FROM events WHERE type = '{}';",
                select.join(", "),
                kind.name
            );
            View {
                name,
                columns,
                create,
            }
        })
        .collect()
}

impl ViewColumn {
    fn of(column: &Column, nullable: bool, tags: Option<Vec<String>>) -> Self {
        Self {
            name: column.name.clone(),
            sql_type: column.sql_type(),
            nullable,
            path: column.path.clone(),
            tags,
        }
    }
}
