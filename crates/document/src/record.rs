//! Typed reader for the `BattleRecord` XML embedded in a build-2259 GRBR.
//!
//! The file is a .NET `BinaryFormatter` graph wrapping one XML document. This
//! module locates that document and deserializes the parts the battle, turn and
//! state documents are built from. Fields those documents exclude are simply not
//! declared here; `serde` ignores what it is not asked for.
//!
//! Element names, and the two spellings of a position, are the game's own.

use serde::Deserialize;

/// One recorded match.
#[derive(Debug, Deserialize)]
#[serde(rename = "BattleRecord")]
pub struct BattleRecord {
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "Seat")]
    pub seat: i32,
    #[serde(rename = "BattleInfo")]
    pub info: BattleInfo,
    #[serde(rename = "playerRecords")]
    pub players: PlayerRecords,
    #[serde(rename = "matchDatas")]
    pub match_rounds: MatchRounds,
}

/// The match header, constant across every round.
#[derive(Debug, Deserialize)]
pub struct BattleInfo {
    #[serde(rename = "SystemSeed")]
    pub system_seed: i32,
    #[serde(rename = "MapID")]
    pub map_id: i32,
    #[serde(rename = "BattleID")]
    pub battle_id: String,
    #[serde(rename = "MatchMode")]
    pub match_mode: String,
    /// Present only outside a ranked match, where it reads `Test`.
    #[serde(default, rename = "MatchType")]
    pub match_type: Option<String>,
    #[serde(default, rename = "gameRules")]
    pub game_rules: IntList,
}

#[derive(Debug, Deserialize)]
pub struct PlayerRecords {
    #[serde(default, rename = "PlayerRecord")]
    pub entries: Vec<PlayerRecord>,
}

#[derive(Debug, Deserialize)]
pub struct PlayerRecord {
    pub seed: i32,
    pub data: PlayerSetup,
    #[serde(rename = "playerRoundRecords")]
    pub rounds: PlayerRoundRecords,
}

/// The per-player match-level record: supply settings and the tech loadout.
#[derive(Debug, Deserialize)]
pub struct PlayerSetup {
    #[serde(rename = "maxRoundSupply")]
    pub max_round_supply: i32,
    #[serde(rename = "firstRoundSupply")]
    pub first_round_supply: i32,
    #[serde(rename = "roundSupplyIncreaseValue")]
    pub round_supply_increase: i32,
    #[serde(default, rename = "unitDatas")]
    pub unit_datas: UnitTechRows,
}

#[derive(Debug, Default, Deserialize)]
pub struct UnitTechRows {
    #[serde(default, rename = "unitData")]
    pub entries: Vec<UnitTechRow>,
}

#[derive(Debug, Deserialize)]
pub struct UnitTechRow {
    pub id: i32,
    #[serde(default)]
    pub techs: TechList,
}

#[derive(Debug, Default, Deserialize)]
pub struct TechList {
    #[serde(default, rename = "tech")]
    pub entries: Vec<TechRef>,
}

#[derive(Debug, Deserialize)]
pub struct TechRef {
    #[serde(rename = "@data")]
    pub data: i32,
}

#[derive(Debug, Deserialize)]
pub struct PlayerRoundRecords {
    #[serde(default, rename = "PlayerRoundRecord")]
    pub entries: Vec<PlayerRoundRecord>,
}

#[derive(Debug, Deserialize)]
pub struct PlayerRoundRecord {
    pub round: i32,
    #[serde(rename = "playerData")]
    pub data: PlayerData,
    #[serde(default, rename = "actionRecords")]
    pub actions: ActionRecords,
}

