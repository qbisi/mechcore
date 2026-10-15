//! Inverse of the synthetic layout writer, not a general test-command player.
//! Re-encoding must reproduce the embedded XML exactly, so ignored native
//! fields or unrecognized restoration commands cannot silently lose state.

use crate::record::{ActionRecord, PlayerRoundRecord, PositionRecord};
use crate::{
    BattleSkillEntry, BattleSkillRelease, Layout, Position, Side, UnitPlacement, UnitSource,
};

/// Restores the layout encoded by this crate's synthetic replay writer.
///
/// # Errors
///
/// Refuses another build, an unavailable round, unknown restoration commands,
/// or an XML document this writer cannot reproduce from the restored layout.
pub fn read(grbr: &[u8], round: i32) -> Result<Layout, String> {
    let selected = crate::record::select_round(grbr, Some(round))?;
    if !selected.synthetic {
        return Err("this is not a synthetic layout replay".into());
    }
    let record = crate::record::read(grbr)?;
    let version = super::build_number(crate::game_build())?.to_string();
    if record.version != version {
        return Err(format!(
            "layout replay version {} differs from {version}",
            record.version
        ));
    }
    let retained = crate::retained_from_grbr_round(
        grbr,
        u32::try_from(round).map_err(|_| "layout replay round must be positive")?,
    )?;
    let side = |at: usize, red: bool, standing: Vec<crate::GrbrStanding>| {
        let turn = record.players.entries[at]
            .rounds
            .entries
            .iter()
            .find(|turn| turn.round == round)
            .ok_or("layout replay has no selected snapshot")?;
        restored_side(turn, red, round, standing)
    };
    let layout = Layout {
        kind: crate::DocumentKind::Layout,
        game_build: crate::game_build().to_owned(),
        map_id: Some(record.info.map_id),
        seed: Some(record.info.system_seed),
        round,
        blue: side(0, false, retained.blue)?,
        red: side(1, true, retained.red)?,
    };
    let plan = crate::compile_layout(layout.clone())?;
    let encoded = super::layout_replay(&plan, crate::game_build())?;
    if crate::record::embedded_xml(&encoded)? != crate::record::embedded_xml(grbr)? {
        return Err(
            "synthetic replay is not a canonical layout replay: its restored layout \
            does not reproduce the native snapshot and commands"
                .into(),
        );
    }
    Ok(layout)
}

fn restored_side(
    turn: &PlayerRoundRecord,
    red: bool,
    round: i32,
    standing: Vec<crate::GrbrStanding>,
) -> Result<Side, String> {
    let mut side = crate::convert::layout_snapshot(&turn.data, red)?;
    side.battle_skills = standing
        .into_iter()
        .map(|entry| BattleSkillEntry::Standing(entry.standing))
        .collect();
    let mut next = turn.data.unit_index;
    let economy = crate::economy::Economy::embedded()?;
    for officer in &side.officers {
        if let Some(squad) = economy
            .officer(*officer)
            .filter(|row| row.active_round.contains(&round))
            .and_then(|row| row.opening_unit)
        {
            side.units
                .push(added(squad.unit, next, squad.level, UnitSource::Delivered)?);
            next = next
                .checked_add(1)
                .ok_or("layout replay unit index overflow")?;
        }
    }
    for action in &turn.actions.entries {
        restore_action(&mut side, turn, action, red, &mut next)?;
    }
    Ok(side)
}

fn added(id: i32, index: i32, level: i32, source: UnitSource) -> Result<UnitPlacement, String> {
    let (name, _) = crate::unit_type_from_id(id)
        .ok_or_else(|| format!("layout replay unit {id} has no layout type"))?;
    Ok(UnitPlacement {
        type_name: name.to_owned(),
        index,
        position: Position { x: 0, y: 0 },
        level: Some(level).filter(|level| *level != 1),
        exp: None,
        rotated: None,
        equipment: Vec::new(),
        travelling: None,
        source,
    })
}

fn field(value: Option<i32>, name: &str) -> Result<i32, String> {
    value.ok_or_else(|| format!("layout restoration command has no {name}"))
}

fn unit(side: &mut Side, index: Option<i32>) -> Result<&mut UnitPlacement, String> {
    let index = field(index, "unit index")?;
    side.units
        .iter_mut()
        .find(|unit| unit.index == index)
        .ok_or_else(|| format!("layout restoration names absent unit {index}"))
}

fn position(record: &PositionRecord, red: bool) -> Position {
    let sign = if red { -1 } else { 1 };
    Position {
        x: sign * record.x,
        y: sign * record.y,
    }
}

