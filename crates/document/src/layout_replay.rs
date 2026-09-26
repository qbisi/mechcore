//! A layout written as a replay the game can fight.
//!
//! A replay's `PlayerRoundRecord` holds the position each side opens a round
//! with, and the game's replay starts a round from that snapshot rather than
//! from the actions before it. So a layout is a replay with one deployment
//! round: its snapshot is the layout and it carries no action, and the fight
//! the game plays from it is the layout's fight.
//!
//! The fight draws only from streams the game derives from `SystemSeed` and
//! the round, so the record carries no random state. The rest of the record
//! is what the game needs to open it: two players, the map, and the 1v1
//! header constants. What this writer has not been measured against it
//! refuses by name rather than writes.
//!
//! The file around the record is a .NET `BinaryFormatter` stream of one
//! `GameRiver.Replay`, which holds the record as an XML string.
//! `docs/spec/document/layout-replay.md` states both.

use crate::catalog::NativeFormation;
use crate::compile::{Placement, Plan, SidePlan};
use std::fmt::Write as _;

/// The map a layout that names none is fought on, the Training Ground's.
pub const DEFAULT_MAP_ID: i32 = 1021;

/// Writes `plan` as a replay whose round `plan.round` is the layout's fight.
///
/// # Errors
///
/// Returns an error when the plan holds anything this writer has not been
/// measured against, or when it names no seed.
pub fn layout_replay(plan: &Plan, game_build: &str) -> Result<Vec<u8>, String> {
    let refusals: Vec<String> = [("blue", &plan.blue), ("red", &plan.red)]
        .into_iter()
        .flat_map(|(name, side)| side_refusals(name, side))
        .chain((plan.round != 1).then(|| format!("round {} is not round 1", plan.round)))
        .collect();
    if !refusals.is_empty() {
        return Err(format!(
            "a layout replay cannot state {}",
            refusals.join("; ")
        ));
    }
    let seed = plan
        .seed
        .ok_or("a layout replay needs a seed: the game seeds the fight from it")?;
    let map_id = plan.map_id.unwrap_or(DEFAULT_MAP_ID);
    let version = build_number(game_build)?;
    let mut xml = String::from("\u{feff}");
    write_record(&mut xml, plan, seed, map_id, version);
    Ok(binary_formatter(version, map_id, xml.as_bytes()))
}

fn side_refusals(name: &str, side: &SidePlan) -> Vec<String> {
    let mut refusals = Vec::new();
    let mut refuse = |present: bool, what: &str| {
        if present {
            refusals.push(format!("side {name} {what}"));
        }
    };
    refuse(!side.techs.officers.is_empty(), "officers");
    refuse(!side.techs.units.is_empty(), "techs");
    refuse(!side.energy_tower_skills.is_empty(), "energy_tower_skills");
    refuse(
        side.tower_strengthen_levels.iter().any(|level| *level != 0),
        "tower_strengthen_levels",
    );
    refuse(!side.constructions.is_empty(), "constructions");
    refuse(!side.contraptions.is_empty(), "contraptions");
    refuse(!side.airdrop_shields.is_empty(), "airdrop_shields");
    refuse(!side.terrains.is_empty(), "terrains");
    refuse(!side.battle_skills.is_empty(), "battle_skills");
    for unit in &side.units {
        let at = format!(
            "unit {} at ({}, {})",
            unit.type_name, unit.position.x, unit.position.y
        );
        refuse(
            unit.level.is_some_and(|level| level != 1),
            &format!("{at} level"),
        );
        refuse(unit.exp.is_some_and(|exp| exp != 0), &format!("{at} exp"));
        refuse(!unit.equipment.is_empty(), &format!("{at} equipment"));
        refuse(unit.travelling, &format!("{at} travelling"));
    }
    refusals
}

/// The replay format's version: the build's last component.
fn build_number(game_build: &str) -> Result<i32, String> {
    game_build
        .rsplit('.')
        .next()
        .and_then(|number| number.parse().ok())
        .ok_or_else(|| format!("game build {game_build:?} does not end in a build number"))
}

