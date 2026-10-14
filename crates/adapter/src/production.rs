//! The clocks of unit-owned production lines, read from the side's native
//! `TeamSupportUnitManager.creators` at the snapshot boundary.

use crate::capture::{find_module, list_count, list_item};
use crate::il2cpp::{Api, Class, FieldInfo, Object};
use mechcore_mcfr::ProductionLineState;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Default)]
pub(crate) struct ProductionMetadata {
    manager: usize,
    creators: usize,
    data: usize,
    counter: usize,
    mech: usize,
}

pub(crate) fn initialize(api: Api) -> Result<ProductionMetadata, String> {
    let class = |name| {
        api.class("GRFight.dll", "GameRiver.Fight", name)
            .map_err(|e| e.to_string())
    };
    let field = |class, name| {
        api.field(class, name)
            .map(|f| f as usize)
            .map_err(|e| e.to_string())
    };
    let manager = class("TeamSupportUnitManager")?;
    let creator = class("SupportUnitCreator")?;
    Ok(ProductionMetadata {
        manager: manager as usize,
        creators: field(manager, "creators")?,
        data: field(creator, "unitDataSource")?,
        counter: field(creator, "intervalTimeCounter")?,
        mech: class("FightMech")? as usize,
    })
}

pub(crate) fn read(
    api: Api,
    controller: *mut Object,
    metadata: &ProductionMetadata,
) -> Result<BTreeMap<usize, Vec<ProductionLineState>>, String> {
    let modules = api
        .invoke(controller, "GetModules", &mut [])
        .map_err(|e| e.to_string())?;
    let manager = find_module(api, modules, metadata.manager, "TeamSupportUnitManager")?;
    let creators: *mut Object = api
        .field_value(manager, metadata.creators as *mut FieldInfo)
        .map_err(|e| e.to_string())?;
    let mut rows: BTreeMap<usize, Vec<ProductionLineState>> = BTreeMap::new();
    for index in 0..list_count(api, creators, 100_000)? {
        let creator = list_item(api, creators, index)?;
        let data: *mut Object = api
            .field_value(creator, metadata.data as *mut FieldInfo)
            .map_err(|e| e.to_string())?;
        let owner = api
            .invoke(data, "GetParent", &mut [])
            .map_err(|e| e.to_string())?;
        if !api
            .object_class(owner)
            .is_some_and(|class| api.class_is_or_inherits(class, metadata.mech as *mut Class))
        {
            return Err("a permanent production line has no FightMech owner".into());
        }
        rows.entry(owner as usize)
            .or_default()
            .push(ProductionLineState {
                source_id: api
                    .invoke_value(data, "GetID", &mut [])
                    .map_err(|e| e.to_string())?,
                interval_time_counter: api
                    .field_value(creator, metadata.counter as *mut FieldInfo)
                    .map_err(|e| e.to_string())?,
            });
    }
    for lines in rows.values_mut() {
        lines.sort_by_key(|line| line.source_id);
        if lines
            .windows(2)
            .any(|pair| pair[0].source_id == pair[1].source_id)
        {
            return Err("two permanent production lines on one owner share a source id".into());
        }
    }
    Ok(rows)
}
