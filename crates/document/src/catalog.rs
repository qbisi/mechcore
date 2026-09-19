//! Build-pinned tables: which native ID a public type name denotes, and back.
//!
//! Every entry here is a fact about one game build. A document names types in
//! public words, and everything that has to reach the game resolves them here.

use crate::layout::TerrainType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeFormation {
    Unit(i32),
    Construction(i32),
    Contraption(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FormationSpec {
    pub(crate) native: NativeFormation,
    pub(crate) footprint: Option<(i64, i64)>,
}

#[derive(Clone, Copy)]
pub(crate) enum BattleSkillShape {
    Circle { radius: i64 },
    RandomCircle { outer_radius: i64, radius: i64 },
    Line { width: i64 },
    Path { width: i64 },
}

#[derive(Clone, Copy)]
pub(crate) enum BattleSkillMapRule {
    Overlap,
    Center,
    Contained,
}

#[derive(Clone, Copy)]
pub(crate) struct BattleSkillSpec {
    pub(crate) commander_skill_id: i32,
    pub(crate) positions: usize,
    pub(crate) shape: BattleSkillShape,
    pub(crate) map_rule: BattleSkillMapRule,
    pub(crate) tower_exclusion_radius: Option<i64>,
}

pub(crate) const fn formation_spec(native: NativeFormation, footprint: Option<(i64, i64)>) -> FormationSpec {
    FormationSpec { native, footprint }
}

pub(crate) const fn unit_spec(id: i32, width: i64, height: i64) -> FormationSpec {
    formation_spec(NativeFormation::Unit(id), Some((width, height)))
}

pub(crate) const fn construction_spec(id: i32, width: i64, height: i64) -> FormationSpec {
    formation_spec(NativeFormation::Construction(id), Some((width, height)))
}

pub(crate) const fn resolve_unit_type(type_name: &str) -> Option<FormationSpec> {
    match type_name.as_bytes() {
        b"fortress" => Some(unit_spec(1, 40, 40)),
        b"marksman" => Some(unit_spec(2, 20, 20)),
        b"vulcan" => Some(unit_spec(3, 40, 40)),
        b"melting_point" => Some(unit_spec(4, 40, 40)),
        b"rhino" => Some(unit_spec(5, 30, 30)),
        b"wasp" => Some(unit_spec(6, 50, 20)),
        b"mustang" => Some(unit_spec(7, 50, 20)),
        b"steel_ball" => Some(unit_spec(8, 50, 20)),
        b"fang" => Some(unit_spec(9, 50, 20)),
        b"crawler" => Some(unit_spec(10, 50, 20)),
        b"overlord" => Some(unit_spec(11, 50, 50)),
        b"stormcaller" => Some(unit_spec(12, 50, 20)),
        b"sledgehammer" => Some(unit_spec(13, 50, 20)),
        b"hacker" => Some(unit_spec(14, 30, 30)),
        b"arclight" => Some(unit_spec(15, 20, 20)),
        b"phoenix" => Some(unit_spec(16, 40, 20)),
        b"war_factory" => Some(unit_spec(17, 70, 70)),
        b"wraith" => Some(unit_spec(18, 30, 30)),
        b"scorpion" => Some(unit_spec(19, 30, 30)),
        b"fire_badger" => Some(unit_spec(20, 50, 20)),
        b"sabertooth" => Some(unit_spec(21, 30, 30)),
        b"typhoon" => Some(unit_spec(22, 40, 20)),
        b"sandworm" => Some(unit_spec(23, 40, 40)),
        b"tarantula" => Some(unit_spec(24, 30, 30)),
        b"phantom_ray" => Some(unit_spec(25, 50, 20)),
        b"farseer" => Some(unit_spec(26, 30, 30)),
        b"raiden" => Some(unit_spec(27, 40, 40)),
        b"hound" => Some(unit_spec(28, 40, 20)),
        b"abyss" => Some(unit_spec(29, 70, 70)),
        b"void_eye" => Some(unit_spec(30, 40, 20)),
        b"vortex" => Some(unit_spec(31, 20, 20)),
        b"mountain" => Some(unit_spec(2002, 70, 70)),
        _ => None,
    }
}

pub(crate) const fn resolve_construction_type(type_name: &str) -> Option<FormationSpec> {
    match type_name.as_bytes() {
        b"defensive_wall" => Some(construction_spec(1, 60, 10)),
        b"anti_armor_turret" => Some(construction_spec(2, 20, 20)),
        b"rapid_fire_turret" => Some(construction_spec(3, 20, 20)),
        b"magnetic_barrier" => Some(construction_spec(4, 50, 10)),
        _ => None,
    }
}

pub(crate) const fn resolve_contraption_type(type_name: &str) -> Option<FormationSpec> {
    match type_name.as_bytes() {
        b"shield" => Some(formation_spec(NativeFormation::Contraption(10001), None)),
        b"missile" => Some(formation_spec(NativeFormation::Contraption(20001), None)),
        b"interceptor" => Some(formation_spec(
            NativeFormation::Contraption(30001),
            Some((30, 30)),
        )),
        _ => None,
    }
}

/// Resolves a build-pinned native unit type ID to the public layout name and footprint.
#[must_use]
pub const fn unit_type_from_id(id: i32) -> Option<(&'static str, (i64, i64))> {
    match id {
        1 => Some(("fortress", (40, 40))),
        2 => Some(("marksman", (20, 20))),
        3 => Some(("vulcan", (40, 40))),
        4 => Some(("melting_point", (40, 40))),
        5 => Some(("rhino", (30, 30))),
        6 => Some(("wasp", (50, 20))),
        7 => Some(("mustang", (50, 20))),
        8 => Some(("steel_ball", (50, 20))),
        9 => Some(("fang", (50, 20))),
        10 => Some(("crawler", (50, 20))),
        11 => Some(("overlord", (50, 50))),
        12 => Some(("stormcaller", (50, 20))),
        13 => Some(("sledgehammer", (50, 20))),
        14 => Some(("hacker", (30, 30))),
        15 => Some(("arclight", (20, 20))),
        16 => Some(("phoenix", (40, 20))),
        17 => Some(("war_factory", (70, 70))),
        18 => Some(("wraith", (30, 30))),
        19 => Some(("scorpion", (30, 30))),
        20 => Some(("fire_badger", (50, 20))),
        21 => Some(("sabertooth", (30, 30))),
        22 => Some(("typhoon", (40, 20))),
        23 => Some(("sandworm", (40, 40))),
        24 => Some(("tarantula", (30, 30))),
        25 => Some(("phantom_ray", (50, 20))),
        26 => Some(("farseer", (30, 30))),
        27 => Some(("raiden", (40, 40))),
        28 => Some(("hound", (40, 20))),
        29 => Some(("abyss", (70, 70))),
        30 => Some(("void_eye", (40, 20))),
        31 => Some(("vortex", (20, 20))),
        2002 => Some(("mountain", (70, 70))),
        _ => None,
    }
}

/// Resolves a native construction type ID to the public layout name and footprint.
#[must_use]
pub const fn construction_type_from_id(id: i32) -> Option<(&'static str, (i64, i64))> {
    match id {
        1 => Some(("defensive_wall", (60, 10))),
        2 => Some(("anti_armor_turret", (20, 20))),
        3 => Some(("rapid_fire_turret", (20, 20))),
        4 => Some(("magnetic_barrier", (50, 10))),
        _ => None,
    }
}

/// Resolves a build-pinned native contraption type ID to the public layout name.
#[must_use]
pub const fn contraption_type_from_id(id: i32) -> Option<&'static str> {
    match id {
        10_001 => Some("shield"),
        20_001 => Some("missile"),
        30_001 => Some("interceptor"),
        _ => None,
    }
}

/// Resolves a build-pinned native commander-skill ID to the public layout name.
#[must_use]
pub const fn battle_skill_type_from_id(id: i32) -> Option<&'static str> {
    match id {
        100_002 => Some("incendiary_bomb"),
        200_001 => Some("electromagnetic_impact"),
        200_002 => Some("electromagnetic_blast"),
        200_003 => Some("photon_emission"),
        300_001 => Some("missile_strike"),
        300_003 => Some("orbital_bombardment"),
        300_004 => Some("nuke"),
        300_005 => Some("lightning_storm"),
        300_006 => Some("ion_blast"),
        300_007 => Some("orbital_javelin"),
        400_002 => Some("sticky_oil_bomb"),
        500_002 => Some("acid_blast"),
        600_002 => Some("smoke_bomb"),
        800_001 => Some("shield_airdrop"),
        1_200_001 => Some("underground_threat"),
        1_200_002 => Some("rhino_assault"),
        1_200_003 => Some("wasp_swarm"),
        1_200_004 => Some("mobilize_battleship"),
        1_200_005 => Some("vulcans_descent"),
        1_500_001 => Some("mobile_beacon"),
        1_500_002 => Some("mobile_beacon_card"),
        _ => None,
    }
}