fn write_record(xml: &mut String, plan: &Plan, seed: i32, map_id: i32, version: i32) {
    xml.push_str(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<BattleRecord \
         xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><playerRecords>",
    );
    // Both sides are written in the board's frame, which is blue's own.
    for (id, name, side, sign) in [(1, "blue", &plan.blue, 1), (2, "red", &plan.red, -1)] {
        write_player(xml, id, name, &side.units, sign, plan.round);
    }
    xml.push_str("</playerRecords><matchDatas>");
    for round in 0..=plan.round {
        let _ = write!(
            xml,
            "<MatchSnapshotData><round>{round}</round>{EMPTY_RANDOM}<reinforceItems />\
             <teamRanks><int>0</int><int>1</int></teamRanks><useConstruction>false</useConstruction>\
             <poolOPs /><deadCount>0</deadCount><RoundExcludeReinforce /></MatchSnapshotData>"
        );
    }
    let _ = write!(
        xml,
        "</matchDatas><reinforceItems /><Version>{version}</Version><Seat>0</Seat><BattleInfo>\
         <gameRules /><StartTime>0</StartTime><SystemSeed>{seed}</SystemSeed>\
         <BattleID>{BATTLE_ID}</BattleID><PrepareTime>30</PrepareTime><DeployTime>100</DeployTime>\
         <FightTime>120</FightTime><MapID>{map_id}</MapID><MaxRound>40</MaxRound>\
         <BlueprintIncreaseSupply>0</BlueprintIncreaseSupply>\
         <EnableAdvanceTeam>false</EnableAdvanceTeam><EnableReinforcement>false</EnableReinforcement>\
         <EnableUnitReinforcement>false</EnableUnitReinforcement>\
         <EnableConstruction>true</EnableConstruction><GameMode>Normal</GameMode>\
         <MatchMode>VS_1_1</MatchMode><ScoreMode>ReduceScore</ScoreMode><HostID>0</HostID>\
         <SurviveModeDifficulty>VeryEasy</SurviveModeDifficulty></BattleInfo>\
         <CreateTime>0001-01-01T00:00:00</CreateTime></BattleRecord>"
    );
}

const BATTLE_ID: &str = "layout";
const EMPTY_RANDOM: &str = "<randomStateData><randomStates /></randomStateData>";

fn write_player(xml: &mut String, id: u64, name: &str, units: &[Placement], sign: i32, round: i32) {
    let _ = write!(
        xml,
        "<PlayerRecord><id>{id}</id><name>{name}</name><seed>0</seed><ad>0</ad><data>\
         <reactorCore>4500</reactorCore><MaxReactorCore>4500</MaxReactorCore>\
         <maxRoundSupply>4000</maxRoundSupply><firstRoundSupply>200</firstRoundSupply>\
         <roundSupplyIncreaseValue>200</roundSupplyIncreaseValue><team>0</team>\
         <isLeader>false</isLeader><type>Player</type><unitDatas />\
         <style><unitStyles /></style><plugins /></data><playerRoundRecords>"
    );
    // Round 0 is the opening, which holds nothing; the layout's round opens
    // with its units.
    for snapshot in 0..=round {
        let placed = if snapshot == round { units } else { &[] };
        let _ = write!(
            xml,
            "<PlayerRoundRecord><round>{snapshot}</round><playerData>{EMPTY_RANDOM}\
             <reinforceRandomStateData><randomStates /></reinforceRandomStateData>\
             <reactorCore>4500</reactorCore><supply>0</supply>\
             <preRoundFightResult>Win</preRoundFightResult>"
        );
        write_units(xml, placed, sign);
        let _ = write!(
            xml,
            "<unitIndex>{}</unitIndex><officers /><mainEffects /><lastRoundSupply>0</lastRoundSupply>\
             <reinforceShopStrengthens /><commanderSkills /><activeTechnologies /><equipmentDatas />\
             <shop><unlockedUnits /><lockedUnits />{EMPTY_RANDOM}<BuyCount>0</BuyCount>\
             <MaxUnlockCount>0</MaxUnlockCount><UnlockCount>0</UnlockCount>\
             <DiscountBalance>0</DiscountBalance><MaxUnitCount>0</MaxUnitCount></shop>\
             <contraptions /><contraptionIndex>0</contraptionIndex><bluepints /><researchQueue />\
             <energyTowerSkills /><towerStrengthenLevels><int>0</int><int>0</int></towerStrengthenLevels>\
             <constructionSnapshotDatas /><constructionIndex>0</constructionIndex>\
             <asynReinforcePools /><asynReinforcePoolOPs /><IsSpecialSupply>false</IsSpecialSupply>\
             </playerData><actionRecords /></PlayerRoundRecord>",
            next_index(placed)
        );
    }
    xml.push_str("</playerRoundRecords></PlayerRecord>");
}