/// One side's snapshot, taken at the start of the round and before its reset.
#[derive(Debug, Deserialize)]
pub struct PlayerData {
    #[serde(rename = "reactorCore")]
    pub reactor_core: i32,
    pub supply: i32,
    #[serde(default)]
    pub units: UnitRecords,
    #[serde(rename = "unitIndex")]
    pub unit_index: i32,
    #[serde(default)]
    pub officers: IntList,
    #[serde(default, rename = "commanderSkills")]
    pub commander_skills: CommanderSkills,
    #[serde(default, rename = "activeTechnologies")]
    pub active_technologies: ActiveTechnologies,
    #[serde(default, rename = "equipmentDatas")]
    pub equipment: EquipmentDatas,
    pub shop: ShopData,
    #[serde(default)]
    pub contraptions: ContraptionRecords,
    #[serde(rename = "contraptionIndex")]
    pub contraption_index: i32,
    /// The game's own spelling.
    #[serde(default, rename = "bluepints")]
    pub blueprints: IntList,
    #[serde(default, rename = "energyTowerSkills")]
    pub energy_tower_skills: IntList,
    #[serde(default, rename = "towerStrengthenLevels")]
    pub tower_strengthen_levels: IntList,
    #[serde(default, rename = "constructionSnapshotDatas")]
    pub constructions: ConstructionRecords,
}

#[derive(Debug, Default, Deserialize)]
pub struct UnitRecords {
    #[serde(default, rename = "NewUnitData")]
    pub entries: Vec<UnitRecord>,
}

#[derive(Debug, Deserialize)]
pub struct UnitRecord {
    pub id: i32,
    #[serde(rename = "Index")]
    pub index: i32,
    #[serde(rename = "Exp")]
    pub exp: i32,
    #[serde(rename = "Level")]
    pub level: i32,
    #[serde(rename = "Position")]
    pub position: PositionRecord,
    #[serde(rename = "EquipmentID")]
    pub equipment_id: i32,
    #[serde(rename = "IsRotate")]
    pub rotated: bool,
    /// What recovering the formation pays back, at the prices actually paid.
    #[serde(rename = "SellSupply")]
    pub sell_supply: i32,
}

#[derive(Debug, Default, Deserialize)]
pub struct CommanderSkills {
    #[serde(default, rename = "CommanderSkillData")]
    pub entries: Vec<CommanderSkill>,
}

#[derive(Debug, Deserialize)]
pub struct CommanderSkill {
    pub index: i32,
    pub id: i32,
    #[serde(rename = "coolingRound")]
    pub cooling_round: i32,
}

#[derive(Debug, Default, Deserialize)]
pub struct ActiveTechnologies {
    #[serde(default, rename = "UnitData")]
    pub entries: Vec<UnitTechRow>,
}

#[derive(Debug, Default, Deserialize)]
pub struct EquipmentDatas {
    #[serde(default, rename = "EquipmentData")]
    pub entries: Vec<EquipmentRecord>,
}

#[derive(Debug, Deserialize)]
pub struct EquipmentRecord {
    pub id: i32,
    pub durability: i32,
}

#[derive(Debug, Deserialize)]
pub struct ShopData {
    #[serde(default, rename = "unlockedUnits")]
    pub unlocked_units: IntList,
    #[serde(rename = "BuyCount")]
    pub buy_count: i32,
    #[serde(rename = "UnlockCount")]
    pub unlock_count: i32,
}

#[derive(Debug, Default, Deserialize)]
pub struct ContraptionRecords {
    #[serde(default, rename = "ContraptionData")]
    pub entries: Vec<ContraptionRecord>,
}

#[derive(Debug, Deserialize)]
pub struct ContraptionRecord {
    pub index: i32,
    pub id: i32,
    pub position: PositionRecord,
}

#[derive(Debug, Default, Deserialize)]
pub struct ConstructionRecords {
    #[serde(default, rename = "ConstructionSnapshotData")]
    pub entries: Vec<ConstructionRecord>,
}

#[derive(Debug, Deserialize)]
pub struct ConstructionRecord {
    #[serde(rename = "Index")]
    pub index: i32,
    #[serde(rename = "ID")]
    pub id: i32,
    #[serde(rename = "Position")]
    pub position: PositionRecord,
}

#[derive(Debug, Default, Deserialize)]
pub struct ActionRecords {
    #[serde(default, rename = "MatchActionData")]
    pub entries: Vec<ActionRecord>,
}

