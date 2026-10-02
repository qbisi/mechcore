//! The `control_progress` instrument channel: the units a control beam is
//! turning, read from `TeamTranslationSystem.translatingDatas` at each
//! snapshot.
//!
//! The field is a `SyncDictionary<FightMech, TranslationData>`, which keeps
//! its keys and values in two lists side by side. `ControllEffect.Start` adds
//! a beam's skill to its target's entry, `ControllEffect.Perform` adds each
//! hit's power to its progress (`TeamTranslationSystem.Translate`),
//! `ControllEffect.Stop` takes the skill away, and the entry goes with the
//! last one. `TeamTranslationSystem.Update` turns a unit whose progress has
//! reached its life.

use crate::capture::{CaptureState, list_count, list_item, object_ref_from_pointer};
use crate::il2cpp::{Api, FieldInfo, Object};
use mechcore_mcfr::ControlProgress;

/// A `TranslationData`, as a `List<TranslationData>` holds it.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Data {
    progress: i32,
    padding: i32,
    sources: usize,
}

/// The classes and fields the reader uses, resolved once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ControlMetadata {
    pub(crate) system_class: usize,
    translating: usize,
    skill_owner: usize,
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn initialize(api: Api) -> Result<ControlMetadata, String> {
    let error = |error: crate::il2cpp::Error| error.to_string();
    let class = |name: &str| {
        api.class("GRFight.dll", "GameRiver.Fight", name)
            .map_err(error)
    };
    let system = class("TeamTranslationSystem")?;
    let skill = class("FightSkillBase")?;
    Ok(ControlMetadata {
        system_class: system as usize,
        translating: api.field(system, "translatingDatas").map_err(error)? as usize,
        skill_owner: api.field(skill, "skillOwner").map_err(error)? as usize,
    })
}

fn field<T: Copy>(api: Api, object: *mut Object, field: usize, name: &str) -> Result<T, String> {
    api.field_value(object, field as *mut FieldInfo)
        .map_err(|error| format!("{name}: {error}"))
}

/// A field of the object's own class, which a generic instance's fields are.
fn own_field<T: Copy>(api: Api, object: *mut Object, name: &str) -> Result<T, String> {
    let class = api
        .object_class(object)
        .ok_or_else(|| format!("{name}: the object has no class"))?;
    let info = api.field(class, name).map_err(|error| error.to_string())?;
    api.field_value(object, info)
        .map_err(|error| format!("{name}: {error}"))
}

/// Every unit being turned, ordered by unit.
pub(crate) fn read(
    api: Api,
    system: *mut Object,
    metadata: &ControlMetadata,
    capture: &CaptureState,
) -> Result<Vec<ControlProgress>, String> {
    let translating: *mut Object = field(
        api,
        system,
        metadata.translating,
        "TeamTranslationSystem.translatingDatas",
    )?;
    if translating.is_null() {
        return Ok(Vec::new());
    }
    let keys: *mut Object = own_field(api, translating, "keys")?;
    let values: *mut Object = own_field(api, translating, "values")?;
    let count = list_count(api, keys, 100_000)?;
    let items: *mut Object = own_field(api, values, "_items")?;
    let size: i32 = own_field(api, values, "_size")?;
    if size != count {
        return Err("translatingDatas holds a different number of keys and values".into());
    }
    let count = usize::try_from(count).map_err(|_| "a negative list count")?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let data = api
        .value_array_range::<Data>(items, 0, count)
        .map_err(|error| error.to_string())?;
    let mut rows = Vec::with_capacity(count);
    for (index, data) in data.into_iter().enumerate() {
        let index = i32::try_from(index).map_err(|_| "a list index beyond i32")?;
        let target = list_item(api, keys, index)?;
        let Some(unit) = object_ref_from_pointer(target as usize, capture) else {
            return Err("a unit being turned has no recorded identity".into());
        };
        let mut sources = Vec::new();
        let list = data.sources as *mut Object;
        if !list.is_null() {
            for slot in 0..list_count(api, list, 1_000)? {
                let skill = list_item(api, list, slot)?;
                let owner: *mut Object = field(
                    api,
                    skill,
                    metadata.skill_owner,
                    "FightSkillBase.skillOwner",
                )?;
                sources.push(
                    object_ref_from_pointer(owner as usize, capture)
                        .ok_or("a control beam's owner has no recorded identity")?,
                );
            }
        }
        rows.push(ControlProgress {
            unit,
            progress: data.progress,
            sources,
        });
    }
    rows.sort_by_key(|row| row.unit);
    Ok(rows)
}
