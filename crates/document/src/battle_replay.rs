//! A battle written back as the replay it converts from.
//!
//! `mechcore replay convert` reads a replay into a battle; this writes a
//! battle into a replay that the same conversion reads back as that battle.
//! Each of a replay's rounds holds the position a side opens the round with,
//! taken before the round's own opening, and the decisions it takes from
//! there. A battle's state segment is the position after the opening, so a
//! round is written by undoing the opening: taking back what the round's
//! officers deliver, the round's income and cooldown count-down, and the
//! energy tower debt the previous round owes. Each undone position is opened
//! again with the conversion's own rule, and a position that does not open
//! onto the battle's is refused rather than written.
//!
//! The random streams a replay records are where the battle's seeds put them.
//! `docs/spec/document/battle-replay.md` states what the file holds.

use crate::battle::{Action, SideState, SkillTarget, Turn};
use crate::economy::Economy;
use crate::layout::Position;
use crate::layout_replay::{
    SHIELD_AIRDROP_SKILL, binary_formatter, build_number, shield_range_data, terrain_range_data,
    write_ints, write_technology_rows,
};
use crate::opening::{Stated, StatedSide, Stream};
use std::fmt::Write as _;

/// Energy tower skill `1`, whose price the next round's income repays.
const RAPID_SUPPLY_SKILL: i32 = 1;
const BATTLE_ID: &str = "battle";

/// Writes `stated` as a replay `replay convert` reads back as the same battle.
///
/// # Errors
///
/// Returns an error when the battle holds what a replay cannot record: a
/// deployment clock, a position after its last decisions, a side without an
/// opening or a seed, or a round whose opening cannot be undone onto a
/// position that opens back onto the battle's.
pub fn battle_replay(
    economy: &Economy,
    stated: &Stated,
    game_build: &str,
) -> Result<Vec<u8>, String> {
    if stated.deploy_time.is_some() {
        return Err("a replay records no deployment clock, and this battle states one".into());
    }
    if !stated.ends_on_actions {
        return Err(
            "a replay records no position after its last decisions, and this battle states one"
                .into(),
        );
    }
    let version = build_number(game_build)?;
    let match_states = match_states(economy, stated)?;
    let mut xml = String::from("\u{feff}");
    xml.push_str(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<BattleRecord \
         xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><playerRecords>",
    );
    for (id, name, side, sign) in [(1, "blue", &stated.blue, 1), (2, "red", &stated.red, -1)] {
        write_player(&mut xml, economy, stated, id, name, side, sign)?;
    }
    xml.push_str("</playerRecords><matchDatas>");
    for (round, state) in match_states.iter().enumerate() {
        let offers = usize::checked_sub(round, 1)
            .and_then(|at| stated.turns.get(at))
            .and_then(|turn| turn.state.reinforce_offers.as_ref());
        let _ = write!(xml, "<MatchSnapshotData><round>{round}</round>");
        write_random(&mut xml, "randomStateData", Some(state));
        match offers {
            Some(offers) => {
                xml.push_str("<reinforceItems>");
                write_ints(&mut xml, "ArrayOfInt", &offers.dealt);
                xml.push_str("</reinforceItems>");
            }
            None => xml.push_str("<reinforceItems />"),
        }
        xml.push_str(
            "<teamRanks><int>0</int><int>1</int></teamRanks><useConstruction>false</useConstruction>\
             <poolOPs /><deadCount>0</deadCount><RoundExcludeReinforce /></MatchSnapshotData>",
        );
    }
    let _ = write!(
        xml,
        "</matchDatas><reinforceItems /><Version>{version}</Version><Seat>0</Seat><BattleInfo>\
         <gameRules /><StartTime>0</StartTime><SystemSeed>{}</SystemSeed>\
         <BattleID>{BATTLE_ID}</BattleID><PrepareTime>30</PrepareTime><DeployTime>100</DeployTime>\
         <FightTime>120</FightTime><MapID>{}</MapID><MaxRound>40</MaxRound>\
         <BlueprintIncreaseSupply>0</BlueprintIncreaseSupply>\
         <EnableAdvanceTeam>true</EnableAdvanceTeam><EnableReinforcement>true</EnableReinforcement>\
         <EnableUnitReinforcement>true</EnableUnitReinforcement>\
         <EnableConstruction>true</EnableConstruction><GameMode>Normal</GameMode>\
         <MatchMode>VS_1_1</MatchMode><ScoreMode>ReduceScore</ScoreMode><HostID>0</HostID>\
         <SurviveModeDifficulty>VeryEasy</SurviveModeDifficulty></BattleInfo>\
         <CreateTime>0001-01-01T00:00:00</CreateTime></BattleRecord>",
        stated.seed, stated.map_id
    );
    Ok(binary_formatter(
        BATTLE_ID,
        version,
        stated.map_id,
        xml.as_bytes(),
    ))
}

