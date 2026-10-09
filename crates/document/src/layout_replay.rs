//! A layout written as a replay the game can fight.
//!
//! A replay's `PlayerRoundRecord` holds the position each side opens a round
//! with, and the game's replay starts a round from that snapshot rather than
//! from the rounds before it, then plays the round's recorded decisions on
//! it. So a layout is a replay whose layout round opens from a snapshot of
//! the layout's sides, and whose decisions are what a layout states as made
//! that round: releases, Energy Tower activations, the units that join a side
//! during the round, and the moves that put them in place. The fight the game
//! plays from it is the layout's fight.
//!
//! The fight draws only from streams the game derives from `SystemSeed` and
//! the round, so the record carries no random state. The rest of the record
//! is what the game needs to open it: two players, the map, and the 1v1
//! header constants. What a replay cannot open or play it refuses by name.
//!
//! The file around the record is a .NET `BinaryFormatter` stream of one
//! `GameRiver.Replay`, which holds the record as an XML string.
//! `docs/spec/document/layout-replay.md` states both.

use crate::catalog::NativeFormation;
use crate::compile::{Placement, Plan, SidePlan};
use crate::layout::{OilArea, Position, SHIELD_AIRDROP_SKILL, STICKY_OIL_BOMB_SKILL};
use crate::mobility::{DEPLOYMENT_MODULE, JUMP_DRIVES};
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
    let economy = crate::economy::Economy::embedded()?;
    let mut refusals = Vec::new();
    let mut deployments = Vec::new();
    for (name, side) in [("blue", &plan.blue), ("red", &plan.red)] {
        refusals.extend(side_refusals(name, side));
        match deployment(economy, side, plan.round) {
            Ok(deployment) => deployments.push(deployment),
            Err(reasons) => {
                refusals.extend(
                    reasons
                        .into_iter()
                        .map(|reason| format!("side {name} {reason}")),
                );
                deployments.push(Deployment::default());
            }
        }
    }
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
    write_record(&mut xml, plan, &deployments, seed, map_id, version);
    Ok(binary_formatter(BATTLE_ID, version, map_id, xml.as_bytes()))
}

fn side_refusals(name: &str, side: &SidePlan) -> Vec<String> {
    let mut refusals = Vec::new();
    let mut refuse = |present: bool, what: &str| {
        if present {
            refusals.push(format!("side {name} {what}"));
        }
    };
    for technology in &side.techs.units {
        refuse(
            crate::names::technology_owner(*technology).is_none(),
            &format!("technology {technology}, which names no unit"),
        );
    }
    refusals
}

/// The replay format's version: the build's last component.
pub(crate) fn build_number(game_build: &str) -> Result<i32, String> {
    game_build
        .rsplit('.')
        .next()
        .and_then(|number| number.parse().ok())
        .ok_or_else(|| format!("game build {game_build:?} does not end in a build number"))
}

