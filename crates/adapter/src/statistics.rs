//! The build's own damage and kill counters, read from
//! `BattleStatisticManager` at each snapshot.
//!
//! `FightController.OnActorHitted` credits every hit, inside the logic tick,
//! to the attacker's recorder and charges it to the target's: a formation
//! (`MechTeam`), a construction group (`FightConstructionCombination`), a
//! construction outside one, or a mind-controlled unit, which counts alone. The
//! current round keeps one dictionary per team, keyed by recorder, and a
//! temporary one for recorders no team holds.

use crate::capture::{CaptureState, list_count, list_item};
use crate::il2cpp::{Api, Class, FieldInfo, Object};
use mechcore_mcfr::{DamageStatistics, FormationState, RecorderKind};

/// A `Dictionary<IDamageRecorder, UnitDamageStatisticData>` entry.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Entry {
    hash_code: i32,
    next: i32,
    key: usize,
    value: usize,
}

/// The fields and classes the reader uses, resolved once.
#[derive(Clone, Copy)]
pub(crate) struct StatisticsMetadata {
    manager: usize,
    round: usize,
    by_team: usize,
    temporary: usize,
    damage_max: usize,
    damage_real: usize,
    kill_count: usize,
    damage_taken: usize,
    combination_constructions: usize,
    mech_team: usize,
    combination: usize,
    construction: usize,
    mech: usize,
    experience: usize,
    max_experience: usize,
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn initialize(api: Api) -> Result<StatisticsMetadata, String> {
    let error = |error: crate::il2cpp::Error| error.to_string();
    let class =
        |namespace: &str, name: &str| api.class("GRFight.dll", namespace, name).map_err(error);
    let field = |class, name: &str| {
        api.field(class, name)
            .map(|field| field as usize)
            .map_err(error)
    };
    let fight = class("GameRiver.Fight", "FightController")?;
    let manager = class("GameRiver", "BattleStatisticManager")?;
    let round = class("GameRiver", "RoundStatisticData")?;
    let data = class("GameRiver", "UnitDamageStatisticData")?;
    let combination = class("GameRiver.Fight", "FightConstructionCombination")?;
    let mech_team = class("GameRiver.Fight", "MechTeam")?;
    Ok(StatisticsMetadata {
        manager: field(fight, "battleStatisticManager")?,
        round: field(manager, "roundStatisticData")?,
        by_team: field(round, "unitDamageStatisticDatasByTeam")?,
        temporary: field(round, "tempUnitDamageStatisticDatas")?,
        damage_max: field(data, "<DamageMax>k__BackingField")?,
        damage_real: field(data, "<DamageReal>k__BackingField")?,
        kill_count: field(data, "<KillCount>k__BackingField")?,
        damage_taken: field(data, "<DamageTaken>k__BackingField")?,
        combination_constructions: field(combination, "constructions")?,
        mech_team: mech_team as usize,
        combination: combination as usize,
        construction: class("GameRiver.Fight", "FightConstruction")? as usize,
        mech: class("GameRiver.Fight", "FightMech")? as usize,
        experience: field(mech_team, "expFloat")?,
        max_experience: field(mech_team, "maxExpFloat")?,
    })
}

fn value<T: Copy>(api: Api, object: *mut Object, field: usize, name: &str) -> Result<T, String> {
    api.field_value(object, field as *mut FieldInfo)
        .map_err(|error| format!("{name}: {error}"))
}

/// The live entries of a dictionary, in the order the build holds them.
fn entries(api: Api, dictionary: *mut Object) -> Result<Vec<Entry>, String> {
    let class = api
        .object_class(dictionary)
        .ok_or("a dictionary has no class")?;
    let field = |name: &str| api.field(class, name).map_err(|error| error.to_string());
    let array: *mut Object = api
        .field_value(dictionary, field("_entries")?)
        .map_err(|error| error.to_string())?;
    let count: i32 = api
        .field_value(dictionary, field("_count")?)
        .map_err(|error| error.to_string())?;
    if array.is_null() || count <= 0 {
        return Ok(Vec::new());
    }
    let count = usize::try_from(count).map_err(|_| "a dictionary's count is negative")?;
    Ok(api
        .value_array_range::<Entry>(array, 0, count)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|entry| entry.hash_code >= 0)
        .collect())
}