fn next_index(units: &[Placement]) -> i32 {
    units
        .iter()
        .map(|unit| unit_index(unit) + 1)
        .max()
        .unwrap_or(0)
}

fn unit_index(unit: &Placement) -> i32 {
    unit.index
        .expect("a compiled unit carries its layout index")
}

fn write_units(xml: &mut String, units: &[Placement], sign: i32) {
    if units.is_empty() {
        xml.push_str("<units />");
        return;
    }
    xml.push_str("<units>");
    for unit in units {
        let NativeFormation::Unit(type_id) = unit.native else {
            unreachable!("a compiled unit is a native unit")
        };
        let _ = write!(
            xml,
            "<NewUnitData><id>{type_id}</id><Index>{}</Index><RoundCount>0</RoundCount>\
             <Durability>0</Durability><Exp>0</Exp><Level>0</Level><Position><x>{}</x>\
             <y>{}</y></Position><EquipmentID>0</EquipmentID><IsRotate>{}</IsRotate>\
             <SellSupply>0</SellSupply><equipments /></NewUnitData>",
            unit_index(unit),
            sign * unit.position.x,
            sign * unit.position.y,
            unit.rotated,
        );
    }
    xml.push_str("</units>");
}

const LIBRARY: &str = "GRCore, Version=0.0.0.0, Culture=neutral, PublicKeyToken=null";
const PLAYER_DATA: &str = "GameRiver.Replay+PlayerData";

/// The `BinaryFormatter` stream of one `GameRiver.Replay` holding `xml`, with
/// the two players the record names.
fn binary_formatter(version: i32, map_id: i32, xml: &[u8]) -> Vec<u8> {
    let list = format!("System.Collections.Generic.List`1[[{PLAYER_DATA}, {LIBRARY}]]");
    let mut out = Nrbf::default();
    // SerializationHeaderRecord: root object 1, header -1, format 1.0.
    out.byte(0x00).i32(1).i32(-1).i32(1).i32(0);
    // BinaryLibrary 2.
    out.byte(0x0c).i32(2).string(LIBRARY);
    // ClassWithMembersAndTypes 1: GameRiver.Replay.
    out.byte(0x05).i32(1).string("GameRiver.Replay").i32(7);
    for member in [
        "battleID",
        "version",
        "seat",
        "mapID",
        "playerDatas",
        "realBattleRecordData",
        "<CreateTime>k__BackingField",
    ] {
        out.string(member);
    }
    // String, Int32 x3, SystemClass, String, DateTime.
    out.bytes(&[1, 0, 0, 0, 3, 1, 0, 8, 8, 8])
        .string(&list)
        .byte(0x0d)
        .i32(2);
    out.byte(0x06).i32(3).string(BATTLE_ID);
    out.i32(version).i32(0).i32(map_id);
    out.byte(0x09).i32(4);
    out.byte(0x06).i32(5).length(xml.len()).bytes(xml);
    out.bytes(&0_u64.to_le_bytes());
    // SystemClassWithMembersAndTypes 4: the player list.
    out.byte(0x04).i32(4).string(&list).i32(3);
    out.string("_items").string("_size").string("_version");
    out.bytes(&[4, 0, 0])
        .string(&format!("{PLAYER_DATA}[]"))
        .i32(2)
        .bytes(&[8, 8]);
    out.byte(0x09).i32(6).i32(2).i32(2);
    // BinaryArray 6: single-dimensional, length 4, of PlayerData.
    out.byte(0x07)
        .i32(6)
        .byte(0)
        .i32(1)
        .i32(4)
        .byte(4)
        .string(PLAYER_DATA)
        .i32(2);
    out.byte(0x09).i32(7).byte(0x09).i32(8).byte(0x0d).byte(2);
    // ClassWithMembersAndTypes 7 and ClassWithId 8: the two players.
    out.byte(0x05).i32(7).string(PLAYER_DATA).i32(2);
    out.string("id").string("name").bytes(&[0, 1, 0x10]).i32(2);
    out.bytes(&1_u64.to_le_bytes())
        .byte(0x06)
        .i32(9)
        .string("blue");
    out.byte(0x01).i32(8).i32(7);
    out.bytes(&2_u64.to_le_bytes())
        .byte(0x06)
        .i32(10)
        .string("red");
    out.byte(0x0b);
    out.0
}

