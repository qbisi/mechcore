//! The manual the binary carries: the game's rules, and its own contracts.
//!
//! A binary is distributed on its own, so what it knows travels with it. The
//! documents are compiled in, and `mechcore man <topic>` reads one back.
//! `mechcore man <kind>` also says which verbs a file of that kind takes, from
//! the same table the verbs are refused by.

use serde::Serialize;

use crate::cli::{Args, Failure, Format, Outcome, Verdict};
use crate::kind::Kind;

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
        "rules/combat",
        include_str!("../../../docs/rules/combat.md"),
    ),
    (
        "rules/commander_skills",
        include_str!("../../../docs/rules/commander_skills.md"),
    ),
    (
        "rules/constructions",
        include_str!("../../../docs/rules/constructions.md"),
    ),
    (
        "rules/contraptions",
        include_str!("../../../docs/rules/contraptions.md"),
    ),
    (
        "rules/control",
        include_str!("../../../docs/rules/control.md"),
    ),
    (
        "rules/equipment",
        include_str!("../../../docs/rules/equipment.md"),
    ),
    (
        "rules/landing",
        include_str!("../../../docs/rules/landing.md"),
    ),
    ("rules/map", include_str!("../../../docs/rules/map.md")),
    (
        "rules/mobility",
        include_str!("../../../docs/rules/mobility.md"),
    ),
    (
        "rules/officers",
        include_str!("../../../docs/rules/officers.md"),
    ),
    (
        "rules/energy_tower_skills",
        include_str!("../../../docs/rules/energy_tower_skills.md"),
    ),
    (
        "rules/equipment_effects",
        include_str!("../../../docs/rules/equipment_effects.md"),
    ),
    (
        "rules/extra_weapons",
        include_str!("../../../docs/rules/extra_weapons.md"),
    ),
    (
        "rules/officer_effects",
        include_str!("../../../docs/rules/officer_effects.md"),
    ),
    (
        "rules/opening",
        include_str!("../../../docs/rules/opening.md"),
    ),
    (
        "rules/reinforce_items",
        include_str!("../../../docs/rules/reinforce_items.md"),
    ),
    (
        "rules/reactor_damage",
        include_str!("../../../docs/rules/reactor_damage.md"),
    ),
    (
        "rules/reinforcements",
        include_str!("../../../docs/rules/reinforcements.md"),
    ),
    (
        "rules/standalone_weapons",
        include_str!("../../../docs/rules/standalone_weapons.md"),
    ),
    (
        "rules/super_deployment",
        include_str!("../../../docs/rules/super_deployment.md"),
    ),
    ("rules/sweep", include_str!("../../../docs/rules/sweep.md")),
    (
        "rules/technology_effects",
        include_str!("../../../docs/rules/technology_effects.md"),
    ),
    (
        "rules/terrain",
        include_str!("../../../docs/rules/terrain.md"),
    ),
    (
        "rules/turrets",
        include_str!("../../../docs/rules/turrets.md"),
    ),
    (
        "rules/underground",
        include_str!("../../../docs/rules/underground.md"),
    ),
    (
        "rules/visibility",
        include_str!("../../../docs/rules/visibility.md"),
    ),
    (
        "rules/unit_experience",
        include_str!("../../../docs/rules/unit_experience.md"),
    ),
    (
        "rules/towers",
        include_str!("../../../docs/rules/towers.md"),
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
        "terminology/README",
        include_str!("../../../docs/terminology/README.md"),
    ),
    (
        "terminology/blueprints",
        include_str!("../../../docs/terminology/blueprints.md"),
    ),
    (
        "terminology/commander_skills",
        include_str!("../../../docs/terminology/commander_skills.md"),
    ),
    (
        "terminology/energy_tower_skills",
        include_str!("../../../docs/terminology/energy_tower_skills.md"),
    ),
    (
        "terminology/equipment",
        include_str!("../../../docs/terminology/equipment.md"),
    ),
    (
        "terminology/game",
        include_str!("../../../docs/terminology/game.md"),
    ),
    (
        "terminology/maps",
        include_str!("../../../docs/terminology/maps.md"),
    ),
    (
        "terminology/mechcore",
        include_str!("../../../docs/terminology/mechcore.md"),
    ),
    (
        "terminology/officers",
        include_str!("../../../docs/terminology/officers.md"),
    ),
    (
        "terminology/technologies",
        include_str!("../../../docs/terminology/technologies.md"),
    ),
    (
        "terminology/units",
        include_str!("../../../docs/terminology/units.md"),
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
        "spec/document/match",
        include_str!("../../../docs/spec/document/match.md"),
    ),
    (
        "spec/document/layout",
        include_str!("../../../docs/spec/document/layout.md"),
    ),
    (
        "spec/document/fight",
        include_str!("../../../docs/spec/document/fight.md"),
    ),
    (
        "spec/document/layout-replay",
        include_str!("../../../docs/spec/document/layout-replay.md"),
    ),
    (
        "spec/document/match-replay",
        include_str!("../../../docs/spec/document/match-replay.md"),
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
        "spec/mechcore/cli",
        include_str!("../../../docs/spec/mechcore/cli.md"),
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
        "spec/simulation/quadtree",
        include_str!("../../../docs/spec/simulation/quadtree.md"),
    ),
    (
        "spec/simulation/rvo",
        include_str!("../../../docs/spec/simulation/rvo.md"),
    ),
    (
        "spec/simulation/unit-rules",
        include_str!("../../../docs/spec/simulation/unit-rules.md"),
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
    let kind = Kind::parse(&topic).map(Verbs::of);
    let section = verb_section(&wanted);
    let page = match (find(&wanted), &kind) {
        (Some((name, text)), _) => (name, title(text), text),
        // A verb's own section of the command line's contract.
        (None, _) if section.is_some() => section.expect("checked"),
        // A kind no document describes still takes verbs.
        (None, Some(verbs)) if language.is_none() => (verbs.kind, "", ""),
        (None, _) => return Err(missing(&wanted, language.as_deref())),
    };
    let (name, title, text) = page;
    if format == Format::Text {
        if let Some(verbs) = &kind {
            println!("{}", verbs.text());
            if !text.is_empty() {
                println!();
            }
        }
        print!("{text}");
    } else {
        crate::cli::emit(
            &Page {
                schema: "mechcore.man-page.v1",
                topic: name,
                title,
                text,
                verbs: kind,
            },
            format,
        )?;
    }
    Ok(Verdict::Yes)
}

