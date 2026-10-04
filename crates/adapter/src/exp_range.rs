//! The `exp_range` instrument channel: each kill's search for the formations
//! near enough to share its experience.
//!
//! `ExpSystem.DoCalculateExp` lists the formations that hit the target and then
//! calls `AddRangeUnit`, which asks the killer's side's `mechQuadtree` for a
//! square `assistExpRange` wide around the target and appends the formation of
//! each unit it finds in range. The hook reads the list before and after the
//! call, so a row says which formations the search added to which were there.

use crate::capture::{capture_state, list_count, list_item, object_ref_from_pointer};
use crate::il2cpp::{Api, MethodInfo, Object};
use mechcore_mcfr::ExpRange;
use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

static ARMED: AtomicBool = AtomicBool::new(false);
static ORIGINAL_ADD_RANGE_UNIT: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// The most formations a list read here may hold.
const MAX_FORMATIONS: i32 = 4096;

/// `ExpSystem.AddRangeUnit(List<MechTeam>, IFightGroup, FightActor)`.
type AddRangeUnitFn =
    unsafe extern "C" fn(*mut Object, *mut Object, *mut Object, *mut Object, *const MethodInfo);

/// Turns the hook's reads on for a recording that asked for the channel.
pub(crate) fn arm(armed: bool) {
    ARMED.store(armed, Ordering::Release);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn initialize(api: Api) -> Result<(), String> {
    let error = |error: crate::il2cpp::Error| error.to_string();
    let system = api
        .class("GRFight.dll", "GameRiver.Fight", "ExpSystem")
        .map_err(error)?;
    crate::capture::install_inline_hook(
        api,
        api.method(system, "AddRangeUnit", 3).map_err(error)?,
        add_range_unit_hook as *const c_void,
        &ORIGINAL_ADD_RANGE_UNIT,
        "ExpSystem.AddRangeUnit",
    )
}

unsafe extern "C" fn add_range_unit_hook(
    system: *mut Object,
    units: *mut Object,
    group: *mut Object,
    target: *mut Object,
    method: *const MethodInfo,
) {
    let original = ORIGINAL_ADD_RANGE_UNIT.load(Ordering::Acquire);
    // SAFETY: the installer stores the trampoline for this exact IL2CPP method ABI.
    let original: AddRangeUnitFn = unsafe { std::mem::transmute(original) };
    let before = ARMED
        .load(Ordering::Acquire)
        .then(|| catch_unwind(AssertUnwindSafe(|| read_list(units))).ok())
        .flatten();
    // SAFETY: arguments are forwarded unchanged.
    unsafe { original(system, units, group, target, method) };
    if let Some(before) = before {
        let after = catch_unwind(AssertUnwindSafe(|| read_list(units)))
            .ok()
            .unwrap_or_else(|| Err("the formation list could not be read".into()));
        let _ = catch_unwind(AssertUnwindSafe(|| record(target, before, after)));
    }
}

/// The `MechTeam` pointers a `List<MechTeam>` holds, in order.
fn read_list(list: *mut Object) -> Result<Vec<usize>, String> {
    let api = crate::capture::runtime_api().ok_or("the IL2CPP runtime is unavailable")?;
    let count = list_count(api, list, MAX_FORMATIONS)?;
    (0..count)
        .map(|index| list_item(api, list, index).map(|item| item as usize))
        .collect()
}

fn record(
    target: *mut Object,
    before: Result<Vec<usize>, String>,
    after: Result<Vec<usize>, String>,
) {
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed || !state.instruments.exp_range {
        return;
    }
    if !state.in_update {
        state.fail("a kill's experience was shared outside the captured logic tick".into());
        return;
    }
    let row = (|| {
        let before = before?;
        let after = after?;
        if after.get(..before.len()) != Some(before.as_slice()) {
            return Err("the search changed the formations listed before it".to_owned());
        }
        let formation =
            |pointer: &usize| {
                state.formation_ids.get(pointer).copied().ok_or_else(|| {
                    "a shared formation is one the capture has not numbered".to_owned()
                })
            };
        Ok(ExpRange {
            target: object_ref_from_pointer(target as usize, &state),
            before: before.iter().map(formation).collect::<Result<_, _>>()?,
            added: after[before.len()..]
                .iter()
                .map(formation)
                .collect::<Result<_, _>>()?,
        })
    })();
    match row {
        Ok(row) => state.exp_ranges.push(row),
        Err(error) => state.fail(format!("exp range: {error}")),
    }
}
