//! The document format: layout, state, turn and battle.
//!
//! One type system carries all four kinds, because they are one another's
//! parts. A battle holds turns, a turn holds a state and the decisions taken
//! from it, and a layout is the projection of a state onto what a fight
//! simulates. `docs/spec/document/battle.md`, `docs/spec/document/turn.md`, `docs/spec/document/state.md` and
//! `docs/spec/document/layout.md` define them.
//!
//! The modules are layered. [`layout`] and [`battle`] define documents,
//! [`catalog`] pins the names they use to one build, [`compile`] turns a
//! layout into a plan a scene can install, and [`record`] with [`convert`]
//! reads a replay into a battle behind the `convert` feature.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod battle;
pub mod catalog;
pub mod compile;
#[cfg(feature = "convert")]
pub mod convert;
pub mod economy;
mod grbr;
pub mod layout;
pub mod ledger;
#[cfg(feature = "convert")]
pub mod observe;
pub mod opening;
#[cfg(feature = "convert")]
pub mod oracle;
pub mod project;
pub mod transition;
#[cfg(feature = "convert")]
pub mod record;
pub mod reinforcement;

pub use catalog::{
    NativeFormation, battle_skill_type_from_id, construction_type_from_id,
    contraption_type_from_id, unit_type_from_id,
};
pub use compile::{BattleSkill, Placement, Plan, SidePlan, compile, compile_layout};
pub use grbr::{GrbrRoundRetained, GrbrSideRetained, retained_from_grbr_round};
pub use layout::{
    BattleSkillDefinition, ContraptionPlacement, FIGHT_VISIBLE_ENERGY_TOWER_SKILLS, Formation,
    Layout, MAX_TOWER_STRENGTHEN_LEVEL, MOVEMENT_ENHANCEMENT_SKILL, Position,
    RANGE_ENHANCEMENT_SKILL, Region, Side, Sides, StaticPlacement, TOWER_COUNT, Techs, Terrain,
    TerrainType, canonical_embedded_yaml, canonical_yaml, parse_embedded_yaml, parse_yaml,
};

/// Names the kind of document a file carries.
///
/// The four kinds share most of their shape, since they are one another's
/// parts: a turn carries a state beside its actions, and a state carries what a
/// layout projects. Structure alone cannot say which one a file holds, so every
/// document names itself. The tag is a constant, not a version: it never needs
/// maintaining, and because it takes one value within a kind it cannot split
/// one state across two documents.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Layout,
    Battle,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{
        resolve_battle_skill_type, resolve_construction_type, resolve_contraption_type,
        resolve_unit_type,
    };
    use crate::compile::{grid_center_remainder, placement_footprint};
    use serde_json::{Value, json};

    fn yaml_fixture(source: &str) -> Value {
        serde_yaml::from_str(source).unwrap()
    }

    fn layout_with_blue_battle_skill(type_name: &str, positions: impl serde::Serialize) -> Value {
        json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}],
                    "battle_skills": [{"type": type_name, "positions": positions}]
                },
                "red": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                }
            }
        })
    }

    fn layout_with_blue_terrains(terrains: &Value) -> Value {
        json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}],
                    "terrains": terrains
                },
                "red": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                }
            }
        })
    }

    #[test]
    fn a_document_of_another_kind_is_rejected_as_that_kind() {
        // The point of the discriminator is that this reads as "not a layout"
        // rather than as a layout with an unexpected field. A turn carries a
        // partial layout, so the shapes overlap and the first structural
        // difference would say nothing about what the file is.
        let turn = br"
kind: turn
round: 1
actions:
  - type: deploy
sides:
  blue:
    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]
  red:
    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]
";
        let error = parse_embedded_yaml(turn).unwrap_err();
        assert_eq!(error, "expected a layout document, found kind \"turn\"");

        let untagged = br"
round: 1
sides:
  blue:
    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]
  red:
    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]