/// The match's reinforcement stream as each round opened. Round 0 holds the
/// seed's own state, from which the opening is dealt; a round that deals
/// offers holds the state its deal starts from, and the round after it the
/// state the deal ends on. A round that deals nothing holds the one before it.
fn match_states(economy: &Economy, stated: &Stated) -> Result<Vec<[u64; 4]>, String> {
    let seeded = crate::opening::initialize(stated.seed)?.stream.state();
    let deal = crate::opening::verify(economy, stated)
        .and_then(|opening| crate::reinforcement::verify(economy, stated, &opening))
        .ok();
    let dealt = |round: i32| {
        deal.as_ref()
            .and_then(|deal| deal.rounds.iter().find(|entry| entry.round == round))
    };
    let mut recorded = vec![seeded];
    for turn in &stated.turns {
        let previous = *recorded.last().expect("round 0 is written");
        let state = dealt(turn.round).map_or_else(
            || dealt(turn.round - 1).map_or(previous, |entry| entry.after_state),
            |entry| entry.before_state,
        );
        recorded.push(state);
    }
    Ok(recorded)
}

fn write_random(xml: &mut String, tag: &str, state: Option<&[u64; 4]>) {
    let _ = write!(xml, "<{tag}>");
    match state {
        Some(words) => {
            xml.push_str("<randomStates>");
            for word in words {
                let _ = write!(xml, "<unsignedLong>{word}</unsignedLong>");
            }
            xml.push_str("</randomStates>");
        }
        None => xml.push_str("<randomStates />"),
    }
    let _ = write!(xml, "</{tag}>");
}