/// The battlefield area a commander skill leaves behind, if it leaves one.
///
/// Five skill classes derive from `RangeItemCommanderSkill` in build 2259 and
/// each answers `GetRangeItemType` with a constant: `CS_Fire` with `Fire`,
/// `CS_Oil` with `Oil`, `CS_Fog` with `Fog`, `CS_Acid` with `Acid` and
/// `CS_Recovery` with `RecoveryZone`. Four of those five are reached by a skill
/// this catalogue names, and the shipped description of each names the same
/// substance its class does, which is what ties an ID to a type here.
///
/// `CS_Recovery` is left out because no skill ID in this catalogue reaches it.
/// A skill that leaves nothing behind answers `None`, and that is most of them.
#[must_use]
pub const fn terrain_type_from_skill(id: i32) -> Option<TerrainType> {
    match id {
        100_002 => Some(TerrainType::Fire),
        400_002 => Some(TerrainType::Oil),
        500_002 => Some(TerrainType::Acid),
        600_002 => Some(TerrainType::Fog),
        _ => None,
    }
}

/// Which skill produces one terrain, which is what carries its geometry.
///
/// A terrain's own document says where it is and how much of it is left; how
/// wide each of its points is, and how many points a release expands into, are
/// the producing skill's. Only a type this catalogue can name a skill for can
/// be compiled into a plan.
#[must_use]
pub const fn terrain_skill_from_type(terrain: TerrainType) -> Option<i32> {
    match terrain {
        TerrainType::Fire => Some(100_002),
        TerrainType::Oil => Some(400_002),
        TerrainType::Acid => Some(500_002),
        TerrainType::Fog => Some(600_002),
        TerrainType::RecoveryZone => None,
    }
}