";
        assert_eq!(
            parse_embedded_yaml(untagged).unwrap_err(),
            "document does not name its kind: a layout starts with `kind: layout`"
        );

        // The JSON entry point that MCP publishes answers the same way.
        assert_eq!(
            compile(&json!({"kind": "turn", "round": 1, "sides": {}})).unwrap_err(),
            "expected a layout document, found kind \"turn\""
        );

        // Malformed YAML still reports the syntax problem, not the kind.
        assert!(
            parse_embedded_yaml(b"kind: layout\nround: [\n")
                .unwrap_err()
                .starts_with("invalid layout YAML:")
        );
    }

    #[test]
    fn requires_a_positive_round() {
        let missing = compile(&json!({
            "kind": "layout",
            "sides": {
                "blue": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap_err();
        assert!(missing.contains("missing field `round`"));

        let invalid = compile(&json!({
            "kind": "layout",
            "round": 0,
            "sides": {
                "blue": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap_err();
        assert_eq!(invalid, "layout round must be at least 1");

        // The staging budget belongs to the executor, so the schema accepts a
        // round that no Training Ground run can reach.
        assert_eq!(
            compile(&json!({
                "kind": "layout",
                "round": 40,
                "sides": {
                    "blue": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]},
                    "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
                }
            }))
            .unwrap()
            .round,
            40
        );
    }

    /// Every coordinate pair the schema holds is written on one line.
    ///
    /// A pair is one value. The writer used to fold only a placement's
    /// `position`, so the three collections of bare pairs came out three or
    /// more lines each and a generated document disagreed with the hand-written
    /// fixtures beside it.
    #[test]
    fn every_coordinate_pair_is_written_on_one_line() {
        let layout: Layout = serde_json::from_value(json!({
            "kind": "layout",
            "round": 2,
            "sides": {
                "blue": {
                    "formations": [
                        {"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}
                    ],
                    "airdrop_shields": [{"x": -200, "y": -20}],
                    "terrains": [
                        {"type": "oil", "control_points": [{"x": 100, "y": 0}, {"x": 120, "y": 0}]}
                    ],
                    "battle_skills": [
                        {"type": "lightning_storm", "positions": [{"x": 20, "y": -150}]}
                    ]
                },
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap();

        let yaml = canonical_yaml(layout).unwrap();
        for pair in [
            "      position: {x: 0, y: -50}\n",
            "    airdrop_shields:\n    - {x: -200, y: -20}\n",
            "      control_points:\n      - {x: 100, y: 0}\n      - {x: 120, y: 0}\n",
            "      positions:\n      - {x: 20, y: -150}\n",
        ] {
            assert!(yaml.contains(pair), "{pair:?} is not folded: {yaml}");
        }
        assert!(!yaml.contains("\n      y:"), "a pair was left open: {yaml}");
        assert_eq!(
            canonical_yaml(parse_yaml(yaml.as_bytes()).unwrap()).unwrap(),
            yaml,
            "folding a folded document changes nothing"
        );
    }

    #[test]
    fn normalization_reaches_its_fixed_point_in_one_pass() {
        let denormalized: Layout = serde_json::from_value(json!({
            "kind": "layout",
            "round": 2,
            "sides": {
                "blue": {
                    "techs": {"officers": [20039, 10014], "units": [10202, 10101]},
                    "formations": [
                        {"index": 4, "type": "marksman", "position": {"x": 40, "y": -50}, "level": 1},
                        {"index": 1, "type": "marksman", "position": {"x": 0, "y": -50},
                         "exp": 0, "rotated": false, "travelling": false}
                    ],
                    "contraptions": [
                        {"index": 3, "type": "shield", "position": {"x": 0, "y": -120}},
                        {"index": 2, "type": "shield", "position": {"x": 100, "y": -120}}
                    ],
                    "airdrop_shields": [{"x": 200, "y": 20}, {"x": -200, "y": 20}],
                    "terrains": [
                        {"type": "oil", "control_points": [{"x": 100, "y": 0}, {"x": 120, "y": 0}]},
                        {"type": "oil", "control_points": [{"x": -100, "y": 0}, {"x": -80, "y": 0}]}
                    ]
                },
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap();

        let once = denormalized.normalized();
        assert_eq!(once.sides.blue.techs.officers, [10014, 20039]);
        assert_eq!(once.sides.blue.techs.units, [10101, 10202]);
        assert_eq!(
            once.sides
                .blue
                .formations
                .iter()
                .map(|formation| formation.index)
                .collect::<Vec<_>>(),
            [1, 4]
        );
        assert_eq!(
            once.sides
                .blue
                .contraptions
                .iter()
                .map(|contraption| contraption.index)
                .collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(
            once.sides.blue.airdrop_shields,
            [Position { x: -200, y: 20 }, Position { x: 200, y: 20 }]
        );
        assert_eq!(
            once.sides
                .blue
                .terrains
                .iter()
                .map(|terrain| terrain.control_points[0].x)
                .collect::<Vec<_>>(),
            [-100, 100]
        );
        assert!(once.sides.blue.formations[0].level.is_none());
        assert!(once.sides.blue.formations[0].exp.is_none());
        assert!(once.sides.blue.formations[0].rotated.is_none());
        assert!(once.sides.blue.formations[0].travelling.is_none());

        assert_eq!(once.clone().normalized(), once);
    }

    #[test]
    fn tracked_layouts_are_normal_and_normalize_idempotently() {
        let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/layouts");
        let mut checked = 0;
        for entry in std::fs::read_dir(directory).expect("tracked layout directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().is_none_or(|extension| extension != "yaml") {
                continue;
            }
            let bytes = std::fs::read(&path).expect("readable layout");
            let layout = parse_yaml(&bytes).unwrap_or_else(|error| {
                panic!("{} does not parse: {error}", path.display());
            });
            assert_eq!(
                layout.clone(),
                layout.clone().normalized(),
                "{} is not in normal form",
                path.display()
            );
            let once = layout.normalized();
            let twice = once.clone().normalized();
            assert_eq!(
                once,
                twice,
                "{} is not a normalization fixed point",
                path.display()
            );
            checked += 1;
        }
        assert!(checked > 0, "no tracked layouts were read from {directory}");
    }

    #[test]
    fn map_id_is_optional_positive_and_preserved() {
        let mut value = json!({"kind": "layout", "round": 1, "sides": {
            "blue": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]},
            "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
        }});
        assert_eq!(compile(&value).unwrap().map_id, None);
        for id in [1001, 1021] {
            value["map_id"] = json!(id);
            assert_eq!(compile(&value).unwrap().map_id, Some(id));
            let layout: Layout = serde_json::from_value(value.clone()).unwrap();
            let yaml = canonical_yaml(layout).unwrap();
            assert!(yaml.contains(&format!("map_id: {id}")));
        }
        for id in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("1021"),
            json!(2_147_483_648_i64),
        ] {
            value["map_id"] = id;
            assert!(compile(&value).is_err());
        }
    }

    #[test]
    fn leaves_an_unspecified_seed_absent_and_rejects_the_zero_sentinel() {
        let layout = |seed| {
            let mut value = json!({
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]},
                    "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
                }
            });
            if let Some(seed) = seed {
                value["seed"] = json!(seed);
            }
            value
        };

        assert_eq!(compile(&layout(None)).unwrap().seed, None);
        assert_eq!(compile(&layout(Some(-17))).unwrap().seed, Some(-17));
        assert!(
            compile(&layout(Some(0)))
                .unwrap_err()
                .contains("native system-random request")
        );
    }

    #[test]
    fn compiles_and_counts_valid_oil_terrain_state() {
        let plan = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": -60, "y": 40}, {"x": 60, "y": 40}]},
            {"type": "oil", "control_points": [{"x": -60, "y": 40}, {"x": 60, "y": 40}], "grid_rows": {"0": [], "1": vec![0x0fff_u32; 12]}}
        ])))
        .unwrap();

        assert_eq!(plan.terrain_count(), 2);
        assert_eq!(plan.blue.terrains[0].terrain_type, TerrainType::Oil);
        assert!(plan.blue.terrains[0].grid_rows.is_empty());
        assert!(plan.blue.terrains[1].grid_rows[&0].is_empty());
        assert_eq!(plan.blue.terrains[1].grid_rows[&1], vec![0x0fff; 12]);
        assert!(plan.red.terrains.is_empty());
    }

    #[test]
    fn terrain_fields_and_grid_shape_are_fail_closed() {
        // A substance the build makes but this one has no measured geometry
        // for parses and is then refused by name, so a recording holding one
        // can be described even though no plan can be built from it.
        let unmeasured = compile(&layout_with_blue_terrains(&json!([
            {"type": "fire", "control_points": [{"x": 0, "y": 0}, {"x": 60, "y": 0}]}
        ])))
        .unwrap_err();
        assert!(
            unmeasured.contains("has no measured point radius or count"),
            "{unmeasured}"
        );

        // A substance the build does not make at all is still a parse error.
        let unknown = compile(&layout_with_blue_terrains(&json!([
            {"type": "tar", "control_points": [{"x": 0, "y": 0}, {"x": 60, "y": 0}]}
        ])))
        .unwrap_err();
        assert!(unknown.contains("unknown variant `tar`"), "{unknown}");

        let outside = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": 431, "y": 0}, {"x": 500, "y": 0}]}
        ])))
        .unwrap_err();
        assert!(outside.contains("does not overlap the battlefield"));

        let wrong_count = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": 0, "y": 0}]}
        ])))
        .unwrap_err();
        assert!(wrong_count.contains("control_points must contain exactly two points"));

        let wrong_height = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": 0, "y": 0}, {"x": 60, "y": 0}], "grid_rows": {"1": vec![1_u32; 11]}}
        ])))
        .unwrap_err();
        assert!(wrong_height.contains("exactly 12 rows"));

        let outside_width = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": 0, "y": 0}, {"x": 60, "y": 0}], "grid_rows": {"1": vec![0x1000_u32; 12]}}
        ])))
        .unwrap_err();
        assert!(outside_width.contains("uses bits outside width 12"));

        let empty_grid = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": 0, "y": 0}, {"x": 60, "y": 0}], "grid_rows": {"1": vec![0_u32; 12]}}
        ])))
        .unwrap_err();
        assert!(empty_grid.contains("must activate at least one cell"));

        let outside_index = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": 0, "y": 0}, {"x": 60, "y": 0}], "grid_rows": {"7": []}}
        ])))
        .unwrap_err();
        assert!(outside_index.contains("point index 7 must be within 0..6"));
    }

    #[test]
    fn embedded_layout_keeps_placement_categories_explicit() {
        let error = parse_embedded_yaml(
            br"
kind: layout
round: 1
sides:
  blue:
    formations:
      - {type: defensive_wall, index: 0, position: {x: 140, y: -105}}
  red:
    formations:
      - {type: marksman, index: 0, position: {x: 0, y: -50}}
",
        )
        .unwrap_err();
        assert_eq!(
            error,
            "side blue formation type \"defensive_wall\" belongs in constructions"
        );
    }

    #[test]
    fn compiles_formations_in_declaration_order() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}},
                    {"index": 1, "type": "marksman", "position": {"x": -310, "y": 20}},
                    {"index": 2, "type": "arclight", "position": {"x": 310, "y": 20}, "travelling": true}
                ], "contraptions": [
                    {"index": 0, "type": "interceptor", "position": {"x": 5, "y": -85}}
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap();

        assert_eq!(plan.round, 3);
        let types = plan
            .blue
            .formations
            .iter()
            .map(|placement| placement.type_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(types, ["marksman", "marksman", "arclight"]);
        assert!(!plan.blue.formations[1].travelling);
        assert!(plan.blue.formations[2].travelling);
        assert_eq!(plan.blue.contraptions[0].type_name, "interceptor");
    }

    #[test]
    fn enforces_round_rules_for_ambush_units() {
        let layout = |round, travelling| {
            json!({
                "kind": "layout",
                "round": round,
                "sides": {
                    "blue": {"formations": [
                        {"index": 0, "type": "marksman", "position": {"x": -310, "y": 20}, "travelling": travelling}
                    ]},
                    "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
                }
            })
        };

        assert!(
            compile(&layout(1, false))
                .unwrap_err()
                .contains("in round 1")
        );
        assert!(
            compile(&layout(1, true))
                .unwrap_err()
                .contains("in round 1")
        );
        assert!(
            compile(&layout(2, false))
                .unwrap_err()
                .contains("first flank deployment")
        );
        let omitted = compile(&json!({
            "kind": "layout",
            "round": 2,
            "sides": {
                "blue": {"formations": [{"index": 0, "type": "marksman", "position": {"x": -310, "y": 20}}]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap_err();
        assert!(omitted.contains("first flank deployment"));
        assert!(compile(&layout(2, true)).unwrap().blue.formations[0].travelling);
        assert!(!compile(&layout(3, false)).unwrap().blue.formations[0].travelling);
    }

    #[test]
    fn rejects_round_2_ambush_fixture_that_omits_travelling() {
        let value = yaml_fixture(include_str!(
            "../tests/fixtures/invalid-round-2-ambush-travelling.yaml"
        ));
        let error = compile(&value).unwrap_err();
        assert_eq!(
            error,
            "side blue formation type \"marksman\" at (-310, 20) must set travelling=true: a round 2 ambush unit is always a first flank deployment"
        );
    }

    #[test]
    fn travelling_requires_an_ambush_position() {
        let main_error = compile(&json!({
            "kind": "layout",
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}, "travelling": true}
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap_err();
        assert!(main_error.contains("travelling=true outside the ambush zones"));
    }

    #[test]
    fn ambush_unit_footprint_must_fit_one_flank_region() {
        let error = compile(&json!({
            "kind": "layout",
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"index": 0, "type": "marksman", "position": {"x": -300, "y": 20}, "travelling": true}
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap_err();
        assert!(error.contains("must fit completely inside one ambush zone"));
    }

    #[test]
    fn ambush_region_orientation_changes_the_effective_unit_footprint() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"index": 0,
                        "type": "crawler", "position": {"x": 325, "y": 60},
                        "rotated": true, "travelling": true
                    }
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap();
        assert_eq!(
            placement_footprint(&plan.blue.formations[0]),
            Some((50, 20))
        );

        let error = compile(&json!({
            "kind": "layout",
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"index": 0,
                        "type": "crawler", "position": {"x": 310, "y": 65},
                        "rotated": true, "travelling": true
                    }
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap_err();
        assert!(error.contains("footprint 50x20 requires center x≡5, y≡0"));
    }

    #[test]
    fn compiles_unit_defaults_for_both_sides() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [{"index": 0,
                    "type": "marksman",
                    "position": {"x": -20, "y": -50}
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman",
                    "position": {"x": 20, "y": -180}
                }]}
            }
        }))
        .unwrap();

        assert_eq!(plan.blue.formations[0].level, Some(1));
        assert_eq!(plan.blue.formations[0].index, Some(0));
        assert_eq!(plan.blue.formations[0].exp, Some(0));
        assert!(!plan.blue.formations[0].rotated);
        assert_eq!(plan.blue.formations[0].equipment, None);
        assert_eq!(plan.blue.formations[0].type_name, "marksman");
        assert_eq!(plan.blue.formations[0].native, NativeFormation::Unit(2));
        assert_eq!(plan.red.formations[0].position, Position { x: 20, y: -180 });
        assert_eq!(plan.formation_count(), 2);
    }

    #[test]
    fn compiles_stable_unit_indices_and_experience() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [
                    {"type": "marksman", "index": 0, "position": {"x": -20, "y": -50}, "exp": 7},
                    {"type": "marksman", "index": 2, "position": {"x": 20, "y": -50}}
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -180}}]}
            }
        }))
        .unwrap();

        assert_eq!(plan.blue.formations[0].index, Some(0));
        assert_eq!(plan.blue.formations[0].exp, Some(7));
        assert_eq!(plan.blue.formations[1].index, Some(2));
        assert_eq!(plan.blue.formations[1].exp, Some(0));

        let duplicate = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [
                    {"type": "marksman", "index": 1, "position": {"x": -20, "y": -50}},
                    {"type": "marksman", "index": 1, "position": {"x": 20, "y": -50}}
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -180}}]}
            }
        }))
        .unwrap_err();
        assert!(duplicate.contains("indices must be strictly increasing"));

        let negative_exp = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [
                    {"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}, "exp": -1}
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -180}}]}
            }
        }))
        .unwrap_err();
        assert!(negative_exp.contains("exp must be non-negative"));
    }

    #[test]
    fn compiles_one_equipment_for_a_unit() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50},
                    "equipment": 13_030_001
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap();

        assert_eq!(plan.blue.formations[0].equipment, Some(13_030_001));
        assert_eq!(plan.red.formations[0].equipment, None);
    }

    #[test]
    fn constructions_reject_unit_only_fields() {
        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 100, "y": -50}}],
                    "constructions": [{"index": 0, "type": "defensive_wall", "position": {"x": 0, "y": -55},
                     "equipment": 13_030_001}]
                },
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap_err();

        assert!(error.contains("unknown field `equipment`"));
    }

    #[test]
    fn rejects_nonpositive_equipment_id() {
        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}, "equipment": 0
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap_err();

        assert_eq!(
            error,
            "side blue formation type \"marksman\" at (0, -50) equipment must be a positive integer"
        );
    }

    #[test]
    fn validates_tech_ids() {
        let valid = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "techs": {"officers": [30602], "units": [10202]},
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                },
                "red": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                }
            }
        }))
        .unwrap();
        assert_eq!(valid.blue.techs.officers, [30602]);
        assert_eq!(valid.blue.techs.units, [10202]);

        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"techs": {"units": [10202, 10202]}, "formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap_err();
        assert_eq!(error, "side blue techs.units contains duplicate ID 10202");

        // An Officer is the other way round. A card that may be taken again
        // stacks, and two copies of Advanced Offensive Tactics are +60% damage,
        // so a layout has to be able to say it.
        let repeated = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"techs": {"officers": [20002, 20002]}, "formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap();
        assert_eq!(repeated.blue.techs.officers, [20002, 20002]);

        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"techs": {"officers": [0]}, "formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap_err();
        assert_eq!(
            error,
            "side blue techs.officers[0] must be a positive integer"
        );
    }

    #[test]
    fn rejects_tower_levels_outside_the_runtime_catalog() {
        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "tower_strengthen_levels": [0, 5],
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                },
                "red": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                }
            }
        }))
        .unwrap_err();
        assert_eq!(error, "side blue tower_strengthen_levels[1] must be 0..=4");
    }

    #[test]
    fn rejects_a_tower_list_that_does_not_name_every_tower() {
        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "tower_strengthen_levels": [1],
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                },
                "red": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                }
            }
        }))
        .unwrap_err();
        assert_eq!(
            error,
            "side blue tower_strengthen_levels must hold 2 levels, one per fixed tower, or none at all"
        );
    }

    #[test]
    fn rejects_an_energy_tower_skill_no_fight_can_see() {
        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "energy_tower_skills": [1],
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                },
                "red": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                }
            }
        }))
        .unwrap_err();
        assert_eq!(
            error,
            "side blue energy_tower_skills[0] is 1, which no fight can see: a layout carries [5, 6]"
        );
    }

    #[test]
    fn rejects_construction_types_in_formations() {
        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [{"index": 0,
                    "type": "defensive_wall",
                    "position": {"x": 0, "y": -55}
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap_err();
        assert_eq!(
            error,
            "side blue formation type \"defensive_wall\" at (0, -55) belongs in constructions"
        );
    }

    #[test]
    fn requires_nonempty_formations_for_both_sides() {
        let missing = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap_err();
        assert!(missing.contains("missing field `formations`"));

        let empty = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": []},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap_err();
        assert!(empty.contains("at least one valid unit"));
    }

    #[test]
    fn rejects_positive_area_unit_overlap_but_allows_edge_contact() {
        let layout = |second_x| {
            json!({
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {"formations": [
                        {"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}},
                        {"index": 1, "type": "arclight", "position": {"x": second_x, "y": -50}}
                    ]},
                    "red": {"formations": [{"index": 0,
                        "type": "marksman", "position": {"x": 0, "y": -50}
                    }]}
                }
            })
        };

        let error = compile(&layout(10)).unwrap_err();
        assert!(error.contains("placements collide"));
        assert!(error.contains("marksman"));
        assert!(error.contains("arclight"));
        compile(&layout(20)).unwrap();
    }

    #[test]
    fn rejects_fixture_whose_center_is_inside_but_footprint_crosses_boundary() {
        let value = yaml_fixture(include_str!(
            "../tests/fixtures/invalid-footprint-boundary.yaml"
        ));
        let error = compile(&value).unwrap_err();
        assert_eq!(
            error,
            "side blue placement type \"sledgehammer\" at (285, -60) footprint 50x20 exceeds the main deployment boundary x=[-300,300], y=[-310,-10]"
        );
    }

    #[test]
    fn rejects_collision_fixture_and_names_both_units() {
        let value = yaml_fixture(include_str!(
            "../tests/fixtures/invalid-unit-collision.yaml"
        ));
        let error = compile(&value).unwrap_err();
        assert_eq!(
            error,
            "placements collide: blue type \"sledgehammer\" at (5, -100) and blue type \"crawler\" at (20, -95)"
        );
    }

    #[test]
    fn rejects_unit_construction_collision_and_names_both_placements() {
        let value = yaml_fixture(include_str!(
            "../tests/fixtures/invalid-unit-construction-collision.yaml"
        ));
        let error = compile(&value).unwrap_err();
        assert_eq!(
            error,
            "placements collide: blue type \"marksman\" at (140, -100) and blue type \"defensive_wall\" at (140, -105)"
        );
    }

    #[test]
    fn rejects_center_that_does_not_match_its_footprint_grid_class() {
        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 5, "y": -50}
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap_err();
        assert_eq!(
            error,
            "side blue placement type \"marksman\" at (5, -50) is off the native 10x10 grid: footprint 20x20 requires center x≡0, y≡0 (mod 10)"
        );
    }

    #[test]
    fn derives_center_grid_class_from_footprint_dimension() {
        assert_eq!(grid_center_remainder(20), Some(0));
        assert_eq!(grid_center_remainder(40), Some(0));
        assert_eq!(grid_center_remainder(30), Some(5));
        assert_eq!(grid_center_remainder(50), Some(5));
        assert_eq!(grid_center_remainder(70), Some(5));
        assert_eq!(grid_center_remainder(25), None);
    }

    #[test]
    fn formation_specs_cover_all_public_unit_footprints() {
        let groups: &[(&[&str], (i64, i64))] = &[
            (&["marksman", "arclight", "vortex"], (20, 20)),
            (
                &[
                    "rhino",
                    "hacker",
                    "wraith",
                    "scorpion",
                    "sabertooth",
                    "tarantula",
                    "farseer",
                ],
                (30, 30),
            ),
            (&["phoenix", "typhoon", "hound", "void_eye"], (40, 20)),
            (
                &["fortress", "vulcan", "melting_point", "sandworm", "raiden"],
                (40, 40),
            ),
            (
                &[
                    "wasp",
                    "mustang",
                    "steel_ball",
                    "fang",
                    "crawler",
                    "stormcaller",
                    "sledgehammer",
                    "fire_badger",
                    "phantom_ray",
                ],
                (50, 20),
            ),
            (&["overlord"], (50, 50)),
            (&["war_factory", "abyss", "mountain"], (70, 70)),
        ];
        for &(type_names, footprint) in groups {
            for &type_name in type_names {
                let spec = resolve_unit_type(type_name).unwrap();
                assert!(matches!(spec.native, NativeFormation::Unit(_)));
                assert_eq!(spec.footprint, Some(footprint));
            }
        }
        assert_eq!(
            groups.iter().map(|(names, _)| names.len()).sum::<usize>(),
            32
        );
    }

    #[test]
    fn construction_specs_cover_the_four_constructions() {
        for (type_name, footprint) in [
            ("defensive_wall", (60, 10)),
            ("anti_armor_turret", (20, 20)),
            ("rapid_fire_turret", (20, 20)),
            ("magnetic_barrier", (50, 10)),
        ] {
            let spec = resolve_construction_type(type_name).unwrap();
            assert!(matches!(spec.native, NativeFormation::Construction(_)));
            assert_eq!(spec.footprint, Some(footprint));
        }
    }

    #[test]
    fn contraption_specs_cover_all_public_types() {
        assert_eq!(
            resolve_contraption_type("interceptor").unwrap().footprint,
            Some((30, 30))
        );
        assert_eq!(resolve_contraption_type("shield").unwrap().footprint, None);
        assert_eq!(resolve_contraption_type("missile").unwrap().footprint, None);
        assert_eq!(contraption_type_from_id(10_001), Some("shield"));
        assert_eq!(contraption_type_from_id(20_001), Some("missile"));
        assert_eq!(contraption_type_from_id(30_001), Some("interceptor"));
        assert_eq!(contraption_type_from_id(0), None);
    }

    #[test]
    fn battle_skill_reverse_catalog_matches_the_compiler_catalog() {
        for id in [
            100_002, 200_001, 200_002, 200_003, 300_001, 300_003, 300_004, 300_005, 300_006,
            300_007, 400_002, 500_002, 600_002, 800_001, 1_200_001, 1_200_002, 1_200_003,
            1_200_004, 1_200_005, 1_500_002,
        ] {
            let type_name = battle_skill_type_from_id(id).unwrap();
            assert_eq!(
                resolve_battle_skill_type(type_name)
                    .unwrap()
                    .commander_skill_id,
                id
            );
        }
        assert_eq!(battle_skill_type_from_id(1_500_001), Some("mobile_beacon"));
        assert_eq!(
            resolve_battle_skill_type(battle_skill_type_from_id(1_500_001).unwrap())
                .unwrap()
                .commander_skill_id,
            1_500_002
        );
        assert_eq!(battle_skill_type_from_id(0), None);
    }

    #[test]
    fn compiles_interceptor_and_reuses_formation_collision_validation() {
        let layout = |interceptor_y| {
            json!({
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -100}}],
                        "contraptions": [{"index": 0, "type": "interceptor", "position": {"x": 5, "y": interceptor_y}}]
                    },
                    "red": {"formations": [{"index": 0,
                        "type": "marksman", "position": {"x": 0, "y": -50}
                    }]}
                }
            })
        };

        let error = compile(&layout(-95)).unwrap_err();
        assert_eq!(
            error,
            "placements collide: blue type \"marksman\" at (0, -100) and blue type \"interceptor\" at (5, -95)"
        );

        let plan = compile(&layout(-125)).unwrap();
        assert_eq!(
            plan.blue.contraptions[0].native,
            NativeFormation::Contraption(30001)
        );
        assert_eq!(plan.blue.contraptions[0].level, None);
        assert!(!plan.blue.contraptions[0].rotated);
    }

    #[test]
    fn shield_and_missile_skip_grid_and_collision_validation() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -100}}],
                    "contraptions": [
                    {"index": 0, "type": "shield", "position": {"x": 1, "y": -101}},
                    {"index": 1, "type": "missile", "position": {"x": 1, "y": -101}}
                ]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -100}
                }]}
            }
        }))
        .unwrap();

        assert_eq!(plan.formation_count(), 2);
        assert_eq!(plan.contraption_count(), 2);
        assert_eq!(
            plan.blue
                .contraptions
                .iter()
                .map(|placement| placement.native)
                .collect::<Vec<_>>(),
            [
                NativeFormation::Contraption(10001),
                NativeFormation::Contraption(20001),
            ]
        );
    }

    #[test]
    fn retained_airdrop_shields_are_their_own_collection() {
        let mut value = json!({"kind": "layout", "round": 2, "sides": {
            "blue": {"formations": [{"index": 0, "type":"marksman","position": {"x": 0, "y": -150}}],
                "contraptions": [{"index": 0, "type":"shield","position": {"x": 0, "y": -120}}],
                "airdrop_shields": [{"x":300,"y":20}, {"x":-300,"y":20}]},
            "red": {"formations": [{"index": 0, "type":"marksman","position": {"x": 0, "y": -150}}]}}});
        let plan = compile(&value).unwrap();
        assert_eq!(plan.blue.contraptions.len(), 1);
        assert_eq!(plan.airdrop_shield_count(), 2);
        assert_eq!(
            plan.blue.airdrop_shields,
            [Position { x: 300, y: 20 }, Position { x: -300, y: 20 }]
        );

        // A retained airdrop stands where it was released, not where a new
        // contraption could be placed, so only the battlefield bounds apply.
        value["sides"]["blue"]["airdrop_shields"][0]["x"] = json!(401);
        assert!(
            compile(&value)
                .unwrap_err()
                .contains("airdrop_shields[0] center (401, 20) is outside the battlefield")
        );
    }

    #[test]
    fn contraptions_reject_the_retired_isairdrop_field() {
        for kind in ["shield", "missile", "interceptor"] {
            let value = json!({"kind": "layout", "round":1,"sides":{
                "blue":{"formations":[{"index": 0, "type":"marksman","position": {"x": 0, "y": -150}}],
                    "contraptions":[{"index": 0, "type":kind,"position": {"x": 5, "y": -95},"isairdrop":false}]},
                "red":{"formations":[{"index": 0, "type":"marksman","position": {"x": 0, "y": -150}}]}}});
            assert!(compile(&value).unwrap_err().contains("unknown field"));
        }
    }

    #[test]
    fn shield_requires_its_complete_edge_inside_the_own_side() {
        let layout = |x, y| {
            json!({
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -150}}],
                        "contraptions": [{"index": 0, "type": "shield", "position": {"x": x, "y": y}}]
                    },
                    "red": {"formations": [{"index": 0,
                        "type": "marksman", "position": {"x": 0, "y": -150}
                    }]}
                }
            })
        };

        compile(&layout(230, -80)).unwrap();
        compile(&layout(-230, -240)).unwrap();
        let error = compile(&layout(231, -79)).unwrap_err();
        assert_eq!(
            error,
            "side blue placement type \"shield\" at (231, -79) places its radius-70 edge outside the own-side deployment boundary: center must be within x=[-230,230], y=[-240,-80]"
        );
    }

    #[test]
    fn missile_requires_only_its_center_inside_the_own_side() {
        let layout = |x, y| {
            json!({
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -150}}],
                        "contraptions": [{"index": 0, "type": "missile", "position": {"x": x, "y": y}}]
                    },
                    "red": {"formations": [{"index": 0,
                        "type": "marksman", "position": {"x": 0, "y": -150}
                    }]}
                }
            })
        };

        compile(&layout(300, -10)).unwrap();
        compile(&layout(-300, -310)).unwrap();
        let error = compile(&layout(301, -10)).unwrap_err();
        assert_eq!(
            error,
            "side blue placement type \"missile\" at (301, -10) is outside the own-side deployment boundary x=[-300,300], y=[-310,-10]"
        );
    }

    #[test]
    fn compiles_all_four_construction_types_in_document_order() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -150}}],
                    "constructions": [
                    {"index": 0, "type": "rapid_fire_turret", "position": {"x": 140, "y": -60}},
                    {"index": 1, "type": "defensive_wall", "position": {"x": 140, "y": -105}},
                    {"index": 2, "type": "anti_armor_turret", "position": {"x": -140, "y": -60}},
                    {"index": 3, "type": "magnetic_barrier", "position": {"x": -165, "y": -105}}
                ]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap();

        assert_eq!(plan.formation_count(), 2);
        assert_eq!(plan.construction_count(), 4);
        assert_eq!(
            plan.blue
                .constructions
                .iter()
                .map(|placement| placement.index)
                .collect::<Vec<_>>(),
            [Some(0), Some(1), Some(2), Some(3)]
        );
        assert_eq!(
            plan.blue
                .constructions
                .iter()
                .map(|placement| placement.native)
                .collect::<Vec<_>>(),
            [
                NativeFormation::Construction(3),
                NativeFormation::Construction(1),
                NativeFormation::Construction(2),
                NativeFormation::Construction(4),
            ]
        );
    }

    #[test]
    fn construction_and_contraption_indices_are_required_and_ordered() {
        let layout = |constructions: Value, contraptions: Value| {
            json!({
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -150}}],
                        "constructions": constructions,
                        "contraptions": contraptions
                    },
                    "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
                }
            })
        };
        let wall = |index: i32| json!({"index": index, "type": "defensive_wall", "position": {"x": 140, "y": -105}});
        let shield = |index: i32, x: i32| json!({"index": index, "type": "shield", "position": {"x": x, "y": -95}});

        let plan = compile(&layout(
            json!([wall(7)]),
            json!([shield(2, 5), shield(9, -105)]),
        ))
        .unwrap();
        assert_eq!(plan.blue.constructions[0].index, Some(7));
        assert_eq!(
            plan.blue
                .contraptions
                .iter()
                .map(|placement| placement.index)
                .collect::<Vec<_>>(),
            [Some(2), Some(9)]
        );

        // An index survives serialization, because it is the object's identity
        // rather than a position in the list.
        let definition: Layout =
            serde_json::from_value(layout(json!([wall(7)]), json!([]))).unwrap();
        let encoded = serde_json::to_value(definition).unwrap();
        assert_eq!(encoded["sides"]["blue"]["constructions"][0]["index"], 7);

        let missing = json!({"type": "defensive_wall", "position": {"x": 140, "y": -105}});
        assert!(
            compile(&layout(json!([missing]), json!([])))
                .unwrap_err()
                .contains("missing field `index`")
        );
        assert!(
            compile(&layout(json!([]), json!([shield(9, 5), shield(2, -105)])))
                .unwrap_err()
                .contains("contraption indices must be strictly increasing")
        );
        assert!(
            compile(&layout(json!([wall(-1)]), json!([])))
                .unwrap_err()
                .contains("construction index must be non-negative")
        );
    }

    #[test]
    fn compiles_position_targeted_battle_skills_in_document_order() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}],
                    "battle_skills": [{
                        "type": "missile_strike",
                        "positions": [{"x": 55, "y": 60}]
                    }]
                },
                "red": {
                    "formations": [{"index": 0, "type": "fang", "position": {"x": -55, "y": -60}}],
                    "battle_skills": [{
                        "type": "mobile_beacon",
                        "positions": [
                            {"x": -55, "y": -60},
                            {"x": -105, "y": -90},
                            {"x": -105, "y": 20}
                        ]
                    }]
                }
            }
        }))
        .unwrap();

        assert_eq!(plan.blue.battle_skills[0].commander_skill_id, 300_001);
        assert_eq!(
            plan.red.battle_skills[0],
            BattleSkill {
                type_name: "mobile_beacon".into(),
                commander_skill_id: 1_500_002,
                positions: vec![
                    Position { x: -55, y: -60 },
                    Position { x: -105, y: -90 },
                    Position { x: -105, y: 20 },
                ],
            }
        );
    }

    #[test]
    fn battle_skill_circle_requires_only_map_overlap() {
        compile(&layout_with_blue_battle_skill(
            "electromagnetic_impact",
            json!([{"x": 460, "y": 0}]),
        ))
        .unwrap();

        let error = compile(&layout_with_blue_battle_skill(
            "electromagnetic_impact",
            json!([{"x": 461, "y": 0}]),
        ))
        .unwrap_err();
        assert!(error.contains("violates its battlefield map rule"));
    }

    #[test]
    fn random_circle_overlap_includes_the_sub_effect_radius() {
        compile(&layout_with_blue_battle_skill(
            "orbital_bombardment",
            json!([{"x": 560, "y": 0}]),
        ))
        .unwrap();

        let error = compile(&layout_with_blue_battle_skill(
            "orbital_bombardment",
            json!([{"x": 561, "y": 0}]),
        ))
        .unwrap_err();
        assert!(error.contains("violates its battlefield map rule"));
    }

    #[test]
    fn line_skill_requires_its_full_width_to_overlap_the_map() {
        compile(&layout_with_blue_battle_skill(
            "ion_blast",
            json!([{"x": -500, "y": 360}, {"x": 500, "y": 360}]),
        ))
        .unwrap();

        let error = compile(&layout_with_blue_battle_skill(
            "ion_blast",
            json!([{"x": -500, "y": 361}, {"x": 500, "y": 361}]),
        ))
        .unwrap_err();
        assert!(error.contains("violates its battlefield map rule"));

        let extreme = compile(&layout_with_blue_battle_skill(
            "ion_blast",
            json!([
                {"x": i32::MIN, "y": i32::MAX},
                {"x": i32::MAX, "y": i32::MAX}
            ]),
        ))
        .unwrap_err();
        assert!(extreme.contains("violates its battlefield map rule"));
    }

    #[test]
    fn shield_airdrop_requires_its_center_inside_the_map() {
        compile(&layout_with_blue_battle_skill(
            "shield_airdrop",
            json!([{"x": 400, "y": 0}]),
        ))
        .unwrap();

        let error = compile(&layout_with_blue_battle_skill(
            "shield_airdrop",
            json!([{"x": 401, "y": 0}]),
        ))
        .unwrap_err();
        assert!(error.contains("violates its battlefield map rule"));
    }

    #[test]
    fn mobile_beacon_requires_its_half_width_inside_the_map() {
        compile(&layout_with_blue_battle_skill(
            "mobile_beacon",
            json!([
                {"x": 380, "y": -100},
                {"x": 380, "y": 0},
                {"x": 380, "y": 100}
            ]),
        ))
        .unwrap();

        let error = compile(&layout_with_blue_battle_skill(
            "mobile_beacon",
            json!([
                {"x": 381, "y": -100},
                {"x": 380, "y": 0},
                {"x": 380, "y": 100}
            ]),
        ))
        .unwrap_err();
        assert!(error.contains("violates its battlefield map rule"));
    }

    #[test]
    fn summon_skills_use_their_effect_range_for_map_and_tower_clearance() {
        compile(&layout_with_blue_battle_skill(
            "rhino_assault",
            json!([{"x": 380, "y": 0}]),
        ))
        .unwrap();
        let map_error = compile(&layout_with_blue_battle_skill(
            "rhino_assault",
            json!([{"x": 381, "y": 0}]),
        ))
        .unwrap_err();
        assert!(map_error.contains("violates its battlefield map rule"));

        let rhino_error = compile(&layout_with_blue_battle_skill(
            "rhino_assault",
            json!([{"x": -300, "y": 170}]),
        ))
        .unwrap_err();
        assert!(rhino_error.contains("must be more than 160 m"));
        compile(&layout_with_blue_battle_skill(
            "rhino_assault",
            json!([{"x": -301, "y": 170}]),
        ))
        .unwrap();

        let underground_error = compile(&layout_with_blue_battle_skill(
            "underground_threat",
            json!([{"x": -312, "y": 170}]),
        ))
        .unwrap_err();
        assert!(underground_error.contains("must be more than 172 m"));
        compile(&layout_with_blue_battle_skill(
            "underground_threat",
            json!([{"x": -313, "y": 170}]),
        ))
        .unwrap();

        let wasp_error = compile(&layout_with_blue_battle_skill(
            "wasp_swarm",
            json!([{"x": -305, "y": 170}]),
        ))
        .unwrap_err();
        assert!(wasp_error.contains("must be more than 165 m"));
        compile(&layout_with_blue_battle_skill(
            "wasp_swarm",
            json!([{"x": -306, "y": 170}]),
        ))
        .unwrap();

        let red_error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                },
                "red": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}],
                    "battle_skills": [{
                        "type": "rhino_assault",
                        "positions": [{"x": -300, "y": 170}]
                    }]
                }
            }
        }))
        .unwrap_err();
        assert!(red_error.starts_with("side red battle skill"));
        assert!(red_error.contains("must be more than 160 m"));
    }

    #[test]
    fn shield_airdrop_uses_its_effect_range_for_tower_clearance() {
        let error = compile(&layout_with_blue_battle_skill(
            "shield_airdrop",
            json!([{"x": -350, "y": 170}]),
        ))
        .unwrap_err();
        assert!(error.contains("must be more than 210 m"));
        compile(&layout_with_blue_battle_skill(
            "shield_airdrop",
            json!([{"x": -351, "y": 170}]),
        ))
        .unwrap();
    }

    #[test]
    fn rejects_unknown_duplicate_and_wrong_length_battle_skills() {
        let layout = |battle_skills| {
            json!({
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}],
                        "battle_skills": battle_skills
                    },
                    "red": {
                        "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                    }
                }
            })
        };

        let unknown = compile(&layout(json!([{
            "type": "not_a_skill", "positions": [{"x": 0, "y": 0}]
        }])))
        .unwrap_err();
        assert_eq!(
            unknown,
            "side blue battle skill type \"not_a_skill\" is unknown"
        );

        let wrong_length = compile(&layout(json!([{
            "type": "mobile_beacon", "positions": [{"x": 0, "y": 0}]
        }])))
        .unwrap_err();
        assert_eq!(
            wrong_length,
            "side blue battle skill type \"mobile_beacon\" requires 3 positions, got 1"
        );

        let duplicate = compile(&layout(json!([
            {"type": "missile_strike", "positions": [{"x": 0, "y": 0}]},
            {"type": "missile_strike", "positions": [{"x": 10, "y": 10}]}
        ])))
        .unwrap_err();
        assert_eq!(
            duplicate,
            "side blue battle skill type \"missile_strike\" is declared more than once"
        );
    }

    #[test]
    fn resolves_representative_public_formation_types() {
        let formations = [
            ("fortress", NativeFormation::Unit(1)),
            ("mountain", NativeFormation::Unit(2002)),
        ];
        for (type_name, native) in formations {
            assert_eq!(
                resolve_unit_type(type_name).map(|spec| spec.native),
                Some(native)
            );
        }
        let constructions = [
            ("defensive_wall", NativeFormation::Construction(1)),
            ("magnetic_barrier", NativeFormation::Construction(4)),
        ];
        for (type_name, native) in constructions {
            assert_eq!(
                resolve_construction_type(type_name).map(|spec| spec.native),
                Some(native)
            );
        }
        let contraptions = [
            ("shield", NativeFormation::Contraption(10001)),
            ("missile", NativeFormation::Contraption(20001)),
            ("interceptor", NativeFormation::Contraption(30001)),
        ];
        for (type_name, native) in contraptions {
            assert_eq!(
                resolve_contraption_type(type_name).map(|spec| spec.native),
                Some(native)
            );
        }
        assert_eq!(resolve_unit_type("unit"), None);
        assert_eq!(resolve_construction_type("unit"), None);
        assert_eq!(resolve_contraption_type("unit"), None);
    }
}