fn restore_action(
    side: &mut Side,
    turn: &PlayerRoundRecord,
    action: &ActionRecord,
    red: bool,
    next: &mut i32,
) -> Result<(), String> {
    match action.kind.as_str() {
        "PAD_TestCommand" => restore_test(side, action, red, next)?,
        "PAD_MoveUnit" => restore_moves(side, action, red, false)?,
        "PAD_UpgradeUnit" => {
            let unit = unit(side, action.unit_index_allocated)?;
            unit.level = Some(unit.level.unwrap_or(1) + 1);
            unit.exp = None;
        }
        "PAD_UseEquipment" => {
            unit(side, action.unit_index)?
                .equipment
                .push(field(action.equipment_id, "equipment ID")?);
        }
        "PAD_ActiveEnergyTowerSkill" => side
            .energy_tower_skills
            .push(field(action.skill_id, "skill ID")?),
        "PAD_ReleaseCommanderSkill" => restore_skill(side, turn, action, red)?,
        "PAD_FinishDeploy" => (),
        name => return Err(format!("layout replay restoration does not admit {name}")),
    }
    Ok(())
}

fn restore_test(
    side: &mut Side,
    action: &ActionRecord,
    red: bool,
    next: &mut i32,
) -> Result<(), String> {
    let command = action
        .command
        .as_ref()
        .ok_or("PAD_TestCommand has no Command")?;
    match command.kind.as_str() {
        "MAD_AddUnit" => {
            let level = command
                .level
                .as_deref()
                .and_then(|level| level.strip_prefix("Level"))
                .and_then(|level| level.parse().ok())
                .ok_or("MAD_AddUnit has no native level")?;
            let index = field(command.unit_index_allocated, "allocated index")?;
            let index = if index == -1 {
                let allocated = *next;
                *next = next
                    .checked_add(1)
                    .ok_or("layout replay unit index overflow")?;
                allocated
            } else {
                index
            };
            side.units.push(added(
                field(command.unit_id, "unit ID")?,
                index,
                level,
                UnitSource::Joined,
            )?);
        }
        "MAD_MoveUnit" => restore_moves(side, command, red, true)?,
        name => {
            return Err(format!(
                "layout replay restoration does not admit test command {name}"
            ));
        }
    }
    Ok(())
}

fn restore_moves(
    side: &mut Side,
    action: &ActionRecord,
    red: bool,
    forced: bool,
) -> Result<(), String> {
    let moves = action
        .moves
        .as_ref()
        .ok_or("layout restoration move has no moveUnitDatas")?;
    for movement in &moves.entries {
        let unit = unit(side, Some(movement.unit_index))?;
        unit.position = position(&movement.position, red);
        unit.rotated = Some(movement.rotated).filter(|rotated| *rotated);
        let travelling = if forced {
            movement.travelling
        } else {
            unit.source != UnitSource::Legacy && crate::Region::of(unit.position).is_flank()
        };
        unit.travelling = Some(travelling).filter(|travelling| *travelling);
    }
    Ok(())
}

fn restore_skill(
    side: &mut Side,
    turn: &PlayerRoundRecord,
    action: &ActionRecord,
    red: bool,
) -> Result<(), String> {
    let slot = field(action.skill_index, "commander skill index")?;
    let id = turn
        .data
        .commander_skills
        .entries
        .iter()
        .find(|skill| skill.index == slot)
        .ok_or_else(|| format!("layout restoration names absent commander skill {slot}"))?
        .id;
    if id == super::FIELD_RECOVERY_SKILL {
        let index = field(action.unit_index, "recovered unit index")?;
        let at = side
            .units
            .iter()
            .position(|unit| unit.index == index)
            .ok_or_else(|| format!("layout restoration recovers absent unit {index}"))?;
        side.units.remove(at);
        side.recovered.push(index);
    } else if crate::transition::TRAINING_SKILLS.contains(&id) {
        let unit = unit(side, action.unit_index)?;
        let level = unit.level.unwrap_or(1);
        let full = crate::experience::full(&unit.type_name, level)
            .ok_or("no experience bar for training")?;
        unit.exp = crate::Experience::of(full, &unit.type_name, level)?;
    } else {
        let name = crate::battle_skill_type_from_id(id)
            .ok_or_else(|| format!("layout restoration commander skill {id} has no layout type"))?;
        let positions = action
            .positions
            .as_ref()
            .ok_or("battle skill release has no Positions")?;
        side.battle_skills
            .push(BattleSkillEntry::Release(BattleSkillRelease {
                type_name: name.to_owned(),
                positions: positions
                    .entries
                    .iter()
                    .map(|entry| position(entry, red))
                    .collect(),
            }));
    }
    Ok(())
}