#[allow(clippy::too_many_lines)] // Keep the build-pinned public catalog one-to-one and auditable.
pub(crate) const fn resolve_battle_skill_type(type_name: &str) -> Option<BattleSkillSpec> {
    let spec = match type_name.as_bytes() {
        b"incendiary_bomb" => battle_skill_spec(
            100_002,
            2,
            BattleSkillShape::Line { width: 40 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"electromagnetic_impact" => battle_skill_spec(
            200_001,
            1,
            BattleSkillShape::Circle { radius: 60 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"electromagnetic_blast" => battle_skill_spec(
            200_002,
            1,
            BattleSkillShape::Circle { radius: 130 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"photon_emission" => battle_skill_spec(
            200_003,
            1,
            BattleSkillShape::Circle { radius: 110 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"missile_strike" => battle_skill_spec(
            300_001,
            1,
            BattleSkillShape::Circle { radius: 40 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"orbital_bombardment" => battle_skill_spec(
            300_003,
            1,
            BattleSkillShape::RandomCircle {
                outer_radius: 130,
                radius: 30,
            },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"nuke" => battle_skill_spec(
            300_004,
            1,
            BattleSkillShape::Circle { radius: 100 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"lightning_storm" => battle_skill_spec(
            300_005,
            1,
            BattleSkillShape::RandomCircle {
                outer_radius: 130,
                radius: 30,
            },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"ion_blast" => battle_skill_spec(
            300_006,
            2,
            BattleSkillShape::Line { width: 20 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"orbital_javelin" => battle_skill_spec(
            300_007,
            1,
            BattleSkillShape::Circle { radius: 30 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"sticky_oil_bomb" => battle_skill_spec(
            400_002,
            2,
            BattleSkillShape::Line { width: 30 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"acid_blast" => battle_skill_spec(
            500_002,
            2,
            BattleSkillShape::Line { width: 40 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"smoke_bomb" => battle_skill_spec(
            600_002,
            2,
            BattleSkillShape::Line { width: 50 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"shield_airdrop" => battle_skill_spec(
            800_001,
            1,
            BattleSkillShape::Circle { radius: 70 },
            BattleSkillMapRule::Center,
            Some(70),
        ),
        b"underground_threat" => battle_skill_spec(
            1_200_001,
            1,
            BattleSkillShape::Circle { radius: 32 },
            BattleSkillMapRule::Contained,
            Some(32),
        ),
        b"rhino_assault" => battle_skill_spec(
            1_200_002,
            1,
            BattleSkillShape::Circle { radius: 20 },
            BattleSkillMapRule::Contained,
            Some(20),
        ),
        b"wasp_swarm" => battle_skill_spec(
            1_200_003,
            1,
            BattleSkillShape::Circle { radius: 25 },
            BattleSkillMapRule::Contained,
            Some(25),
        ),
        b"mobilize_battleship" => battle_skill_spec(
            1_200_004,
            1,
            BattleSkillShape::Circle { radius: 25 },
            BattleSkillMapRule::Contained,
            Some(25),
        ),
        b"vulcans_descent" => battle_skill_spec(
            1_200_005,
            1,
            BattleSkillShape::Circle { radius: 25 },
            BattleSkillMapRule::Contained,
            Some(25),
        ),
        // The blueprint's Mobile Beacon and the card's are two skills with one
        // path, named apart as `config/names.yaml` names them.
        b"mobile_beacon" => battle_skill_spec(
            1_500_001,
            3,
            BattleSkillShape::Path { width: 40 },
            BattleSkillMapRule::Contained,
            None,
        ),
        b"mobile_beacon_card" => battle_skill_spec(
            1_500_002,
            3,
            BattleSkillShape::Path { width: 40 },
            BattleSkillMapRule::Contained,
            None,
        ),
        _ => return None,
    };
    Some(spec)
}

pub(crate) const fn battle_skill_spec(
    commander_skill_id: i32,
    positions: usize,
    shape: BattleSkillShape,
    map_rule: BattleSkillMapRule,
    tower_exclusion_radius: Option<i64>,
) -> BattleSkillSpec {
    BattleSkillSpec {
        commander_skill_id,
        positions,
        shape,
        map_rule,
        tower_exclusion_radius,
    }
}

/// The native unit ID a public type name denotes.
/// The Research Center's two enhancement chains: each blueprint and the officer
/// it hands out, as `config/economy.yaml` states them. A blueprint that grants
/// a commander skill is not here: a fight sees it only as the skill.
pub const CHAIN_BLUEPRINTS: [(i32, i32); 4] =
    [(4, 20_310), (401, 20_311), (5, 20_300), (501, 20_301)];

/// The officer a chain blueprint hands out.
#[must_use]
pub fn chain_officer(blueprint: i32) -> Option<i32> {
    CHAIN_BLUEPRINTS
        .iter()
        .find(|(chain, _)| *chain == blueprint)
        .map(|(_, officer)| *officer)
}

/// The chain blueprint that hands out an officer.
#[must_use]
pub fn chain_blueprint(officer: i32) -> Option<i32> {
    CHAIN_BLUEPRINTS
        .iter()
        .find(|(_, granted)| *granted == officer)
        .map(|(blueprint, _)| *blueprint)
}

pub(crate) fn unit_id_from_type(type_name: &str) -> Option<i32> {
    match resolve_unit_type(type_name)?.native {
        NativeFormation::Unit(id) => Some(id),
        NativeFormation::Construction(_) | NativeFormation::Contraption(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{TerrainType, terrain_skill_from_type, terrain_type_from_skill};

    /// Every skill that leaves an area behind is routed to its substance, and
    /// every other skill to none.
    ///
    /// The Sticky Oil Bomb is the only one a standard 1v1 ever reads back,
    /// because it is the only one that lasts two rounds, but the mapping is
    /// what decides that rather than the reader.
    #[test]
    fn a_skill_is_routed_to_the_substance_it_leaves() {
        assert_eq!(terrain_type_from_skill(400_002), Some(TerrainType::Oil));
        assert_eq!(terrain_type_from_skill(100_002), Some(TerrainType::Fire));
        assert_eq!(terrain_type_from_skill(500_002), Some(TerrainType::Acid));
        assert_eq!(terrain_type_from_skill(600_002), Some(TerrainType::Fog));
        // A shield stands on the board rather than covering it, and a strike
        // leaves nothing at all.
        assert_eq!(terrain_type_from_skill(800_001), None);
        assert_eq!(terrain_type_from_skill(300_001), None);
    }

    /// The two directions agree wherever both are defined.
    #[test]
    fn the_terrain_mapping_round_trips() {
        for terrain in [
            TerrainType::Fire,
            TerrainType::Oil,
            TerrainType::Fog,
            TerrainType::Acid,
        ] {
            let skill = terrain_skill_from_type(terrain).expect("a producing skill");
            assert_eq!(terrain_type_from_skill(skill), Some(terrain));
        }
        // No skill this catalogue names reaches `CS_Recovery`.
        assert_eq!(terrain_skill_from_type(TerrainType::RecoveryZone), None);
    }
}