fn write_player(
    xml: &mut String,
    economy: &Economy,
    stated: &Stated,
    id: u64,
    name: &str,
    side: &StatedSide,
    sign: i32,
) -> Result<(), String> {
    let red = sign < 0;
    let seed = side
        .seed
        .ok_or_else(|| format!("{name} states no seed, and a replay records each side's"))?;
    let opening = side
        .opening
        .as_ref()
        .ok_or_else(|| format!("{name} takes no opening, and a replay records one"))?;
    let shared = economy.round_supply();
    let reactor = crate::opening::reactor_core(stated.map_id, usize::from(red))?;
    let _ = write!(
        xml,
        "<PlayerRecord><id>{id}</id><name>{name}</name><seed>{seed}</seed><ad>0</ad><data>\
         <reactorCore>{reactor}</reactorCore><MaxReactorCore>{reactor}</MaxReactorCore>\
         <maxRoundSupply>{}</maxRoundSupply><firstRoundSupply>{}</firstRoundSupply>\
         <roundSupplyIncreaseValue>{}</roundSupplyIncreaseValue><team>0</team>\
         <isLeader>false</isLeader><type>Player</type><unitDatas>",
        shared.max, shared.first, shared.increase
    );
    for (unit, techs) in &side.tech_loadout {
        let _ = write!(xml, "<unitData><id>{unit}</id><techs>");
        for tech in techs {
            let _ = write!(xml, "<tech data=\"{tech}\" />");
        }
        xml.push_str("</techs><unlockedTechs /></unitData>");
    }
    xml.push_str("</unitDatas><style><unitStyles /></style><plugins /></data><playerRoundRecords>");

    // Round 0: nothing held, and the opening's one decision.
    let mut stream = Stream::seeded(seed);
    xml.push_str("<PlayerRoundRecord><round>0</round><playerData>");
    let empty = crate::transition::before_opening(reactor, side.constructions.clone());
    write_snapshot(xml, &empty, &[], sign, 0, &stream.state())?;
    let team = side
        .offers
        .get(usize::try_from(opening.choose).unwrap_or(usize::MAX))
        .map(|offer| offer.team)
        .ok_or_else(|| {
            format!(
                "{name} takes opening {}, which it is not dealt",
                opening.choose
            )
        })?;
    let _ = write!(
        xml,
        "</playerData><actionRecords>\
         <MatchActionData xsi:type=\"PAD_ChooseAdvanceTeam\"><Time>1</Time><LocalTime>0</LocalTime>\
         <Index>{}</Index><ID>{team}</ID></MatchActionData>\
         <MatchActionData xsi:type=\"PAD_FinishDeploy\"><Time>2</Time><LocalTime>0</LocalTime>\
         </MatchActionData></actionRecords></PlayerRoundRecord>",
        opening.choose
    );

    let mut previous: &[Action] = &[];
    let mut earlier_officers: Vec<i32> = Vec::new();
    for turn in &stated.turns {
        let state = if red {
            &turn.state.red
        } else {
            &turn.state.blue
        };
        let actions = if red {
            &turn.actions.red
        } else {
            &turn.actions.blue
        };
        stream.skip(crate::transition::player_draws(
            economy,
            &earlier_officers,
            turn.round - 1,
        ));
        let snapshot = unopen(economy, state, turn.round, previous, red, stream)
            .map_err(|error| format!("round {} {name}: {error}", turn.round))?;
        let _ = write!(
            xml,
            "<PlayerRoundRecord><round>{}</round><playerData>",
            turn.round
        );
        let chains: Vec<i32> = snapshot
            .blueprints
            .iter()
            .filter_map(|blueprint| crate::catalog::chain_officer(*blueprint))
            .collect();
        write_snapshot(xml, &snapshot, &chains, sign, turn.round, &stream.state())?;
        xml.push_str("</playerData>");
        // The round's decisions are taken from the position it opened with,
        // deliveries and all, which is the battle's.
        write_actions(xml, economy, state, turn, actions, sign)?;
        xml.push_str("</PlayerRoundRecord>");
        earlier_officers.clone_from(&snapshot.officers);
        previous = actions;
    }
    xml.push_str("</playerRoundRecords></PlayerRecord>");
    Ok(())
}

/// The position a side's round opened from, before the round's own opening:
/// what `open_round` makes `state` of.
fn unopen(
    economy: &Economy,
    state: &SideState,
    round: i32,
    previous: &[Action],
    red: bool,
    stream: Stream,
) -> Result<SideState, String> {
    let mut snapshot = take_back_deliveries(economy, state, round, stream)?;
    undo_reset(economy, &mut snapshot, round, previous);
    // The conversion's own opening has to make the battle's position of it.
    let mut placement = crate::landing::placement(red);
    let opened =
        crate::transition::open_round(economy, &snapshot, round, &mut placement, Some(stream))
            .map_err(|reason| format!("the undone position does not open: {reason:?}"))?;
    if &opened != state {
        return Err("the undone position does not open onto the battle's".into());
    }
    Ok(snapshot)
}

