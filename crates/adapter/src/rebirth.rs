//! The units waiting to be reborn, read from
//! `DeadRebirthController.rebirthTasks` at each snapshot into the `rebirths`
//! collection.
//!
//! `DeadEffectSystem.deadEffectControllers` holds one controller per dead
//! effect, the rebirth one among them. Its `rebirthTasks` is a
//! `SyncDictionary<FightMech, RebirthTask>`, which keeps its keys and values
//! in two lists side by side. `DeadRebirthController.PerformDeadEffect` adds a
//! dying unit's task, and `DoTaskEndProcess` takes it away as the unit is
//! reborn or the task fails. `RebirthTask.GetPosAndRotation` answers where
//! the unit will stand again: where it fell, or where the pilot that follows
//! an ally flies.

use crate::capture::{CaptureState, list_count, list_item, object_ref_from_pointer};
use crate::il2cpp::{Api, Class, FieldInfo, MethodInfo, Object};
use mechcore_mcfr::{ObjectKind, QVec3, RebirthState};

/// A `FVector3`, three `FPoint`s.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vector {
    x: i64,
    y: i64,
    z: i64,
}

type GetPosAndRotationFn =
    unsafe extern "C" fn(*mut Object, *mut Vector, *mut Vector, *const MethodInfo);

/// The classes, fields and method the reader uses, resolved once.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RebirthMetadata {
    pub(crate) system_class: usize,
    controllers: usize,
    controller_class: usize,
    tasks: usize,
    task_unit: usize,
    position: usize,
    position_method: usize,
    /// `FightMech.rebirthCount`.
    pub(crate) unit_count: usize,
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn initialize(api: Api) -> Result<RebirthMetadata, String> {
    let error = |error: crate::il2cpp::Error| error.to_string();
    let class = |name: &str| -> Result<*mut Class, String> {
        api.class("GRFight.dll", "GameRiver.Fight", name)
            .map_err(error)
    };
    let system = class("DeadEffectSystem")?;
    let controller = class("DeadRebirthController")?;
    let task = class("RebirthTask")?;
    let position = api.method(task, "GetPosAndRotation", 2).map_err(error)?;
    let unit = class("FightMech")?;
    Ok(RebirthMetadata {
        system_class: system as usize,
        controllers: api.field(system, "deadEffectControllers").map_err(error)? as usize,
        controller_class: controller as usize,
        tasks: api.field(controller, "rebirthTasks").map_err(error)? as usize,
        task_unit: api.field(task, "mFightMech").map_err(error)? as usize,
        position: api.method_pointer(position).map_err(error)? as usize,
        position_method: position as usize,
        unit_count: api.field(unit, "rebirthCount").map_err(error)? as usize,
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

/// Every unit waiting to be reborn, ascending by its id.
pub(crate) fn read(
    api: Api,
    system: *mut Object,
    metadata: &RebirthMetadata,
    capture: &CaptureState,
) -> Result<Vec<RebirthState>, String> {
    let controllers: *mut Object = field(
        api,
        system,
        metadata.controllers,
        "DeadEffectSystem.deadEffectControllers",
    )?;
    let mut controller = None;
    for index in 0..list_count(api, controllers, 128)? {
        let candidate = list_item(api, controllers, index)?;
        if api.object_class(candidate).map(|class| class as usize)
            == Some(metadata.controller_class)
        {
            controller = Some(candidate);
            break;
        }
    }
    let Some(controller) = controller else {
        return Ok(Vec::new());
    };
    let tasks: *mut Object = field(
        api,
        controller,
        metadata.tasks,
        "DeadRebirthController.rebirthTasks",
    )?;
    if tasks.is_null() {
        return Ok(Vec::new());
    }
    let values: *mut Object = own_field(api, tasks, "values")?;
    // SAFETY: the pointer was resolved from `RebirthTask.GetPosAndRotation`,
    // an instance method of two `out FVector3`s.
    let position: GetPosAndRotationFn = unsafe { std::mem::transmute(metadata.position) };
    let mut rows = Vec::new();
    for index in 0..list_count(api, values, 10_000)? {
        let task = list_item(api, values, index)?;
        let unit: *mut Object = field(api, task, metadata.task_unit, "RebirthTask.mFightMech")?;
        let unit = object_ref_from_pointer(unit as usize, capture)
            .filter(|reference| reference.kind == ObjectKind::Unit)
            .ok_or("a unit waiting to be reborn has no recorded identity")?;
        let mut at = Vector::default();
        let mut forward = Vector::default();
        // SAFETY: the arguments match the method's ABI, and the task is live.
        unsafe {
            position(
                task,
                &raw mut at,
                &raw mut forward,
                metadata.position_method as *const MethodInfo,
            );
        }
        rows.push(RebirthState {
            unit_id: unit.id,
            position: QVec3 {
                x: at.x,
                y: at.y,
                z: at.z,
            },
        });
    }
    rows.sort_by_key(|row| row.unit_id);
    Ok(rows)
}
