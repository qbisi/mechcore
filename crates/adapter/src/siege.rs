//! The native clock of each active Field Entrenchment owner.

use crate::capture::{find_module, list_count, list_item};
use crate::il2cpp::{Api, Class, FieldInfo, Object};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Default)]
pub(crate) struct SiegeMetadata {
    system: usize,
    active: usize,
    durations: usize,
    mech: usize,
}

pub(crate) fn initialize(api: Api) -> Result<SiegeMetadata, String> {
    let system = api
        .class("GRFight.dll", "GameRiver.Fight", "SiegeModeEffectSystem")
        .map_err(|e| e.to_string())?;
    Ok(SiegeMetadata {
        system: system as usize,
        active: api
            .field(system, "activeMeches")
            .map_err(|e| e.to_string())? as usize,
        durations: api
            .field(system, "siegeModeOwnerDurations")
            .map_err(|e| e.to_string())? as usize,
        mech: api
            .class("GRFight.dll", "GameRiver.Fight", "FightMech")
            .map_err(|e| e.to_string())? as usize,
    })
}

pub(crate) fn read(
    api: Api,
    modules: *mut Object,
    metadata: &SiegeMetadata,
) -> Result<BTreeMap<usize, i64>, String> {
    let system = find_module(api, modules, metadata.system, "SiegeModeEffectSystem")?;
    let active: *mut Object = api
        .field_value(system, metadata.active as *mut FieldInfo)
        .map_err(|e| e.to_string())?;
    let durations: *mut Object = api
        .field_value(system, metadata.durations as *mut FieldInfo)
        .map_err(|e| e.to_string())?;
    let durations = api
        .dictionary_entries::<usize, i64>(durations)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let mut clocks = BTreeMap::new();
    for index in 0..list_count(api, active, 100_000)? {
        let owner = list_item(api, active, index)?;
        if !api
            .object_class(owner)
            .is_some_and(|class| api.class_is_or_inherits(class, metadata.mech as *mut Class))
        {
            return Err("an active Field Entrenchment owner is not a FightMech".into());
        }
        let pointer = owner as usize;
        let elapsed = *durations
            .get(&pointer)
            .ok_or("an active Field Entrenchment owner has no duration")?;
        if elapsed < 0 || clocks.insert(pointer, elapsed).is_some() {
            return Err("Field Entrenchment has a negative clock or duplicate active owner".into());
        }
    }
    Ok(clocks)
}
