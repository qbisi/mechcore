//! The `projectile_reach` instrument channel: each projectile's reach check.
//!
//! `FightProjectile.Update` asks `CalculateMaxMoveDistance` and then
//! `FightCalculator.IsInRange3D` whether the projectile still stands within that
//! of its owner, and releases one that does not with no damage. The first hook
//! notes what `CalculateMaxMoveDistance` returned for which projectile; the
//! second, on the same thread, records the check it was asked for. A projectile
//! is checked once per update, so the channel grows with the projectiles in
//! flight.

use crate::capture::{capture_state, object_ref_from_pointer};
use crate::il2cpp::{Api, FieldInfo, MethodInfo, Object};
use mechcore_mcfr::{ObjectKind, ObjectRef, ProjectileReach, QVec3};
use std::{
    cell::Cell,
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

static ARMED: AtomicBool = AtomicBool::new(false);
static ORIGINAL_MAX_MOVE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_IN_RANGE_3D: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// `FightProjectile.CalculateMaxMoveDistance()`: an `FPoint`, one raw `long`.
type MaxMoveFn = unsafe extern "C" fn(*mut Object, *const MethodInfo) -> i64;
/// `FightCalculator.IsInRange3D(FightTransform, FPoint, FVector3, FPoint)`: the
/// `FVector3`, 24 bytes, is passed by reference to a copy.
type InRange3dFn = unsafe extern "C" fn(
    *mut Object,
    *mut Object,
    i64,
    *const [i64; 3],
    i64,
    *const MethodInfo,
) -> bool;

/// What the last `CalculateMaxMoveDistance` on this thread answered.
#[derive(Clone, Copy)]
struct Pending {
    projectile: usize,
    owner: usize,
    move_range: i64,
    max_move: i64,
}

thread_local! {
    static PENDING: Cell<Option<Pending>> = const { Cell::new(None) };
}

/// The fields the hooks read, resolved once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ReachMetadata {
    move_range: usize,
    owner: usize,
    position_3d: usize,
}

/// Turns the hooks' reads on for a recording that asked for the channel.
pub(crate) fn arm(armed: bool) {
    ARMED.store(armed, Ordering::Release);
    PENDING.with(|pending| pending.set(None));
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn initialize(api: Api) -> Result<ReachMetadata, String> {
    let error = |error: crate::il2cpp::Error| error.to_string();
    let class = |name: &str| {
        api.class("GRFight.dll", "GameRiver.Fight", name)
            .map_err(error)
    };
    let field = |class, name: &str| {
        api.field(class, name)
            .map(|field| field as usize)
            .map_err(error)
    };
    let projectile = class("FightProjectile")?;
    let calculator = class("FightCalculator")?;
    let transform = class("FightTransform")?;
    let metadata = ReachMetadata {
        move_range: field(projectile, "moveRange")?,
        owner: field(projectile, "owner")?,
        position_3d: field(transform, "position3D")?,
    };
    crate::capture::install_inline_hook(
        api,
        api.method(projectile, "CalculateMaxMoveDistance", 0)
            .map_err(error)?,
        max_move_hook as *const c_void,
        &ORIGINAL_MAX_MOVE,
        "FightProjectile.CalculateMaxMoveDistance",
    )?;
    crate::capture::install_inline_hook(
        api,
        api.method(calculator, "IsInRange3D", 4).map_err(error)?,
        in_range_3d_hook as *const c_void,
        &ORIGINAL_IN_RANGE_3D,
        "FightCalculator.IsInRange3D",
    )?;
    Ok(metadata)
}

unsafe extern "C" fn max_move_hook(projectile: *mut Object, method: *const MethodInfo) -> i64 {
    let original = ORIGINAL_MAX_MOVE.load(Ordering::Acquire);
    // SAFETY: the installer stores the trampoline for this exact IL2CPP method ABI.
    let original: MaxMoveFn = unsafe { std::mem::transmute(original) };
    // SAFETY: arguments are forwarded unchanged.
    let max_move = unsafe { original(projectile, method) };
    if ARMED.load(Ordering::Acquire) {
        let read = catch_unwind(AssertUnwindSafe(|| read_projectile(projectile)))
            .ok()
            .flatten();
        PENDING.with(|pending| {
            pending.set(read.map(|(owner, move_range)| Pending {
                projectile: projectile as usize,
                owner,
                move_range,
                max_move,
            }));
        });
    }
    max_move
}

unsafe extern "C" fn in_range_3d_hook(
    calculator: *mut Object,
    transform: *mut Object,
    radius: i64,
    position: *const [i64; 3],
    range: i64,
    method: *const MethodInfo,
) -> bool {
    let original = ORIGINAL_IN_RANGE_3D.load(Ordering::Acquire);
    // SAFETY: the installer stores the trampoline for this exact IL2CPP method ABI.
    let original: InRange3dFn = unsafe { std::mem::transmute(original) };
    // The caller's copy of the position, read before the call may reuse it.
    // SAFETY: the ABI passes a valid pointer to the 24-byte `FVector3`.
    let at = (!position.is_null()).then(|| unsafe { *position });
    // SAFETY: arguments are forwarded unchanged.
    let in_range = unsafe { original(calculator, transform, radius, position, range, method) };
    if !ARMED.load(Ordering::Acquire) {
        return in_range;
    }
    let pending = PENDING.with(Cell::take);
    if let (Some(pending), Some(at)) = (pending, at)
        && pending.max_move == range
    {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            record(pending, transform, radius, at, in_range);
        }));
    }
    in_range
}