/// Every player action, read as one shape.
///
/// The record distinguishes the sixteen kinds by an `xsi:type` attribute rather
/// than by element name, so one struct with optional fields reads them all and
/// [`crate::battle`] resolves the kind.
#[derive(Debug, Deserialize)]
pub struct ActionRecord {
    #[serde(rename = "@type", alias = "@xsi:type")]
    pub kind: String,
    #[serde(default, rename = "ID")]
    pub id: Option<i32>,
    #[serde(default, rename = "Index")]
    pub index: Option<i32>,
    #[serde(default, rename = "UID")]
    pub unit_id: Option<i32>,
    #[serde(default, rename = "UIDX")]
    pub unit_index_allocated: Option<i32>,
    #[serde(default, rename = "TechID")]
    pub tech_id: Option<i32>,
    #[serde(default, rename = "SkillID")]
    pub skill_id: Option<i32>,
    #[serde(default, rename = "SkillIndex")]
    pub skill_index: Option<i32>,
    #[serde(default, rename = "UnitIndex")]
    pub unit_index: Option<i32>,
    #[serde(default, rename = "ConstructionIndex")]
    pub construction_index: Option<i32>,
    #[serde(default, rename = "EquipmentID")]
    pub equipment_id: Option<i32>,
    #[serde(default, rename = "ContraptionID")]
    pub contraption_id: Option<i32>,
    /// `PAD_BuyUnit` spells its position in lower case.
    #[serde(default, rename = "position")]
    pub buy_position: Option<PositionRecord>,
    /// `PAD_ReleaseContraption` spells it in upper case.
    #[serde(default, rename = "Position")]
    pub release_position: Option<PositionRecord>,
    #[serde(default, rename = "ExtraPosition")]
    pub extra_position: Option<PositionRecord>,
    #[serde(default, rename = "Positions")]
    pub positions: Option<MapVectors>,
    #[serde(default, rename = "moveUnitDatas")]
    pub moves: Option<MoveUnitDatas>,
}

#[derive(Debug, Default, Deserialize)]
pub struct MapVectors {
    #[serde(default, rename = "MapVector")]
    pub entries: Vec<PositionRecord>,
}

#[derive(Debug, Default, Deserialize)]
pub struct MoveUnitDatas {
    #[serde(default, rename = "MoveUnitData")]
    pub entries: Vec<MoveUnitData>,
}

#[derive(Debug, Deserialize)]
pub struct MoveUnitData {
    #[serde(rename = "unitIndex")]
    pub unit_index: i32,
    pub position: PositionRecord,
    #[serde(rename = "isRotate")]
    pub rotated: bool,
}

#[derive(Debug, Deserialize)]
pub struct PositionRecord {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Deserialize)]
pub struct MatchRounds {
    #[serde(default, rename = "MatchSnapshotData")]
    pub entries: Vec<MatchRound>,
}

#[derive(Debug, Deserialize)]
pub struct MatchRound {
    pub round: i32,
    #[serde(default, rename = "reinforceItems")]
    pub reinforce_items: ReinforceItems,
    /// The match's reinforcement random stream as the round opened.
    ///
    /// It is not a state field and no document holds it. The opening is dealt
    /// from it, so the converter reads it to rebuild what the four
    /// combinations were; `crate::opening` says how.
    #[serde(default, rename = "randomStateData")]
    pub random_state: RandomStateData,
}

#[derive(Debug, Default, Deserialize)]
pub struct RandomStateData {
    #[serde(default, rename = "randomStates")]
    pub states: RandomStates,
}