#[derive(Default)]
struct Nrbf(Vec<u8>);

impl Nrbf {
    fn byte(&mut self, value: u8) -> &mut Self {
        self.0.push(value);
        self
    }

    fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.0.extend_from_slice(value);
        self
    }

    fn i32(&mut self, value: i32) -> &mut Self {
        self.bytes(&value.to_le_bytes())
    }

    /// A length as `BinaryFormatter` prefixes strings with: seven bits a byte,
    /// low first, the high bit marking that another byte follows.
    fn length(&mut self, mut value: usize) -> &mut Self {
        loop {
            let low = u8::try_from(value & 0x7f).expect("seven bits fit a byte");
            value >>= 7;
            if value == 0 {
                return self.byte(low);
            }
            self.byte(low | 0x80);
        }
    }

    fn string(&mut self, value: &str) -> &mut Self {
        self.length(value.len()).bytes(value.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::compile;
    use serde_json::json;

    fn plan(layout: &serde_json::Value) -> Plan {
        compile(layout).unwrap()
    }

    fn rhino_mirror() -> serde_json::Value {
        json!({
            "kind": "layout",
            "seed": 4242,
            "round": 1,
            "blue": {"units": [{"name": "rhino", "index": 0, "position": {"x": 5, "y": -95}}]},
            "red": {"units": [{"name": "rhino", "index": 0, "position": {"x": 5, "y": -95}}]},
        })
    }

    fn embedded_xml(replay: &[u8]) -> &str {
        let start = replay
            .windows(3)
            .position(|window| window == b"\xef\xbb\xbf")
            .unwrap();
        let end = replay
            .windows(15)
            .position(|window| window == b"</BattleRecord>")
            .unwrap()
            + 15;
        std::str::from_utf8(&replay[start + 3..end]).unwrap()
    }

    #[test]
    fn the_record_string_is_prefixed_by_its_byte_length() {
        let replay = layout_replay(&plan(&rhino_mirror()), crate::game_build()).unwrap();
        let start = replay
            .windows(3)
            .position(|window| window == b"\xef\xbb\xbf")
            .unwrap();
        let end = replay
            .windows(15)
            .position(|window| window == b"</BattleRecord>")
            .unwrap()
            + 15;
        let mut prefix = Nrbf::default();
        prefix.length(end - start);
        assert_eq!(&replay[start - prefix.0.len()..start], prefix.0.as_slice());
        assert_eq!(replay.last(), Some(&0x0b));
    }

    #[test]
    fn red_is_written_in_the_board_frame() {
        let replay = layout_replay(&plan(&rhino_mirror()), crate::game_build()).unwrap();
        let xml = embedded_xml(&replay);
        assert!(xml.contains("<id>5</id><Index>0</Index>"));
        assert!(xml.contains("<Position><x>5</x><y>-95</y></Position>"));
        assert!(xml.contains("<Position><x>-5</x><y>95</y></Position>"));
        assert!(xml.contains("<SystemSeed>4242</SystemSeed>"));
        assert!(xml.contains("<MapID>1021</MapID>"));
        let build = crate::game_build().rsplit('.').next().unwrap();
        assert!(xml.contains(&format!("<Version>{build}</Version>")));
    }

    #[test]
    fn what_is_not_measured_is_refused_by_name() {
        let mut layout = rhino_mirror();
        layout["round"] = json!(2);
        layout["blue"]["units"][0]["level"] = json!(2);
        let error = layout_replay(&plan(&layout), crate::game_build()).unwrap_err();
        assert!(
            error.contains("side blue unit rhino at (5, -95) level"),
            "{error}"
        );
        assert!(error.contains("round 2 is not round 1"), "{error}");
    }

    #[test]
    fn a_layout_without_a_seed_is_refused() {
        let mut layout = rhino_mirror();
        layout.as_object_mut().unwrap().remove("seed");
        let error = layout_replay(&plan(&layout), crate::game_build()).unwrap_err();
        assert!(error.contains("needs a seed"), "{error}");
    }
}