fn runtime_api() -> Option<Api> {
    crate::capture::runtime_api()
}

fn metadata() -> Option<ReachMetadata> {
    capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .metadata
        .reach
}

/// The projectile's owner and `moveRange`.
fn read_projectile(projectile: *mut Object) -> Option<(usize, i64)> {
    let api = runtime_api()?;
    let fields = metadata()?;
    let owner = api
        .field_value::<*mut Object>(projectile, fields.owner as *mut FieldInfo)
        .ok()?;
    let move_range = api
        .field_value::<i64>(projectile, fields.move_range as *mut FieldInfo)
        .ok()?;
    Some((owner as usize, move_range))
}

fn record(pending: Pending, transform: *mut Object, radius: i64, at: [i64; 3], in_range: bool) {
    let Some(api) = runtime_api() else {
        return;
    };
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed || !state.instruments.projectile_reach {
        return;
    }
    if !state.in_update {
        state.fail("a projectile's reach was checked outside the captured logic tick".into());
        return;
    }
    let row = (|| {
        let fields = state
            .metadata
            .reach
            .ok_or("reach metadata is unavailable")?;
        if transform.is_null() {
            return Err("the reach check has no transform".to_owned());
        }
        let position = api
            .field_value::<[i64; 3]>(transform, fields.position_3d as *mut FieldInfo)
            .map_err(|error| format!("cannot read FightTransform.position3D: {error}"))?;
        let id = state
            .projectile_ids
            .get(&pending.projectile)
            .copied()
            .ok_or("a reach check names a projectile the capture has not released")?;
        Ok(ProjectileReach {
            projectile: ObjectRef::new(ObjectKind::Projectile, id),
            owner: object_ref_from_pointer(pending.owner, &state),
            move_range_raw: pending.move_range,
            max_move_raw: pending.max_move,
            transform_position: vector(position),
            radius_raw: radius,
            position: vector(at),
            in_range,
        })
    })();
    match row {
        Ok(row) => state.projectile_reaches.push(row),
        Err(error) => state.fail(format!("projectile reach: {error}")),
    }
}

const fn vector([x, y, z]: [i64; 3]) -> QVec3 {
    QVec3 { x, y, z }
}