/// Takes back what the round's officers deliver as it opens: the skills they
/// add to the panel, the items they add to the inventory, the squads they
/// hand out and the units they unlock.
fn take_back_deliveries(
    economy: &Economy,
    state: &SideState,
    round: i32,
    stream: Stream,
) -> Result<SideState, String> {
    let mut snapshot = state.clone();
    let mut squads = 0;
    // The officers that draw what they hand out draw in the officers' order,
    // one value each, from the side's stream as the round opens.
    let mut picker = stream;
    let handed_out: Vec<(i32, i32)> = state
        .officers
        .iter()
        .filter_map(|officer| {
            let row = economy.officer(*officer)?;
            (row.random_equipment && !row.equipment.is_empty() && row.active_round.contains(&round))
                .then(|| (*officer, row.equipment[picker.pick(0, row.equipment.len())]))
        })
        .collect();
    for officer in &state.officers {
        let Some(row) = economy.officer(*officer) else {
            continue;
        };
        if let Some(squad) = row.opening_unit
            && squad.unlock_round == round
        {
            snapshot.unlocked_units.retain(|unit| *unit != squad.unit);
        }
        if !row.active_round.contains(&round) {
            continue;
        }
        for skill in &row.commander_skills {
            let slot = snapshot
                .battle_skills
                .iter()
                .rposition(|slot| slot.id == *skill && slot.cooldown == 0)
                .ok_or_else(|| {
                    format!("officer {officer} delivers skill {skill}, which the panel lacks")
                })?;
            snapshot.battle_skills.remove(slot);
        }
        if row.random_equipment && !row.equipment.is_empty() {
            // Which item the draw handed out is the side's stream's to say.
            let item = handed_out
                .iter()
                .find(|(drawer, _)| drawer == officer)
                .map(|(_, item)| *item)
                .ok_or_else(|| format!("officer {officer} draws no item"))?;
            take_item(&mut snapshot, item, *officer)?;
        } else {
            for item in &row.equipment {
                take_item(&mut snapshot, *item, *officer)?;
            }
        }
        if row.opening_unit.is_some() {
            squads += 1;
        }
    }
    snapshot.next_index.unit -= squads;
    let first_delivered = snapshot.next_index.unit;
    snapshot
        .units
        .retain(|entry| entry.unit.index < first_delivered);
    Ok(snapshot)
}

/// Undoes the round's reset: a slot the previous round spent restarts as the
/// round opens and every other counts down, and the income, less what the
/// previous round's Rapid Resupply still owes, is paid.
fn undo_reset(economy: &Economy, snapshot: &mut SideState, round: i32, previous: &[Action]) {
    let spent: Vec<i32> = previous
        .iter()
        .filter_map(|action| match action {
            Action::ReleaseCommanderSkill { index, .. } => Some(*index),
            _ => None,
        })
        .collect();
    for slot in &mut snapshot.battle_skills {
        slot.used = spent.contains(&slot.index);
        slot.cooldown = if slot.used || slot.cooldown == 0 {
            0
        } else {
            slot.cooldown + 1
        };
    }
    let repays = previous.iter().any(|action| {
        matches!(action, Action::ActiveEnergyTowerSkill { skill } if *skill == RAPID_SUPPLY_SKILL)
    });
    snapshot.energy_tower_skills = if repays {
        vec![RAPID_SUPPLY_SKILL]
    } else {
        Vec::new()
    };
    let debt: i32 = snapshot
        .energy_tower_skills
        .iter()
        .filter_map(|skill| economy.energy_tower_skill(*skill))
        .map(|skill| skill.owed)
        .sum();
    let worn: i32 = snapshot
        .units
        .iter()
        .flat_map(|entry| entry.unit.equipment.iter())
        .map(|item| economy.equipment_round_supply(*item))
        .sum();
    let income =
        crate::ledger::round_income(economy, round, &snapshot.officers, economy.round_supply());
    snapshot.supply -= income + worn - debt;
    snapshot.shop = crate::battle::Allowances::default();
    for entry in &mut snapshot.units {
        entry.movable = false;
    }
}

fn take_item(snapshot: &mut SideState, item: i32, officer: i32) -> Result<(), String> {
    let at = snapshot
        .equipment
        .iter()
        .rposition(|held| held.id == item)
        .ok_or_else(|| {
            format!("officer {officer} delivers item {item}, which the side does not hold")
        })?;
    snapshot.equipment.remove(at);
    Ok(())
}

