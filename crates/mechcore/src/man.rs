//! The manual the binary carries: the game's rules, and its own contracts.
//!
//! A binary is distributed on its own, so what it knows travels with it. The
//! documents are compiled in, and `mechcore man <topic>` reads one back.

use serde::Serialize;

use crate::cli::{Args, Failure, Format, Outcome, Verdict};

/// Every topic, as `(topic, text)`.
///
/// The topic is the document's path under `docs/`, without the extension, so a
/// link between two documents names a topic the same way a reader would.
static TOPICS: &[(&str, &str)] = &[
    (
        "rules/battle_skill",
        include_str!("../../../docs/rules/battle_skill.md"),
    ),
    (
        "rules/battle_skill.zh",
        include_str!("../../../docs/rules/battle_skill.zh.md"),
    ),
    (
        "rules/combat",
        include_str!("../../../docs/rules/combat.md"),
    ),
    (
        "rules/combat.zh",
        include_str!("../../../docs/rules/combat.zh.md"),
    ),
    (
        "rules/commander_skills",
        include_str!("../../../docs/rules/commander_skills.md"),
    ),
    (
        "rules/commander_skills.zh",
        include_str!("../../../docs/rules/commander_skills.zh.md"),
    ),
    (
        "rules/constructions",
        include_str!("../../../docs/rules/constructions.md"),
    ),
    (
        "rules/constructions.zh",
        include_str!("../../../docs/rules/constructions.zh.md"),
    ),
    (
        "rules/equipment",
        include_str!("../../../docs/rules/equipment.md"),
    ),
    (
        "rules/equipment.zh",
        include_str!("../../../docs/rules/equipment.zh.md"),
    ),
    (
        "rules/landing",
        include_str!("../../../docs/rules/landing.md"),
    ),
    (
        "rules/landing.zh",
        include_str!("../../../docs/rules/landing.zh.md"),
    ),
    ("rules/map", include_str!("../../../docs/rules/map.md")),
    (
        "rules/map.zh",
        include_str!("../../../docs/rules/map.zh.md"),
    ),
    (
        "rules/mobility",
        include_str!("../../../docs/rules/mobility.md"),
    ),
    (
        "rules/mobility.zh",
        include_str!("../../../docs/rules/mobility.zh.md"),
    ),
    (
        "rules/officers",
        include_str!("../../../docs/rules/officers.md"),
    ),
    (
        "rules/officers.zh",
        include_str!("../../../docs/rules/officers.zh.md"),
    ),
    (
        "rules/officer_effects",
        include_str!("../../../docs/rules/officer_effects.md"),
    ),
    (
        "rules/officer_effects.zh",
        include_str!("../../../docs/rules/officer_effects.zh.md"),
    ),
    (
        "rules/opening",
        include_str!("../../../docs/rules/opening.md"),
    ),
    (
        "rules/opening.zh",
        include_str!("../../../docs/rules/opening.zh.md"),
    ),
    (
        "rules/reinforce_items",
        include_str!("../../../docs/rules/reinforce_items.md"),
    ),
    (
        "rules/reinforce_items.zh",
        include_str!("../../../docs/rules/reinforce_items.zh.md"),
    ),
    (
        "rules/reinforcements",
        include_str!("../../../docs/rules/reinforcements.md"),
    ),
    (
        "rules/reinforcements.zh",
        include_str!("../../../docs/rules/reinforcements.zh.md"),
    ),
    (
        "rules/technology_effects",
        include_str!("../../../docs/rules/technology_effects.md"),
    ),
    (
        "rules/technology_effects.zh",
        include_str!("../../../docs/rules/technology_effects.zh.md"),
    ),
    (
        "rules/terrain",
        include_str!("../../../docs/rules/terrain.md"),
    ),
    (
        "rules/terrain.zh",
        include_str!("../../../docs/rules/terrain.zh.md"),
    ),
    (
        "rules/turrets",
        include_str!("../../../docs/rules/turrets.md"),
    ),
    (
        "rules/turrets.zh",
        include_str!("../../../docs/rules/turrets.zh.md"),
    ),
    (
        "rules/visibility",
        include_str!("../../../docs/rules/visibility.md"),
    ),
    (
        "rules/visibility.zh",
        include_str!("../../../docs/rules/visibility.zh.md"),
    ),
    (
        "rules/unit_experience",
        include_str!("../../../docs/rules/unit_experience.md"),
    ),
    (
        "rules/unit_experience.zh",
        include_str!("../../../docs/rules/unit_experience.zh.md"),
    ),
    (
        "rules/unit_levels",
        include_str!("../../../docs/rules/unit_levels.md"),
    ),
    (
        "rules/unit_techs",
        include_str!("../../../docs/rules/unit_techs.md"),
    ),
    (
        "rules/unit_techs.zh",
        include_str!("../../../docs/rules/unit_techs.zh.md"),
    ),
    (
        "spec/adapter/adapter",
        include_str!("../../../docs/spec/adapter/adapter.md"),
    ),
    (
        "spec/document/action",
        include_str!("../../../docs/spec/document/action.md"),
    ),
    (
        "spec/document/battle",
        include_str!("../../../docs/spec/document/battle.md"),
    ),
    (
        "spec/document/layout",
        include_str!("../../../docs/spec/document/layout.md"),
    ),
    (
        "spec/document/state",
        include_str!("../../../docs/spec/document/state.md"),
    ),
    (
        "spec/mcfr/mcfr",
        include_str!("../../../docs/spec/mcfr/mcfr.md"),
    ),
    (
        "spec/mcfr/mcfr.zh",
        include_str!("../../../docs/spec/mcfr/mcfr.zh.md"),
    ),
    (
        "spec/mechcore/cli",
        include_str!("../../../docs/spec/mechcore/cli.md"),
    ),
    (
        "spec/mechcore/mcscript",
        include_str!("../../../docs/spec/mechcore/mcscript.md"),
    ),
    (
        "spec/mechcore/session",
        include_str!("../../../docs/spec/mechcore/session.md"),
    ),
    (
        "spec/mechcore/turn",
        include_str!("../../../docs/spec/mechcore/turn.md"),
    ),
    (
        "spec/simulation/architecture",
        include_str!("../../../docs/spec/simulation/architecture.md"),
    ),
    (
        "spec/simulation/architecture.zh",
        include_str!("../../../docs/spec/simulation/architecture.zh.md"),
    ),
    (
        "spec/simulation/quadtree",
        include_str!("../../../docs/spec/simulation/quadtree.md"),
    ),
    (
        "spec/simulation/quadtree.zh",
        include_str!("../../../docs/spec/simulation/quadtree.zh.md"),
    ),
    (
        "spec/simulation/rvo",
        include_str!("../../../docs/spec/simulation/rvo.md"),
    ),
    (
        "spec/simulation/rvo.zh",
        include_str!("../../../docs/spec/simulation/rvo.zh.md"),
    ),
    (
        "spec/simulation/unit-rules",
        include_str!("../../../docs/spec/simulation/unit-rules.md"),
    ),
    (
        "spec/simulation/unit-rules.zh",
        include_str!("../../../docs/spec/simulation/unit-rules.zh.md"),
    ),
];