#[derive(Debug, Default, Deserialize)]
pub struct RandomStates {
    #[serde(default, rename = "unsignedLong")]
    pub values: Vec<u64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ReinforceItems {
    #[serde(default, rename = "ArrayOfInt")]
    pub arrays: Vec<IntList>,
}

#[derive(Debug, Default, Deserialize)]
pub struct IntList {
    #[serde(default, rename = "int")]
    pub values: Vec<i32>,
}

/// Reads the `BattleRecord` embedded in a GRBR file.
///
/// # Errors
///
/// Returns an error when the file carries no embedded XML payload, carries more
/// than one, or when that payload does not deserialize as a `BattleRecord`.
pub fn read(grbr: &[u8]) -> Result<BattleRecord, String> {
    let xml = embedded_xml(grbr)?;
    quick_xml::de::from_str(xml).map_err(|error| format!("GRBR XML is not a BattleRecord: {error}"))
}

/// Locates the single embedded XML document inside the `BinaryFormatter` graph.
fn embedded_xml(grbr: &[u8]) -> Result<&str, String> {
    const START: &[u8] = b"<?xml version=\"1.0\" encoding=\"utf-8\"?>";
    const END: &[u8] = b"</BattleRecord>";
    let start = find(grbr, START).ok_or_else(|| "GRBR has no embedded XML payload".to_owned())?;
    if find(&grbr[start + START.len()..], START).is_some() {
        return Err("GRBR contains more than one embedded XML payload".into());
    }
    let end = find(&grbr[start..], END)
        .map(|offset| start + offset + END.len())
        .ok_or_else(|| "GRBR embedded XML has no BattleRecord terminator".to_owned())?;
    std::str::from_utf8(&grbr[start..end])
        .map_err(|error| format!("GRBR embedded XML is not UTF-8: {error}"))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::read;

    const TUFF: &str = "../../tests/grbr/2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].grbr";

    fn tuff() -> Vec<u8> {
        std::fs::read(TUFF).expect("tracked GRBR fixture")
    }

    #[test]
    fn reads_the_match_header() {
        let record = read(&tuff()).unwrap();
        assert_eq!(record.version, "2259");
        assert_eq!(record.seat, 0);
        assert_eq!(record.info.map_id, 1021);
        assert_eq!(record.info.system_seed, 31_103_914);
        assert_eq!(record.info.match_mode, "VS_1_1");
        assert!(record.info.game_rules.values.is_empty());
    }

    #[test]
    fn reads_both_sides_and_every_round() {
        let record = read(&tuff()).unwrap();
        assert_eq!(record.players.entries.len(), 2);
        assert_eq!(record.match_rounds.entries.len(), 9);
        for player in &record.players.entries {
            assert_eq!(player.data.unit_datas.entries.len(), 34);
            assert_eq!(player.data.first_round_supply, 200);
            let rounds: Vec<i32> = player.rounds.entries.iter().map(|entry| entry.round).collect();
            assert_eq!(rounds, (0..9).collect::<Vec<_>>());
        }
    }

    #[test]
    fn reads_attributes_and_nested_positions() {
        let record = read(&tuff()).unwrap();
        let player = &record.players.entries[0];
        let fortress = &player.data.unit_datas.entries[0];
        assert_eq!(fortress.id, 1);
        let mut techs: Vec<i32> = fortress.techs.entries.iter().map(|tech| tech.data).collect();
        techs.sort_unstable();
        assert_eq!(techs, [1105, 10301, 10401, 10801]);

        let round = &player.rounds.entries[7];
        let unit = &round.data.units.entries[0];
        assert_eq!((unit.id, unit.index, unit.level, unit.exp), (31, 1, 1, 615));
        assert_eq!((unit.position.x, unit.position.y), (-160, -100));
    }

    #[test]
    fn reads_the_action_kind_from_its_attribute() {
        let record = read(&tuff()).unwrap();
        let round = &record.players.entries[0].rounds.entries[7];
        let kinds: Vec<&str> = round
            .actions
            .entries
            .iter()
            .map(|action| action.kind.as_str())
            .collect();
        assert_eq!(kinds.first(), Some(&"PAD_ChooseReinforceItem"));
        assert_eq!(kinds.last(), Some(&"PAD_FinishDeploy"));
        let buy = round
            .actions
            .entries
            .iter()
            .find(|action| action.kind == "PAD_BuyUnit")
            .unwrap();
        assert_eq!(buy.unit_id, Some(10));
        assert_eq!(buy.unit_index_allocated, Some(-1));
        let position = buy.buy_position.as_ref().unwrap();
        assert_eq!((position.x, position.y), (5, -160));
    }

    #[test]
    fn reads_the_shared_reinforcement_offer() {
        let record = read(&tuff()).unwrap();
        let round = &record.match_rounds.entries[7];
        assert_eq!(round.round, 7);
        assert_eq!(round.reinforce_items.arrays.len(), 1);
        assert_eq!(round.reinforce_items.arrays[0].values.len(), 4);
        assert!(record.match_rounds.entries[0].reinforce_items.arrays.is_empty());
    }
}