/// What a file of one kind takes: the verbs, and the kinds `convert` reaches
/// from it.
// `verbs` is what the page's JSON calls the list, beside `kind`.
#[allow(clippy::struct_field_names)]
#[derive(Serialize)]
struct Verbs {
    kind: &'static str,
    verbs: &'static [&'static str],
    converts_to: Vec<Reach>,
}

#[derive(Serialize)]
struct Reach {
    kind: &'static str,
    conversion: &'static str,
}

impl Verbs {
    fn of(kind: Kind) -> Self {
        Self {
            kind: kind.name(),
            verbs: kind.verbs(),
            converts_to: kind
                .conversions()
                .iter()
                .map(|(to, how)| Reach {
                    kind: to.name(),
                    conversion: how.name(),
                })
                .collect(),
        }
    }

    fn text(&self) -> String {
        use std::fmt::Write;
        let mut text = if self.verbs.is_empty() {
            format!("a {} file takes no verb", self.kind)
        } else {
            format!("a {} file takes {}", self.kind, self.verbs.join(", "))
        };
        for reach in &self.converts_to {
            let _ = write!(
                text,
                "\n  convert --to {:<8}{}",
                reach.kind, reach.conversion
            );
        }
        text
    }
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
    let kinds = Kind::ALL.map(Kind::name);
    if format == Format::Text {
        for topic in &topics {
            println!("{:<32}{}", topic.topic, topic.title);
        }
        println!(
            "\nfile kinds, each `man <kind>` with the verbs it takes: {}",
            kinds.join(", ")
        );
    } else {
        crate::cli::emit(
            &Listing {
                schema: "mechcore.man-topics.v1",
                topics,
                kinds,
            },
            format,
        )?;
    }
    Ok(Verdict::Yes)
}

/// A verb's section of `spec/mechcore/cli`, its heading `` ## `<verb>` `` to
/// the next section, as `(topic, title, text)`: what `man query` reads.
fn verb_section(verb: &str) -> Option<(&'static str, &'static str, &'static str)> {
    let (_, text) = TOPICS
        .iter()
        .find(|(topic, _)| *topic == "spec/mechcore/cli")?;
    let heading = format!("## `{verb}`\n");
    let start = text.find(&heading)?;
    let body = &text[start..];
    let end = body[heading.len()..]
        .find("\n## ")
        .map_or(body.len(), |at| heading.len() + at + 1);
    let section = &body[..end];
    Some(("spec/mechcore/cli", &section[3..heading.len() - 1], section))
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
    #[serde(skip_serializing_if = "Option::is_none")]
    verbs: Option<Verbs>,
}

#[derive(Serialize)]
struct Listing {
    schema: &'static str,
    topics: Vec<Topic>,
    kinds: [&'static str; Kind::ALL.len()],
}

#[derive(Serialize)]
struct Topic {
    topic: &'static str,
    title: &'static str,
}

#[cfg(test)]
mod tests {

    /// A verb no document is named after reads its section of the command
    /// line's contract, and only that section.
    #[test]
    fn a_verb_reads_its_own_section() {
        let (topic, title, text) = super::verb_section("query").expect("query has a section");
        assert_eq!(topic, "spec/mechcore/cli");
        assert_eq!(title, "`query`");
        assert!(text.starts_with("## `query`\n"));
        assert!(!text[3..].contains("\n## "));
        assert!(super::verb_section("no-such-verb").is_none());
    }

    use super::{TOPICS, Verbs, find, title};
    use crate::kind::Kind;

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

    /// A kind's page opens with the verbs it takes, and a document kind is
    /// also the topic that describes it.
    #[test]
    fn a_kind_names_the_verbs_it_takes() {
        let layout = Verbs::of(Kind::Layout).text();
        assert!(
            layout.starts_with("a layout file takes verify, convert"),
            "{layout}"
        );
        assert!(layout.contains("--to mcfr    computation"), "{layout}");
        assert!(Verbs::of(Kind::State).text().contains("no verb"));
        for kind in ["layout", "match", "state", "action", "mcfr"] {
            assert!(find(kind).is_some(), "{kind}");
        }
    }
}