/// Reads one topic, or lists them all.
///
/// # Errors
///
/// Returns a usage failure for an unknown topic or a name two topics share,
/// and a refusal for a language no translation is carried in.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let language = arguments.value("--lang")?;
    if arguments.is_empty() {
        return list(arguments, format);
    }
    let topic = arguments.operand("a topic")?;
    arguments.finish()?;
    let wanted = match language.as_deref() {
        None | Some("en") => topic.clone(),
        Some(code) => format!("{topic}.{code}"),
    };
    let (name, text) = find(&wanted).ok_or_else(|| missing(&wanted, language.as_deref()))?;
    if format == Format::Text {
        print!("{text}");
    } else {
        crate::cli::emit(
            &Page {
                schema: "mechcore.man-page.v1",
                topic: name,
                title: title(text),
                text,
            },
            format,
        )?;
    }
    Ok(Verdict::Yes)
}

/// The topic list, which is what `man` answers with no topic.
fn list(arguments: Args, format: Format) -> Outcome {
    arguments.finish()?;
    let topics: Vec<Topic> = TOPICS
        .iter()
        .map(|(topic, text)| Topic {
            topic,
            title: title(text),
        })
        .collect();
    if format == Format::Text {
        for topic in &topics {
            println!("{:<32}{}", topic.topic, topic.title);
        }
    } else {
        crate::cli::emit(
            &Listing {
                schema: "mechcore.man-topics.v1",
                topics,
            },
            format,
        )?;
    }
    Ok(Verdict::Yes)
}

/// The topic itself, or the one topic whose last part is this name.
fn find(wanted: &str) -> Option<(&'static str, &'static str)> {
    if let Some(page) = TOPICS.iter().find(|(topic, _)| *topic == wanted) {
        return Some(*page);
    }
    let mut matches = TOPICS
        .iter()
        .filter(|(topic, _)| topic.rsplit('/').next() == Some(wanted));
    let first = matches.next()?;
    matches.next().is_none().then_some(*first)
}

fn missing(wanted: &str, language: Option<&str>) -> Failure {
    match language {
        Some(code) if code != "en" => Failure::refused(format!(
            "no topic {wanted:?} is carried; this binary has no {code} translation of it"
        )),
        _ => Failure::usage(format!(
            "no topic {wanted:?} is carried; `mechcore man` lists them"
        )),
    }
}

/// A document's title is the first heading it opens with.
fn title(text: &str) -> &str {
    text.lines()
        .find_map(|line| line.strip_prefix("# "))
        .unwrap_or("")
        .trim()
}

#[derive(Serialize)]
struct Page {
    schema: &'static str,
    topic: &'static str,
    title: &'static str,
    text: &'static str,
}

#[derive(Serialize)]
struct Listing {
    schema: &'static str,
    topics: Vec<Topic>,
}

#[derive(Serialize)]
struct Topic {
    topic: &'static str,
    title: &'static str,
}

#[cfg(test)]
mod tests {
    use super::{TOPICS, find, title};

    /// Every document under `docs/` is a topic, so a document added without a
    /// line here is a document the binary does not carry.
    #[test]
    fn every_document_is_a_topic() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let mut found = Vec::new();
        let mut directories = vec![root.clone()];
        while let Some(directory) = directories.pop() {
            for entry in std::fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    directories.push(path);
                } else if path.extension().is_some_and(|extension| extension == "md") {
                    let relative = path.strip_prefix(&root).unwrap().with_extension("");
                    let topic = relative.display().to_string();
                    if topic != "README" {
                        found.push(topic);
                    }
                }
            }
        }
        found.sort();
        let mut carried: Vec<String> = TOPICS
            .iter()
            .map(|(topic, _)| (*topic).to_owned())
            .collect();
        carried.sort();
        assert_eq!(found, carried);
    }

    /// A topic is found by its whole name, and by its last part when that
    /// names one topic alone.
    #[test]
    fn a_topic_is_found_by_name_or_by_its_last_part() {
        assert_eq!(
            find("rules/landing").map(|(topic, _)| topic),
            Some("rules/landing")
        );
        assert_eq!(
            find("landing").map(|(topic, _)| topic),
            Some("rules/landing")
        );
        assert!(find("nothing").is_none());
        assert_eq!(title("# The title\n\nbody\n"), "The title");
    }
}