fn write_record(
    xml: &mut String,
    plan: &Plan,
    deployments: &[Deployment],
    seed: i32,
    map_id: i32,
    version: i32,
) {
    xml.push_str(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<BattleRecord \
         xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><playerRecords>",
    );
    // Both sides are written in the board's frame, which is blue's own.
    let sides = [(1, "blue", &plan.blue, 1), (2, "red", &plan.red, -1)];
    for ((id, name, side, sign), deployment) in sides.into_iter().zip(deployments) {
        write_player(xml, id, name, side, deployment, sign, plan.round);
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
/// Supply enough for a round's upgrades, which the Training Ground's is too.
const UPGRADE_SUPPLY: i32 = 10_000;
const EMPTY_RANDOM: &str = "<randomStateData><randomStates /></randomStateData>";

fn write_player(
    xml: &mut String,
    id: u64,
    name: &str,
    side: &SidePlan,
    deployment: &Deployment,
    sign: i32,
    round: i32,
) {
    let _ = write!(
        xml,
        "<PlayerRecord><id>{id}</id><name>{name}</name><seed>0</seed><ad>0</ad><data>\
         <reactorCore>4500</reactorCore><MaxReactorCore>4500</MaxReactorCore>\
         <maxRoundSupply>4000</maxRoundSupply><firstRoundSupply>200</firstRoundSupply>\
         <roundSupplyIncreaseValue>200</roundSupplyIncreaseValue><team>0</team>\
         <isLeader>false</isLeader><type>Player</type>"
    );
    // The loadout a side brought: a technology is researched only out of it.
    write_technology_rows(xml, "unitDatas", "unitData", &side.techs.units);
    xml.push_str("<style><unitStyles /></style><plugins /></data><playerRoundRecords>");
    // Round 0 is the opening, which holds nothing; the layout's round opens
    // with the whole side.
    for snapshot in 0..=round {
        let _ = write!(
            xml,
            "<PlayerRoundRecord><round>{snapshot}</round><playerData>{EMPTY_RANDOM}\
             <reinforceRandomStateData><randomStates /></reinforceRandomStateData>\
             <reactorCore>4500</reactorCore><supply>{}</supply>\
             <preRoundFightResult>Win</preRoundFightResult>",
            if snapshot == round && !deployment.delivered.is_empty() {
                UPGRADE_SUPPLY
            } else {
                0
            }
        );
        if snapshot == round {
            write_side(xml, side, deployment, sign, round);
            xml.push_str("</playerData>");
            write_actions(xml, side, deployment, sign);
            xml.push_str("</PlayerRoundRecord>");
        } else {
            write_side(
                xml,
                &SidePlan::default(),
                &Deployment::default(),
                sign,
                round,
            );
            xml.push_str("</playerData><actionRecords /></PlayerRoundRecord>");
        }
    }
    xml.push_str("</playerRoundRecords></PlayerRecord>");
}

/// One side's `PlayerSnapshotData` after its random state, reactor, supply
/// and last result, in the order `XmlSerializer` writes its members.
fn write_side(xml: &mut String, side: &SidePlan, deployment: &Deployment, sign: i32, round: i32) {
    // A legacy unit the round sends travelling opens it on the other flank.
    let settled: Vec<(&Placement, Position)> = deployment
        .settled
        .iter()
        .map(|at| {
            let unit = &side.units[*at];
            let position = if deployment.redeployed.contains(at) {
                redeployed_from(unit.position)
            } else {
                unit.position
            };
            (unit, position)
        })
        .collect();
    write_units(xml, &settled, sign);
    let _ = write!(xml, "<unitIndex>{}</unitIndex>", deployment.next_unit);
    write_ints(xml, "officers", &side.techs.officers);
    xml.push_str("<mainEffects /><lastRoundSupply>0</lastRoundSupply><reinforceShopStrengthens />");
    write_battle_skill_panel(xml, side, sign, round);
    write_technologies(xml, &side.techs.units);
    // The inventory holds every item a side owns, the fitted ones among them.
    let fitted: Vec<i32> = side
        .units
        .iter()
        .flat_map(|unit| unit.equipment.iter().copied())
        .collect();
    if fitted.is_empty() {
        xml.push_str("<equipmentDatas />");
    } else {
        xml.push_str("<equipmentDatas>");
        for item in fitted {
            let _ = write!(
                xml,
                "<EquipmentData><id>{item}</id><durability>-1</durability></EquipmentData>"
            );
        }
        xml.push_str("</equipmentDatas>");
    }
    xml.push_str("<shop>");
    write_ints(xml, "unlockedUnits", &[]);
    let _ = write!(
        xml,
        "<lockedUnits />{EMPTY_RANDOM}<BuyCount>0</BuyCount>\
         <MaxUnlockCount>0</MaxUnlockCount><UnlockCount>0</UnlockCount>\
         <DiscountBalance>0</DiscountBalance><MaxUnitCount>0</MaxUnitCount></shop>"
    );
    write_contraptions(xml, &side.contraptions, sign);
    let _ = write!(
        xml,
        "<contraptionIndex>{}</contraptionIndex>",
        next_index(&side.contraptions)
    );
    let blueprints: Vec<i32> = side
        .techs
        .officers
        .iter()
        .filter_map(|officer| crate::catalog::chain_blueprint(*officer))
        .collect();
    write_ints(xml, "bluepints", &blueprints);
    xml.push_str("<researchQueue /><energyTowerSkills />");
    let mut levels = side.tower_strengthen_levels.clone();
    levels.resize(2, 0);
    write_ints(xml, "towerStrengthenLevels", &levels);
    write_constructions(xml, &side.constructions, sign);
    let _ = write!(
        xml,
        "<constructionIndex>{}</constructionIndex><asynReinforcePools />\
         <asynReinforcePoolOPs /><IsSpecialSupply>false</IsSpecialSupply>",
        next_index(&side.constructions)
    );
}

pub(crate) fn write_ints(xml: &mut String, tag: &str, values: &[i32]) {
    if values.is_empty() {
        let _ = write!(xml, "<{tag} />");
        return;
    }
    let _ = write!(xml, "<{tag}>");
    for value in values {
        let _ = write!(xml, "<int>{value}</int>");
    }
    let _ = write!(xml, "</{tag}>");
}

fn write_technologies(xml: &mut String, technologies: &[i32]) {
    write_technology_rows(xml, "activeTechnologies", "UnitData", technologies);
}

/// Technologies, one row per unit type that owns some.
pub(crate) fn write_technology_rows(xml: &mut String, tag: &str, row: &str, technologies: &[i32]) {
    let mut rows: Vec<(i32, Vec<i32>)> = Vec::new();
    for technology in technologies {
        let unit = crate::names::technology_owner(*technology)
            .expect("a refused technology never reaches the writer");
        match rows.iter_mut().find(|(row_unit, _)| *row_unit == unit) {
            Some((_, owned)) => owned.push(*technology),
            None => rows.push((unit, vec![*technology])),
        }
    }
    if rows.is_empty() {
        let _ = write!(xml, "<{tag} />");
        return;
    }
    let _ = write!(xml, "<{tag}>");
    for (unit, owned) in rows {
        let _ = write!(xml, "<{row}><id>{unit}</id><techs>");
        for technology in owned {
            let _ = write!(xml, "<tech data=\"{technology}\" />");
        }
        let _ = write!(xml, "</techs><unlockedTechs /></{row}>");
    }
    let _ = write!(xml, "</{tag}>");
}

fn unit_type(unit: &Placement) -> i32 {
    let NativeFormation::Unit(type_id) = unit.native else {
        unreachable!("a compiled unit is a native unit")
    };
    type_id
}

fn next_index(placements: &[Placement]) -> i32 {
    placements
        .iter()
        .map(|placement| index(placement) + 1)
        .max()
        .unwrap_or(0)
}

fn index(placement: &Placement) -> i32 {
    placement
        .index
        .expect("a compiled placement carries its layout index")
}

fn write_units(xml: &mut String, units: &[(&Placement, Position)], sign: i32) {
    if units.is_empty() {
        xml.push_str("<units />");
        return;
    }
    xml.push_str("<units>");
    for (unit, position) in units {
        let NativeFormation::Unit(type_id) = unit.native else {
            unreachable!("a compiled unit is a native unit")
        };
        // The record counts paid upgrades from zero, where a layout counts
        // levels from one, and holds the experience within the level.
        let _ = write!(
            xml,
            "<NewUnitData><id>{type_id}</id><Index>{}</Index><RoundCount>0</RoundCount>\
             <Durability>0</Durability><Exp>{}</Exp><Level>{}</Level><Position><x>{}</x>\
             <y>{}</y></Position><EquipmentID>0</EquipmentID><IsRotate>{}</IsRotate>\
             <SellSupply>0</SellSupply>",
            index(unit),
            unit.exp.unwrap_or(0),
            unit.level.unwrap_or(1) - 1,
            sign * position.x,
            sign * position.y,
            unit.rotated,
        );
        if unit.equipment.is_empty() {
            xml.push_str("<equipments />");
        } else {
            xml.push_str("<equipments>");
            for item in &unit.equipment {
                let _ = write!(xml, "<equipment data=\"{item}\" />");
            }
            xml.push_str("</equipments>");
        }
        xml.push_str("</NewUnitData>");
    }
    xml.push_str("</units>");
}

fn write_contraptions(xml: &mut String, contraptions: &[Placement], sign: i32) {
    if contraptions.is_empty() {
        xml.push_str("<contraptions />");
        return;
    }
    xml.push_str("<contraptions>");
    for contraption in contraptions {
        let NativeFormation::Contraption(type_id) = contraption.native else {
            unreachable!("a compiled contraption is a native contraption")
        };
        let _ = write!(
            xml,
            "<ContraptionData><index>{}</index><id>{type_id}</id><position><x>{}</x>\
             <y>{}</y></position></ContraptionData>",
            index(contraption),
            sign * contraption.position.x,
            sign * contraption.position.y,
        );
    }
    xml.push_str("</contraptions>");
}

/// Constructions, each with the one durability entry per segment the game
/// records: five for a wall, one for anything else.
fn write_constructions(xml: &mut String, constructions: &[Placement], sign: i32) {
    if constructions.is_empty() {
        xml.push_str("<constructionSnapshotDatas />");
        return;
    }
    xml.push_str("<constructionSnapshotDatas>");
    for construction in constructions {
        let NativeFormation::Construction(type_id) = construction.native else {
            unreachable!("a compiled construction is a native construction")
        };
        let segments = if type_id == DEFENSIVE_WALL { 5 } else { 1 };
        xml.push_str("<ConstructionSnapshotData><durability>");
        for _ in 0..segments {
            xml.push_str("<int>-1</int>");
        }
        let _ = write!(
            xml,
            "</durability><Index>{}</Index><ID>{type_id}</ID><Position><x>{}</x><y>{}</y>\
             </Position></ConstructionSnapshotData>",
            index(construction),
            sign * construction.position.x,
            sign * construction.position.y,
        );
    }
    xml.push_str("</constructionSnapshotDatas>");
}

const DEFENSIVE_WALL: i32 = 1;

/// The side's battle skill panel: one slot per release, in release order,
/// each ready this round, then one slot per object an earlier release left
/// standing, which the game restores from the slot's `rangeItems`.
fn write_battle_skill_panel(xml: &mut String, side: &SidePlan, sign: i32, round: i32) {
    let mut slots: Vec<(i32, String)> = side
        .battle_skills
        .iter()
        .map(|skill| (skill.commander_skill_id, String::from("<rangeItems />")))
        .collect();
    for center in &side.standing_shields {
        slots.push((
            SHIELD_AIRDROP_SKILL,
            format!(
                "<rangeItems>{}</rangeItems>",
                shield_range_data(*center, sign)
            ),
        ));
    }
    for area in &side.standing_oil {
        slots.push((STICKY_OIL_BOMB_SKILL, oil_range_item(area, sign)));
    }
    if slots.is_empty() {
        xml.push_str("<commanderSkills />");
        return;
    }
    xml.push_str("<commanderSkills>");
    for (slot, (skill, range_items)) in slots.iter().enumerate() {
        let _ = write!(
            xml,
            "<CommanderSkillData><index>{slot}</index><id>{skill}</id><isActive>true</isActive>\
             <coolingRound>0</coolingRound><getRound>{round}</getRound>{range_items}\
             </CommanderSkillData>"
        );
    }
    xml.push_str("</commanderSkills>");
}

/// The points a Sticky Oil Bomb's line expands into.
const TERRAIN_POINTS: u32 = 7;
/// The rounds a standing oil area has left as the round opens: it lasts two,
/// and the one that released it is over.
const TERRAIN_ROUNDS_LEFT: i32 = 1;

/// A standing oil area as its skill's range item: the two control points, the
/// points still standing, and the clipped grid of each that is not whole,
/// turned half a turn for red as its coordinates are.
fn oil_range_item(area: &OilArea, sign: i32) -> String {
    format!("<rangeItems>{}</rangeItems>", oil_range_data(area, sign))
}

/// One standing oil area's `CommanderSkillRangeItemData`.
pub(crate) fn oil_range_data(area: &OilArea, sign: i32) -> String {
    let active: Vec<bool> = (0..TERRAIN_POINTS)
        .map(|point| area.grid_rows.is_empty() || area.grid_rows.contains_key(&point))
        .collect();
    let mut grids = String::new();
    for point in (0..TERRAIN_POINTS).filter(|point| active[*point as usize]) {
        match area.grid_rows.get(&point).filter(|rows| !rows.is_empty()) {
            None => grids.push_str("<int>0</int>"),
            Some(rows) => {
                let rows = if sign < 0 {
                    crate::grbr::rotate_oil_grid_rows(rows)
                } else {
                    rows.clone()
                };
                let chunks = grid_chunks(&rows);
                let _ = write!(grids, "<int>{}</int>", chunks.len());
                for chunk in chunks {
                    let _ = write!(grids, "<int>{chunk}</int>");
                }
            }
        }
    }
    let mut positions = String::new();
    for point in &area.control_points {
        let _ = write!(
            positions,
            "<Vector2Int><x>{}</x><y>{}</y></Vector2Int>",
            sign * point.x,
            sign * point.y
        );
    }
    format!(
        "<CommanderSkillRangeItemData><positions>{positions}</positions>\
         <activeState>{}</activeState><gridInfo>{grids}</gridInfo><round>{TERRAIN_ROUNDS_LEFT}</round>\
         </CommanderSkillRangeItemData>",
        byte_mask(&active)
    )
}

/// One retained Shield Airdrop's `CommanderSkillRangeItemData`: one point,
/// always whole, and no lifetime.
pub(crate) fn shield_range_data(center: crate::layout::Position, sign: i32) -> String {
    format!(
        "<CommanderSkillRangeItemData><positions><Vector2Int><x>{}</x><y>{}</y></Vector2Int>\
         </positions><activeState>{}</activeState><gridInfo><int>0</int></gridInfo>\
         <round>0</round></CommanderSkillRangeItemData>",
        sign * center.x,
        sign * center.y,
        byte_mask(&[true])
    )
}

/// A 12 x 12 grid as the game stores it: its 144 cells column by column, in
/// masks of 31 cells and a last one of 20.
fn grid_chunks(rows: &[u32]) -> Vec<i32> {
    let cells: Vec<bool> = (0..144)
        .map(|cell: usize| rows[cell % 12] >> (cell / 12) & 1 == 1)
        .collect();
    cells.chunks(31).map(byte_mask).collect()
}

/// A run of flags as the game's `ByteMask` stores them: first flag highest,
/// under a set bit that marks the length. A run of 31 takes the sign bit as
/// that mark.
fn byte_mask(flags: &[bool]) -> i32 {
    let mut value: u32 = 1;
    for flag in flags {
        value = (value << 1) | u32::from(*flag);
    }
    value.cast_signed()
}

/// How a side's units reach the layout. A legacy unit, one below the
/// layout's `legacy_index`, opens the round in the snapshot, but for a squad
/// an officer hands out as the round opens, which the side then upgrades, fits
/// and moves. Every other unit joins during the round: `MAD_AddUnit` adds it
/// where the side deploys, as the Training Ground does, and the side fits it
/// and moves it into place. Moving onto a flank from elsewhere is what makes
/// a unit travel.
#[derive(Debug, Default)]
struct Deployment {
    /// The units the round opens with, as positions in the side's `units`.
    settled: Vec<usize>,
    /// Each delivered squad's unit and the level it arrives at, in the order
    /// the officers deliver.
    delivered: Vec<(usize, i32)>,
    /// The legacy units the round sends travelling, as positions in the
    /// side's `units`: each opens it on the other flank and moves back.
    redeployed: Vec<usize>,
    /// The units that join during the round, in index order.
    joined: Vec<usize>,
    /// The unit allocator the round opens with, which names what the
    /// officers deliver.
    next_unit: i32,
    /// Whether the units that join take the allocator's next indices, from
    /// `legacy_index` on, so that the allocator hands them out.
    allocated: bool,
}

/// The units of a side that its officers delivered as the round opened, as
/// positions in its `units`: the squad each officer's `opening_unit` names in
/// a round its `active_round` holds. The unit allocator names them in
/// delivery order as the round opens, so they are the side's last legacy
/// units.
///
/// # Errors
///
/// Returns why the side's units cannot be those squads, or that two officers
/// deliver in one round, which a standard 1v1 never deals.
pub fn delivered_units(side: &SidePlan, round: i32) -> Result<Vec<usize>, String> {
    let economy = crate::economy::Economy::embedded()?;
    delivered_squads(economy, side, round)
        .map(|delivered| delivered.into_iter().map(|(at, _)| at).collect())
        .map_err(|reasons| reasons.join("; "))
}

/// Each squad the side's officers deliver as the round opens, as the unit it
/// becomes and the level it arrives at, or why the side's units cannot be
/// them.
fn delivered_squads(
    economy: &crate::economy::Economy,
    side: &SidePlan,
    round: i32,
) -> Result<Vec<(usize, i32)>, Vec<String>> {
    let mut reasons = Vec::new();
    let squads: Vec<(i32, crate::economy::OpeningUnit)> = side
        .techs
        .officers
        .iter()
        .filter_map(|officer| {
            let row = economy.officer(*officer)?;
            let squad = row
                .opening_unit
                .filter(|_| row.active_round.contains(&round))?;
            Some((*officer, squad))
        })
        .collect();
    // Which of two officers delivers first is not recorded, and a standard
    // 1v1 never deals a side two officers that deliver in the same round.
    if squads.len() > 1 {
        let officers: Vec<String> = squads
            .iter()
            .map(|(officer, _)| officer.to_string())
            .collect();
        return Err(vec![format!(
            "officers {} each deliver a squad as round {round} opens: a standard 1v1 never \
             deals a side two officers that deliver in one round, and the order they deliver \
             in is not recorded",
            officers.join(" and ")
        )]);
    }
    let delivered = deliveries(side, &squads);
    if delivered.is_none() {
        let named: Vec<String> = squads
            .iter()
            .map(|(officer, squad)| {
                format!(
                    "officer {officer}'s level {} {} squad",
                    squad.level,
                    crate::catalog::unit_type_from_id(squad.unit)
                        .map_or("unknown", |(name, _)| name)
                )
            })
            .collect();
        reasons.push(format!(
            "units for {}, which round {round} delivers as it opens: the side needs a unit \
             of that type, at that level or above and without experience, for each, at \
             consecutive indices in the officers' order",
            named.join(" and ")
        ));
    }
    let delivered = delivered.unwrap_or_default();
    // The officers deliver as the round opens, so their squads are the last
    // units it opens with.
    let squad_count = i32::try_from(delivered.len()).unwrap_or(i32::MAX);
    if let Some((first, _)) = delivered.first()
        && index(&side.units[*first]) + squad_count != side.legacy_unit
    {
        reasons.push(format!(
            "officers' squads, delivered as the round opens from index {}: they are its last \
             legacy units, and legacy_index is {}",
            index(&side.units[*first]),
            side.legacy_unit
        ));
    }
    if reasons.is_empty() {
        Ok(delivered)
    } else {
        Err(reasons)
    }
}

/// Works out how a side's units reach the layout, or why some cannot.
fn deployment(
    economy: &crate::economy::Economy,
    side: &SidePlan,
    round: i32,
) -> Result<Deployment, Vec<String>> {
    let mut reasons = Vec::new();
    let delivered = delivered_squads(economy, side, round).unwrap_or_else(|mut refused| {
        reasons.append(&mut refused);
        Vec::new()
    });
    let legacy = side.legacy_unit;
    let squad_count = i32::try_from(delivered.len()).unwrap_or(i32::MAX);
    let is_delivered = |at: usize| delivered.iter().any(|(unit, _)| *unit == at);
    let settled: Vec<usize> = (0..side.units.len())
        .filter(|at| index(&side.units[*at]) < legacy && !is_delivered(*at))
        .collect();
    // A legacy unit travels only when its round moves it onto a flank from
    // another region, which only something that frees it to move allows.
    let mut redeployed = Vec::new();
    for at in &settled {
        let unit = &side.units[*at];
        if !unit.travelling {
            continue;
        }
        let freed = unit.equipment.contains(&DEPLOYMENT_MODULE)
            || JUMP_DRIVES
                .iter()
                .any(|(tech, name)| *name == unit.type_name && side.techs.units.contains(tech));
        if freed {
            redeployed.push(*at);
        } else {
            reasons.push(format!(
                "unit {} at ({}, {}) travelling: it is legacy, and nothing frees it to move \
                 onto a flank, neither a Deployment Module nor its Jump Drive",
                unit.type_name, unit.position.x, unit.position.y
            ));
        }
    }
    let mut joined: Vec<usize> = (0..side.units.len())
        .filter(|at| index(&side.units[*at]) >= legacy)
        .collect();
    joined.sort_by_key(|at| index(&side.units[*at]));
    for at in &joined {
        let unit = &side.units[*at];
        if unit.exp.is_some_and(|exp| exp != 0) {
            reasons.push(format!(
                "unit {} at ({}, {}) exp: it joins during the round, and a round's decisions \
                 cannot hand out experience",
                unit.type_name, unit.position.x, unit.position.y
            ));
        }
    }
    // A unit added at the index its action states leaves the allocator where
    // it was, and as the round ends the game enters the next round's
    // deployment, where an officer due then delivers its squad at the
    // allocator. So units that join at the allocator's next indices are
    // handed them by it, as buying them was; ones that skip an index, which
    // a unit sold in the round took, state theirs, which an officer due next
    // round would then collide with.
    let allocated = joined
        .iter()
        .zip(legacy..)
        .all(|(at, next)| index(&side.units[*at]) == next);
    if !allocated {
        for officer in delivering(economy, side, round + 1) {
            reasons.push(format!(
                "officer {officer}, who delivers a squad as round {} opens: the units that join \
                 skip an index, so they state theirs, which leaves the allocator at \
                 legacy_index, where one of them already stands",
                round + 1
            ));
        }
    }
    if !reasons.is_empty() {
        return Err(reasons);
    }
    Ok(Deployment {
        settled,
        delivered,
        redeployed,
        joined,
        next_unit: legacy - squad_count,
        allocated,
    })
}

/// The side's officers whose schedule delivers a squad as `round` opens.
fn delivering(economy: &crate::economy::Economy, side: &SidePlan, round: i32) -> Vec<i32> {
    side.techs
        .officers
        .iter()
        .copied()
        .filter(|officer| {
            economy
                .officer(*officer)
                .is_some_and(|row| row.opening_unit.is_some() && row.active_round.contains(&round))
        })
        .collect()
}

/// Which of the side's units each delivered squad becomes. The allocator
/// names the squads in delivery order, so they take consecutive indices from
/// the first. `None` when some squad has no unit to become.
fn deliveries(
    side: &SidePlan,
    squads: &[(i32, crate::economy::OpeningUnit)],
) -> Option<Vec<(usize, i32)>> {
    let Some((_, first)) = squads.first() else {
        return Some(Vec::new());
    };
    let fits = |at: usize, squad: &crate::economy::OpeningUnit| {
        let unit = &side.units[at];
        unit_type(unit) == squad.unit
            && unit.level.unwrap_or(1) >= squad.level
            && unit.exp.is_none_or(|exp| exp == 0)
    };
    (0..side.units.len())
        .filter(|at| fits(*at, first))
        .find_map(|start| {
            let base = index(&side.units[start]);
            squads
                .iter()
                .enumerate()
                .map(|(offset, (_, squad))| {
                    let wanted = base + i32::try_from(offset).ok()?;
                    let at = (0..side.units.len()).find(|at| index(&side.units[*at]) == wanted)?;
                    fits(at, squad).then_some((at, squad.level))
                })
                .collect()
        })
}

/// The round's decisions: each delivered squad fitted and moved into place,
/// each unit that joins added, fitted and moved into place, each Energy
/// Tower activation, then each battle skill's release in the layout's order,
/// which is the order its side's skills draw their scatter in, and the end of
/// the side's deployment.
fn write_actions(xml: &mut String, side: &SidePlan, deployment: &Deployment, sign: i32) {
    let mut actions: Vec<(&str, String)> = Vec::new();
    for (at, level) in &deployment.delivered {
        let unit = &side.units[*at];
        prepare(&mut actions, unit, *level);
        // Where the board landed the squad is the game's; the move only
        // needs where it goes.
        move_unit(&mut actions, unit, Position { x: 0, y: 0 }, sign);
    }
    for at in &deployment.redeployed {
        let unit = &side.units[*at];
        move_unit(&mut actions, unit, redeployed_from(unit.position), sign);
    }
    // The replay seats blue first, and each side's main deployment area is
    // its territory's region 1 for blue and 4 for red.
    let (room, main_region) = if sign == 1 { (0, 1) } else { (1, 4) };
    for at in &deployment.joined {
        let unit = &side.units[*at];
        let level = unit.level.unwrap_or(1);
        // A replay plays a match action as the test command that carries it,
        // as the Training Ground performs one.
        actions.push((
            "PAD_TestCommand",
            format!(
                "<Command xsi:type=\"MAD_AddUnit\"><Time>0</Time><LocalTime>0</LocalTime>\
                 <PIDX>{room}</PIDX><UID>{}</UID><Level>Level{level}</Level><UIDX>{}</UIDX>\
                 <IsFixedPosition>false</IsFixedPosition><Position><x>0</x><y>0</y></Position>\
                 <IsRotate>false</IsRotate><SellSupply>-1</SellSupply>\
                 <RegionID>{main_region}</RegionID></Command>",
                unit_type(unit),
                if deployment.allocated {
                    -1
                } else {
                    index(unit)
                },
            ),
        ));
        prepare(&mut actions, unit, level);
        move_unit(&mut actions, unit, Position { x: 0, y: 0 }, sign);
    }
    for skill in &side.energy_tower_skills {
        actions.push((
            "PAD_ActiveEnergyTowerSkill",
            format!("<SkillID>{skill}</SkillID>"),
        ));
    }
    for (slot, skill) in side.battle_skills.iter().enumerate() {
        let mut fields = format!("<ID>0</ID><SkillIndex>{slot}</SkillIndex><Positions>");
        for position in &skill.positions {
            let _ = write!(
                fields,
                "<MapVector><x>{}</x><y>{}</y></MapVector>",
                sign * position.x,
                sign * position.y
            );
        }
        fields.push_str(
            "</Positions><UnitIndex>-1</UnitIndex><ConstructionIndex>-1</ConstructionIndex>",
        );
        actions.push(("PAD_ReleaseCommanderSkill", fields));
    }
    if actions.is_empty() {
        xml.push_str("<actionRecords />");
        return;
    }
    // A side that records decisions ends its deployment by recording that it
    // did; without it the replay waits on the side for good.
    actions.push(("PAD_FinishDeploy", String::new()));
    xml.push_str("<actionRecords>");
    for (time, (kind, fields)) in actions.iter().enumerate() {
        let _ = write!(
            xml,
            "<MatchActionData xsi:type=\"{kind}\"><Time>{}</Time><LocalTime>0</LocalTime>\
             {fields}</MatchActionData>",
            time + 1
        );
    }
    xml.push_str("</actionRecords>");
}

/// Upgrades a unit that arrived at `from_level` to its layout level and fits
/// its equipment.
fn prepare(actions: &mut Vec<(&str, String)>, unit: &Placement, from_level: i32) {
    for _ in from_level..unit.level.unwrap_or(1) {
        actions.push((
            "PAD_UpgradeUnit",
            format!("<UIDX>{}</UIDX><UID>0</UID>", index(unit)),
        ));
    }
    for item in &unit.equipment {
        actions.push((
            "PAD_UseEquipment",
            format!(
                "<EquipmentID>{item}</EquipmentID><UnitIndex>{}</UnitIndex>",
                index(unit)
            ),
        ));
    }
}

/// Where a legacy unit the round sends travelling opens it: the same place
/// on the other flank, from which the move onto its own makes it travel, as a
/// Deployment Module's move across did in the match.
fn redeployed_from(position: Position) -> Position {
    Position {
        x: -position.x,
        y: position.y,
    }
}

/// Moves a unit from `from` to its layout position and facing.
fn move_unit(actions: &mut Vec<(&str, String)>, unit: &Placement, from: Position, sign: i32) {
    actions.push((
        "PAD_MoveUnit",
        format!(
            "<moveUnitDatas><MoveUnitData><unitID>{}</unitID><unitIndex>{}</unitIndex>\
             <position><x>{}</x><y>{}</y></position><isRotate>{}</isRotate>\
             <positionRecord><x>{}</x><y>{}</y></positionRecord><rotateRecord>false</rotateRecord>\
             <superDeployRecord>false</superDeployRecord></MoveUnitData></moveUnitDatas>",
            unit_type(unit),
            index(unit),
            sign * unit.position.x,
            sign * unit.position.y,
            unit.rotated,
            sign * from.x,
            sign * from.y,
        ),
    ));
}

const LIBRARY: &str = "GRCore, Version=0.0.0.0, Culture=neutral, PublicKeyToken=null";
const PLAYER_DATA: &str = "GameRiver.Replay+PlayerData";

/// The `BinaryFormatter` stream of one `GameRiver.Replay` holding `xml`, with
/// the two players the record names.
pub(crate) fn binary_formatter(battle_id: &str, version: i32, map_id: i32, xml: &[u8]) -> Vec<u8> {
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
    out.byte(0x06).i32(3).string(battle_id);
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
        // Each Rhino joins during the round, at the index the allocator hands
        // it, and is moved into place.
        assert_eq!(
            xml.matches("<UID>5</UID><Level>Level1</Level><UIDX>-1</UIDX>")
                .count(),
            2
        );
        assert!(xml.contains("<position><x>5</x><y>-95</y></position>"));
        assert!(xml.contains("<position><x>-5</x><y>95</y></position>"));
        assert!(xml.contains("<SystemSeed>4242</SystemSeed>"));
        assert!(xml.contains("<MapID>1021</MapID>"));
        let build = crate::game_build().rsplit('.').next().unwrap();
        assert!(xml.contains(&format!("<Version>{build}</Version>")));
    }

    #[test]
    fn what_is_not_measured_is_refused_by_name() {
        let mut layout = rhino_mirror();
        layout["round"] = json!(3);
        layout["blue"]["units"] = json!([{
            "name": "marksman", "index": 0, "travelling": true, "exp": "100/650",
            "position": {"x": -330, "y": 100},
        }]);
        let error = layout_replay(&plan(&layout), crate::game_build()).unwrap_err();
        assert!(
            error.contains("side blue unit marksman at (-330, 100) exp"),
            "{error}"
        );
    }

    #[test]
    fn a_clipped_grid_reads_back_through_the_replay_reader() {
        let rows = vec![
            240, 1020, 2046, 2046, 4095, 4095, 1023, 511, 254, 126, 60, 48,
        ];
        let chunks = grid_chunks(&rows);
        let mut info = vec![i32::try_from(chunks.len()).unwrap()];
        info.extend(&chunks);
        assert_eq!(
            crate::grbr::decode_grbr_grid_groups(&info).unwrap(),
            vec![rows]
        );
        assert_eq!(
            crate::grbr::decode_grbr_byte_mask(byte_mask(&[true, false, true])).unwrap(),
            vec![true, false, true]
        );
    }

    #[test]
    fn a_squad_an_officer_delivers_is_a_unit_of_the_layout() {
        let layout = |units: serde_json::Value| {
            json!({
                "kind": "layout",
                "seed": 4242,
                "round": 2,
                "blue": {"officers": ["marksman_specialist"], "legacy_index": 2, "units": units},
                "red": {"units": [{"name": "arclight", "index": 0, "position": {"x": 0, "y": -50}}]},
            })
        };
        let lone =
            layout(json!([{"name": "marksman", "index": 0, "position": {"x": 0, "y": -50}}]));
        let error = layout_replay(&plan(&lone), crate::game_build()).unwrap_err();
        assert!(error.contains("level 3 marksman squad"), "{error}");
        let held = layout(json!([
            {"name": "marksman", "index": 0, "position": {"x": 0, "y": -50}},
            {"name": "marksman", "index": 1, "level": 4, "position": {"x": -100, "y": -100}},
        ]));
        let replay = layout_replay(&plan(&held), crate::game_build()).unwrap();
        let xml = embedded_xml(&replay);
        // The round opens with the allocator at the squad's index, which the
        // delivery takes, and the round upgrades and moves it.
        assert!(xml.contains("<unitIndex>1</unitIndex>"));
        assert!(xml.contains("xsi:type=\"PAD_UpgradeUnit\""));
        assert!(xml.contains("<position><x>-100</x><y>-100</y></position>"));
        // The squad is the last unit the round opens with.
        let mut joined = held;
        joined["blue"]["legacy_index"] = json!(1);
        let error = layout_replay(&plan(&joined), crate::game_build()).unwrap_err();
        assert!(error.contains("last legacy units"), "{error}");
    }

    #[test]
    fn a_unit_that_joins_during_the_round_is_added_in_index_order() {
        let travelling = |index: i32, x: i32, y: i32| json!({"name": "marksman", "index": index, "travelling": true, "position": {"x": x, "y": y}});
        let layout = json!({
            "kind": "layout", "seed": 4242, "round": 3,
            "blue": {"legacy_index": 1, "units": [
                {"name": "marksman", "index": 0, "position": {"x": 0, "y": -50}},
                travelling(1, -330, 100),
                {"name": "marksman", "index": 2, "position": {"x": 100, "y": -60}},
                travelling(3, 330, 100),
            ]},
            "red": {"legacy_index": 1, "units": [{"name": "arclight", "index": 0, "position": {"x": 0, "y": -50}}]},
        });
        let replay = layout_replay(&plan(&layout), crate::game_build()).unwrap();
        let xml = embedded_xml(&replay);
        // The legacy unit opens the round, and the allocator after it hands
        // each joining unit its index, in index order.
        assert!(xml.contains("<unitIndex>1</unitIndex>"));
        assert_eq!(xml.matches("<UIDX>-1</UIDX><IsFixedPosition>").count(), 3);
        let moved: Vec<usize> = (1..=3)
            .map(|index| {
                xml.find(&format!("</unitID><unitIndex>{index}</unitIndex>"))
                    .expect("each joining unit is moved")
            })
            .collect();
        assert!(moved.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(xml.matches("xsi:type=\"MAD_AddUnit\"").count(), 3);
        assert!(!xml.contains("PAD_BuyUnit"));
    }

    /// Units that skip an index state theirs, which leaves the allocator
    /// behind them, so an officer that delivers as the next round opens would
    /// take an index one of them stands at.
    #[test]
    fn units_that_skip_an_index_state_theirs() {
        let unit = |index: i32, x: i32| json!({"name": "marksman", "index": index, "position": {"x": x, "y": -60}});
        let layout = |officers: serde_json::Value| {
            json!({
                "kind": "layout", "seed": 4242, "round": 1,
                "blue": {"officers": officers, "units": [unit(0, 0), unit(2, 100)]},
                "red": {"units": [{"name": "arclight", "index": 0, "position": {"x": 0, "y": -50}}]},
            })
        };
        let replay = layout_replay(&plan(&layout(json!([]))), crate::game_build()).unwrap();
        let xml = embedded_xml(&replay);
        for index in [0, 2] {
            assert!(xml.contains(&format!("<UIDX>{index}</UIDX><IsFixedPosition>")));
        }
        // Red's one unit takes the allocator's first index.
        assert!(xml.contains("<UIDX>-1</UIDX><IsFixedPosition>"));
        let error = layout_replay(
            &plan(&layout(json!(["marksman_specialist"]))),
            crate::game_build(),
        )
        .unwrap_err();
        assert!(
            error.contains("delivers a squad as round 2 opens"),
            "{error}"
        );
    }

    /// A legacy unit that travels opens the round on the other flank and
    /// moves back, which only a Deployment Module or its Jump Drive allows.
    #[test]
    fn a_legacy_unit_that_travels_opens_on_the_other_flank() {
        let layout = |equipment: serde_json::Value| {
            json!({
                "kind": "layout", "seed": 4242, "round": 3,
                "blue": {"legacy_index": 1, "units": [
                    {"name": "wasp", "index": 0, "travelling": true, "equipment": equipment, "position": {"x": 330, "y": 285}},
                ]},
                "red": {"legacy_index": 1, "units": [{"name": "arclight", "index": 0, "position": {"x": 0, "y": -50}}]},
            })
        };
        let replay = layout_replay(
            &plan(&layout(json!(["deployment_module"]))),
            crate::game_build(),
        )
        .unwrap();
        let xml = embedded_xml(&replay);
        assert!(xml.contains("<Position><x>-330</x><y>285</y></Position>"));
        assert!(xml.contains(
            "<position><x>330</x><y>285</y></position><isRotate>false</isRotate>\
             <positionRecord><x>-330</x><y>285</y></positionRecord>"
        ));
        let error = layout_replay(&plan(&layout(json!([]))), crate::game_build()).unwrap_err();
        assert!(error.contains("nothing frees it to move"), "{error}");
    }

    #[test]
    fn a_layout_without_a_seed_is_refused() {
        let mut layout = rhino_mirror();
        layout.as_object_mut().unwrap().remove("seed");
        let error = layout_replay(&plan(&layout), crate::game_build()).unwrap_err();
        assert!(error.contains("needs a seed"), "{error}");
    }
}