/// One `PlayerSnapshotData` body.
fn write_snapshot(
    xml: &mut String,
    side: &SideState,
    chain_officers: &[i32],
    sign: i32,
    round: i32,
    stream: &[u64; 4],
) -> Result<(), String> {
    write_random(xml, "randomStateData", Some(stream));
    write_random(xml, "reinforceRandomStateData", None);
    let _ = write!(
        xml,
        "<reactorCore>{}</reactorCore><supply>{}</supply><preRoundFightResult>Win</preRoundFightResult>",
        side.reactor_core, side.supply
    );
    write_units(xml, side, sign)?;
    let _ = write!(xml, "<unitIndex>{}</unitIndex>", side.next_index.unit);
    let mut officers = side.officers.clone();
    officers.extend_from_slice(chain_officers);
    officers.sort_unstable();
    write_ints(xml, "officers", &officers);
    xml.push_str("<mainEffects /><lastRoundSupply>0</lastRoundSupply><reinforceShopStrengthens />");
    write_panel(xml, side, sign, round)?;
    write_technology_rows(xml, "activeTechnologies", "UnitData", &side.techs);
    write_inventory(xml, side);
    xml.push_str("<shop>");
    write_ints(xml, "unlockedUnits", &side.unlocked_units);
    xml.push_str(
        "<lockedUnits /><randomStateData><randomStates /></randomStateData><BuyCount>0</BuyCount>\
         <MaxUnlockCount>0</MaxUnlockCount><UnlockCount>0</UnlockCount>\
         <DiscountBalance>0</DiscountBalance><MaxUnitCount>0</MaxUnitCount></shop>",
    );
    write_contraptions(xml, side, sign)?;
    write_ints(xml, "bluepints", &side.blueprints);
    xml.push_str("<researchQueue />");
    write_ints(xml, "energyTowerSkills", &side.energy_tower_skills);
    write_ints(xml, "towerStrengthenLevels", &side.tower_strengthen_levels);
    write_constructions(xml, side, sign)?;
    xml.push_str(
        "<asynReinforcePools /><asynReinforcePoolOPs /><IsSpecialSupply>false</IsSpecialSupply>",
    );
    Ok(())
}