/// The current round's counters, one row per recorder, in stored order.
pub(crate) fn read(
    api: Api,
    fight: *mut Object,
    metadata: &StatisticsMetadata,
    capture: &CaptureState,
) -> Result<Vec<DamageStatistics>, String> {
    let manager: *mut Object = value(api, fight, metadata.manager, "battleStatisticManager")?;
    if manager.is_null() {
        return Err("the fight has no BattleStatisticManager".into());
    }
    let round: *mut Object = value(api, manager, metadata.round, "roundStatisticData")?;
    let by_team: *mut Object = value(
        api,
        round,
        metadata.by_team,
        "unitDamageStatisticDatasByTeam",
    )?;
    let mut rows = Vec::new();
    for team in 0..list_count(api, by_team, 64)? {
        let dictionary = list_item(api, by_team, team)?;
        let team_id = u32::try_from(team).map_err(|_| "a negative team index")?;
        for entry in entries(api, dictionary)? {
            rows.push(row(api, metadata, capture, Some(team_id), entry)?);
        }
    }
    let temporary: *mut Object = value(
        api,
        round,
        metadata.temporary,
        "tempUnitDamageStatisticDatas",
    )?;
    if !temporary.is_null() {
        for entry in entries(api, temporary)? {
            rows.push(row(api, metadata, capture, None, entry)?);
        }
    }
    rows.sort_by_key(DamageStatistics::key);
    if rows.windows(2).any(|pair| pair[0].key() == pair[1].key()) {
        return Err("two statistics entries name the same recorder".into());
    }
    Ok(rows)
}

fn row(
    api: Api,
    metadata: &StatisticsMetadata,
    capture: &CaptureState,
    team_id: Option<u32>,
    entry: Entry,
) -> Result<DamageStatistics, String> {
    let key = entry.key as *mut Object;
    let data = entry.value as *mut Object;
    if key.is_null() || data.is_null() {
        return Err("a statistics entry has a null key or value".into());
    }
    let is = |class: usize| {
        api.object_class(key)
            .is_some_and(|actual| api.class_is_or_inherits(actual, class as *mut Class))
    };
    let (recorder, recorder_id) = if is(metadata.mech_team) {
        let id = capture
            .formation_ids
            .get(&(key as usize))
            .copied()
            .ok_or("a statistics formation has no formation_id")?;
        (RecorderKind::Formation, id)
    } else if is(metadata.combination) {
        let constructions: *mut Object = value(
            api,
            key,
            metadata.combination_constructions,
            "FightConstructionCombination.constructions",
        )?;
        let mut lowest = None::<u64>;
        for index in 0..list_count(api, constructions, 100_000)? {
            let member = list_item(api, constructions, index)? as usize;
            if let Some(id) = capture.building_ids.get(&member) {
                lowest = Some(lowest.map_or(*id, |current| current.min(*id)));
            }
        }
        (
            RecorderKind::Construction,
            lowest.ok_or("a statistics construction group has no recorded construction")?,
        )
    } else if is(metadata.construction) {
        let id = capture
            .building_ids
            .get(&(key as usize))
            .copied()
            .ok_or("a statistics construction has no building_id")?;
        (RecorderKind::Construction, id)
    } else if is(metadata.mech) {
        let id = capture
            .unit_ids
            .get(&(key as usize))
            .copied()
            .ok_or("a statistics unit has no unit_id")?;
        (RecorderKind::Unit, id)
    } else {
        return Err(format!(
            "a statistics entry is keyed by a {}",
            api.object_class_name(key)
        ));
    };
    let team_id = match team_id {
        Some(team_id) => team_id,
        // A recorder no team's dictionary holds, a mind-controlled unit, is
        // counted under the side it serves now.
        None => capture
            .object_teams
            .get(&mechcore_mcfr::ObjectRef::new(
                mechcore_mcfr::ObjectKind::Unit,
                recorder_id,
            ))
            .copied()
            .ok_or("a unit counted alone has no current team")?,
    };
    Ok(DamageStatistics {
        team_id,
        recorder,
        recorder_id,
        damage: value(api, data, metadata.damage_max, "DamageMax")?,
        damage_real: value(api, data, metadata.damage_real, "DamageReal")?,
        kills: value(api, data, metadata.kill_count, "KillCount")?,
        damage_taken: value(api, data, metadata.damage_taken, "DamageTaken")?,
    })
}

/// Every formation's experience, `MechTeam.expFloat`, and its full bar,
/// `maxExpFloat`, both `FPoint` raw, in `formation_id` order.
pub(crate) fn read_formations(
    api: Api,
    metadata: &StatisticsMetadata,
    capture: &CaptureState,
) -> Result<Vec<FormationState>, String> {
    let mut rows = Vec::new();
    for (pointer, formation_id) in &capture.formation_ids {
        let team = *pointer as *mut Object;
        // A unit with no formation is keyed by its own pointer.
        let is_team = api
            .object_class(team)
            .is_some_and(|class| api.class_is_or_inherits(class, metadata.mech_team as *mut Class));
        if !is_team {
            continue;
        }
        let team_id = capture
            .formation_teams
            .get(formation_id)
            .copied()
            .ok_or("a formation has no team")?;
        rows.push(FormationState {
            formation_id: *formation_id,
            team_id,
            experience: value::<i64>(api, team, metadata.experience, "MechTeam.expFloat")?,
            max_experience: value::<i64>(
                api,
                team,
                metadata.max_experience,
                "MechTeam.maxExpFloat",
            )?,
        });
    }
    rows.sort_by_key(|row| row.formation_id);
    Ok(rows)
}