fn write_units(xml: &mut String, side: &SideState, sign: i32) -> Result<(), String> {
    if side.units.is_empty() {
        xml.push_str("<units />");
        return Ok(());
    }
    xml.push_str("<units>");
    for entry in &side.units {
        let unit = &entry.unit;
        let type_id = crate::catalog::unit_id_from_type(&unit.type_name)
            .ok_or_else(|| format!("unit type {:?} has no ID", unit.type_name))?;
        let _ = write!(
            xml,
            "<NewUnitData><id>{type_id}</id><Index>{}</Index><RoundCount>0</RoundCount>\
             <Durability>0</Durability><Exp>{}</Exp><Level>{}</Level><Position>{}</Position>\
             <EquipmentID>0</EquipmentID><IsRotate>{}</IsRotate><SellSupply>{}</SellSupply>",
            unit.index,
            unit.exp.map_or(0, |exp| exp.current),
            unit.level.unwrap_or(1) - 1,
            board(unit.position, sign),
            unit.rotated.unwrap_or(false),
            entry.value.unwrap_or(0),
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
    Ok(())
}

/// The inventory: every item the side owns, the fitted ones among them.
fn write_inventory(xml: &mut String, side: &SideState) {
    let mut items: Vec<(i32, Option<i32>)> = side
        .equipment
        .iter()
        .map(|item| (item.id, item.durability))
        .collect();
    items.extend(
        side.units
            .iter()
            .flat_map(|entry| entry.unit.equipment.iter().map(|id| (*id, None))),
    );
    if items.is_empty() {
        xml.push_str("<equipmentDatas />");
        return;
    }
    xml.push_str("<equipmentDatas>");
    for (id, durability) in items {
        let _ = write!(
            xml,
            "<EquipmentData><id>{id}</id><durability>{}</durability></EquipmentData>",
            durability.unwrap_or(-1)
        );
    }
    xml.push_str("</equipmentDatas>");
}

fn write_contraptions(xml: &mut String, side: &SideState, sign: i32) -> Result<(), String> {
    if side.contraptions.is_empty() {
        xml.push_str("<contraptions />");
    } else {
        xml.push_str("<contraptions>");
        for contraption in &side.contraptions {
            let id = native_id(crate::catalog::resolve_contraption_type(
                &contraption.type_name,
            ))
            .ok_or_else(|| format!("contraption type {:?} has no ID", contraption.type_name))?;
            let _ = write!(
                xml,
                "<ContraptionData><index>{}</index><id>{id}</id><position>{}</position>\
                 </ContraptionData>",
                contraption.index,
                board(contraption.position, sign)
            );
        }
        xml.push_str("</contraptions>");
    }
    let _ = write!(
        xml,
        "<contraptionIndex>{}</contraptionIndex>",
        side.next_index.contraption
    );
    Ok(())
}

/// Constructions, each with the one durability entry per segment the game
/// records: five for a wall, one for anything else.
fn write_constructions(xml: &mut String, side: &SideState, sign: i32) -> Result<(), String> {
    if side.constructions.is_empty() {
        xml.push_str("<constructionSnapshotDatas />");
    } else {
        xml.push_str("<constructionSnapshotDatas>");
        for construction in &side.constructions {
            let id = native_id(crate::catalog::resolve_construction_type(
                &construction.type_name,
            ))
            .ok_or_else(|| format!("construction type {:?} has no ID", construction.type_name))?;
            let segments = if id == DEFENSIVE_WALL { 5 } else { 1 };
            xml.push_str("<ConstructionSnapshotData><durability>");
            for _ in 0..segments {
                xml.push_str("<int>-1</int>");
            }
            let _ = write!(
                xml,
                "</durability><Index>{}</Index><ID>{id}</ID><Position>{}</Position>\
                 </ConstructionSnapshotData>",
                construction.index,
                board(construction.position, sign)
            );
        }
        xml.push_str("</constructionSnapshotDatas>");
    }
    let next = side
        .constructions
        .iter()
        .map(|construction| construction.index + 1)
        .max()
        .unwrap_or(0);
    let _ = write!(xml, "<constructionIndex>{next}</constructionIndex>");
    Ok(())
}

/// A side-local position in the board's frame, as the record spells one.
fn board(position: Position, sign: i32) -> String {
    format!("<x>{}</x><y>{}</y>", sign * position.x, sign * position.y)
}

const DEFENSIVE_WALL: i32 = 1;

fn native_id(spec: Option<crate::catalog::FormationSpec>) -> Option<i32> {
    use crate::catalog::NativeFormation;
    match spec?.native {
        NativeFormation::Unit(id)
        | NativeFormation::Construction(id)
        | NativeFormation::Contraption(id) => Some(id),
    }
}

/// The panel, each slot with the objects its earlier releases left standing:
/// a retained Shield Airdrop under the Shield Airdrop slot, a retained area
/// under the slot of the skill that leaves it.
fn write_panel(xml: &mut String, side: &SideState, sign: i32, round: i32) -> Result<(), String> {
    let mut retained: Vec<(i32, String)> = side
        .airdrop_shields
        .iter()
        .map(|center| (SHIELD_AIRDROP_SKILL, shield_range_data(*center, sign)))
        .collect();
    for terrain in &side.terrains {
        let skill = crate::catalog::terrain_skill_from_type(terrain.terrain_type)
            .ok_or_else(|| format!("terrain {:?} is left by no skill", terrain.terrain_type))?;
        retained.push((skill, terrain_range_data(terrain, sign)));
    }
    for (skill, _) in &retained {
        if !side.battle_skills.iter().any(|slot| slot.id == *skill) {
            return Err(format!(
                "an object skill {skill} left stands, and the panel holds no slot of it"
            ));
        }
    }
    if side.battle_skills.is_empty() {
        xml.push_str("<commanderSkills />");
        return Ok(());
    }
    xml.push_str("<commanderSkills>");
    let mut placed = vec![false; retained.len()];
    for slot in &side.battle_skills {
        let _ = write!(
            xml,
            "<CommanderSkillData><index>{}</index><id>{}</id><isActive>true</isActive>\
             <coolingRound>{}</coolingRound><getRound>{round}</getRound>",
            slot.index, slot.id, slot.cooldown
        );
        let mut mine: Vec<&String> = Vec::new();
        for (at, (skill, item)) in retained.iter().enumerate() {
            if *skill == slot.id && !placed[at] {
                placed[at] = true;
                mine.push(item);
            }
        }
        if mine.is_empty() {
            xml.push_str("<rangeItems />");
        } else {
            xml.push_str("<rangeItems>");
            for item in mine {
                xml.push_str(item);
            }
            xml.push_str("</rangeItems>");
        }
        xml.push_str("</CommanderSkillData>");
    }
    xml.push_str("</commanderSkills>");
    Ok(())
}

/// A round's decisions as the replay records them, each side ending its
/// deployment. A purchase is recorded as the purchase and a move that puts the
/// formation it creates where the battle does, which conversion folds back.
fn write_actions(
    xml: &mut String,
    economy: &Economy,
    opened: &SideState,
    turn: &Turn,
    actions: &[Action],
    sign: i32,
) -> Result<(), String> {
    let mut records: Vec<(&str, String)> = Vec::new();
    let offers = turn.state.reinforce_offers.as_ref();
    let declined = offers.map(|offers| offers.refund);
    // Each decision is stepped as conversion steps it, which is what names
    // the formation a purchase creates and the type a move carries.
    let mut position = opened.clone();
    for action in actions {
        let moved = match action {
            Action::MoveUnit { index, .. } => position
                .units
                .iter()
                .find(|entry| entry.unit.index == *index)
                .and_then(|entry| crate::catalog::unit_id_from_type(&entry.unit.type_name))
                .unwrap_or(0),
            _ => 0,
        };
        let context = Context {
            created: position.next_index.unit,
            moved,
            offers,
            sign,
        };
        records.extend(
            action_records(action, &context)
                .map_err(|error| format!("round {}: {error}", turn.round))?,
        );
        position = crate::transition::step_placing(
            economy,
            &position,
            action,
            declined,
            &mut crate::landing::placement(sign < 0),
        )
        .map_err(|reason| {
            format!(
                "round {} decision {action:?} does not step: {reason:?}",
                turn.round
            )
        })?;
    }
    if !matches!(actions.last(), Some(Action::Concede)) {
        records.push(("PAD_FinishDeploy", String::new()));
    }
    xml.push_str("<actionRecords>");
    for (time, (kind, fields)) in records.iter().enumerate() {
        let _ = write!(
            xml,
            "<MatchActionData xsi:type=\"{kind}\"><Time>{}</Time><LocalTime>0</LocalTime>{fields}\
             </MatchActionData>",
            time + 1
        );
    }
    xml.push_str("</actionRecords>");
    Ok(())
}

/// What recording one decision needs from the position it is taken from.
struct Context<'a> {
    /// The index a purchase creates.
    created: i32,
    /// The unit type a move carries.
    moved: i32,
    offers: Option<&'a crate::battle::Offers>,
    sign: i32,
}

/// One decision as the actions the replay records for it.
fn action_records(
    action: &Action,
    context: &Context,
) -> Result<Vec<(&'static str, String)>, String> {
    let sign = context.sign;
    Ok(match action {
        Action::ChooseReinforceItem { index, id } => {
            let declined = context
                .offers
                .is_some_and(|offers| *index == offers.decline_index());
            let (index, id) = if declined {
                (-1, 0)
            } else {
                (*index, id.unwrap_or(0))
            };
            vec![(
                "PAD_ChooseReinforceItem",
                format!("<ID>{id}</ID><Index>{index}</Index><paras />"),
            )]
        }
        Action::ChooseAdvanceTeam { .. } => {
            return Err("a deployment round takes an opening, and only round 0 does".into());
        }
        // The game places a purchase itself, where the deployment area is
        // free, so a move puts it where the battle does; conversion folds the
        // move back into the purchase.
        Action::BuyUnit {
            unit,
            position,
            rotated,
        } => vec![
            (
                "PAD_BuyUnit",
                format!(
                    "<UID>{unit}</UID><position>{}</position><UIDX>-1</UIDX>",
                    board(*position, sign)
                ),
            ),
            (
                "PAD_MoveUnit",
                move_record(*unit, context.created, *position, *rotated, sign),
            ),
        ],
        Action::UpgradeUnit { index } => vec![(
            "PAD_UpgradeUnit",
            format!("<UIDX>{index}</UIDX><UID>0</UID>"),
        )],
        Action::UnlockUnit { unit } => vec![("PAD_UnlockUnit", format!("<UID>{unit}</UID>"))],
        Action::UpgradeTechnology { unit, tech } => vec![(
            "PAD_UpgradeTechnology",
            format!("<UID>{unit}</UID><TechID>{tech}</TechID>"),
        )],
        Action::ActiveBlueprint { id } => vec![("PAD_ActiveBlueprint", format!("<ID>{id}</ID>"))],
        Action::ActiveEnergyTowerSkill { skill } => vec![(
            "PAD_ActiveEnergyTowerSkill",
            format!("<SkillID>{skill}</SkillID>"),
        )],
        Action::StrengthenTower { tower } => {
            vec![("PAD_StrengthenTower", format!("<Index>{tower}</Index>"))]
        }
        Action::UseEquipment { equipment, index } => vec![(
            "PAD_UseEquipment",
            format!("<EquipmentID>{equipment}</EquipmentID><UnitIndex>{index}</UnitIndex>"),
        )],
        Action::MoveUnit {
            index,
            position,
            rotated,
        } => vec![(
            "PAD_MoveUnit",
            move_record(context.moved, *index, *position, *rotated, sign),
        )],
        Action::ReleaseCommanderSkill { index, target, .. } => {
            vec![(
                "PAD_ReleaseCommanderSkill",
                release_record(*index, target, sign),
            )]
        }
        Action::ReleaseContraption {
            contraption,
            position,
            extra_position,
        } => vec![(
            "PAD_ReleaseContraption",
            format!(
                "<ContraptionID>{contraption}</ContraptionID><Position>{}</Position>\
                 <ExtraPosition>{}</ExtraPosition>",
                board(*position, sign),
                board(extra_position.unwrap_or(Position { x: 0, y: 0 }), sign)
            ),
        )],
        Action::Concede => vec![("PAD_GiveUp", String::new())],
    })
}

/// A release from panel slot `index`: an area at its positions, or the unit
/// or construction it names.
fn release_record(index: i32, target: &SkillTarget, sign: i32) -> String {
    let (positions, unit, construction) = match target {
        SkillTarget::Area(points) => (
            points.iter().fold(String::new(), |mut all, point| {
                let _ = write!(all, "<MapVector>{}</MapVector>", board(*point, sign));
                all
            }),
            -1,
            -1,
        ),
        SkillTarget::Unit(unit) => (String::new(), *unit, -1),
        SkillTarget::Construction(construction) => (String::new(), -1, *construction),
    };
    format!(
        "<ID>0</ID><SkillIndex>{index}</SkillIndex><Positions>{positions}</Positions>\
         <UnitIndex>{unit}</UnitIndex><ConstructionIndex>{construction}</ConstructionIndex>"
    )
}

fn move_record(unit: i32, index: i32, position: Position, rotated: bool, sign: i32) -> String {
    format!(
        "<moveUnitDatas><MoveUnitData><unitID>{unit}</unitID><unitIndex>{index}</unitIndex>\
         <position>{}</position><isRotate>{rotated}</isRotate>\
         <positionRecord><x>0</x><y>0</y></positionRecord><rotateRecord>false</rotateRecord>\
         <superDeployRecord>false</superDeployRecord></MoveUnitData></moveUnitDatas>",
        board(position, sign)
    )
}
