use crate::{
    il2cpp::{Api, FieldInfo, MethodInfo, Object, argument, object_argument},
    runtime::Runtime,
};
use jpeg_encoder::{ColorType, Encoder};
use mechcore_mcfr::{
    BuildingState, Domain, DurableContext, Event, EventPayload, Gauge, IdentityContract,
    MCFR_SCHEMA_VERSION, MotionState, NumericConvention, ObjectKind, ObjectRef,
    PersonalShieldState, Pose, ProjectileState, Rational, StatusState, TransitionEvents, UnitState,
    Vec3, Visibility, WorldSnapshot,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicPtr, Ordering},
    },
};

const QUEUE_CAPACITY: usize = 4096;
const Q32_ONE: i128 = 1_i128 << 32;
const DISTANCE_UNITS_PER_METER: u64 = 1_000;
const ROTATION_UNITS_PER_DEGREE: u64 = 1_000;
const TIME_UNITS_PER_SECOND: u64 = 2_000;
const CAPTURE_WIDTH: u16 = 2_560;
const CAPTURE_HEIGHT: u16 = 1_600;
const CAPTURE_FRAME_RATE: i32 = 20;
pub(crate) const CALIBRATION_VIEW: &str = "calibration_topdown";
pub(crate) const CALIBRATION_CAMERA_HEIGHT: f32 = 1_070.0;
pub(crate) const CALIBRATION_CAMERA_Z: f32 = -1_070.0;
pub(crate) const CALIBRATION_CAMERA_PITCH_DEGREES: f32 = 45.0;
pub(crate) const CALIBRATION_FIELD_OF_VIEW_DEGREES: f32 = 20.0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FixedPoint {
    raw: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FixedVec2 {
    x: FixedPoint,
    y: FixedPoint,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FixedVec3 {
    x: FixedPoint,
    y: FixedPoint,
    z: FixedPoint,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FixedRect {
    center: FixedVec2,
    size: FixedVec2,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UnityVec3 {
    x: f32,
    y: f32,
    z: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UnityRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Clone)]
pub(crate) enum CaptureMessage {
    Initial {
        context: DurableContext,
        state: WorldSnapshot,
        frame: Option<Vec<u8>>,
    },
    Transition {
        events: TransitionEvents,
        state: WorldSnapshot,
        terminal: bool,
        frame: Option<Vec<u8>>,
    },
    Failure(String),
}

enum PendingVisualMessage {
    Initial {
        context: DurableContext,
        state: WorldSnapshot,
    },
    Transition {
        events: TransitionEvents,
        state: WorldSnapshot,
        terminal: bool,
    },
}

struct VisualCapture {
    api: Api,
    camera_handle: u32,
    camera_transform_handle: u32,
    controlled_handles: [u32; 4],
    original_controlled_enabled: [bool; 4],
    texture_handle: u32,
    screen_class: usize,
    application_class: usize,
    set_resolution: usize,
    destroy_immediate: usize,
    width: u16,
    height: u16,
    original_screen_width: i32,
    original_screen_height: i32,
    original_fullscreen: bool,
    original_target_frame_rate: i32,
    original_camera_position: UnityVec3,
    original_camera_euler_angles: UnityVec3,
    original_camera_orthographic: bool,
    original_camera_orthographic_size: f32,
    original_camera_field_of_view: f32,
    original_camera_far_clip_plane: f32,
}

#[derive(Default)]
struct Metadata {
    projectile_system_class: usize,
    projectile_controllers: usize,
    buff_list: usize,
    buff_duration_time: usize,
    buff_max_duration_time: usize,
    buff_step_time: usize,
    buff_step_time_config: usize,
    motion_fsm: usize,
    motion_idle_state_class: usize,
    motion_move_state_class: usize,
    motion_attack_state_class: usize,
    motion_stop_state_class: usize,
}

#[derive(Default)]
#[allow(clippy::struct_excessive_bools)] // The booleans mirror independent native hook boundaries.
struct CaptureState {
    availability: Option<String>,
    metadata: Metadata,
    armed: bool,
    initialized: bool,
    queue: VecDeque<CaptureMessage>,
    unit_ids: BTreeMap<usize, u64>,
    building_ids: BTreeMap<usize, u64>,
    projectile_ids: BTreeMap<usize, u64>,
    status_ids: BTreeMap<usize, u64>,
    formation_ids: BTreeMap<usize, u64>,
    next_unit_id: u64,
    next_building_id: u64,
    next_projectile_id: u64,
    next_status_id: u64,
    next_formation_id: u64,
    in_update: bool,
    traces: Vec<NativeTrace>,
    visual: Option<VisualCapture>,
    pending_visual: Option<PendingVisualMessage>,
    render_completed: bool,
}

impl CaptureState {
    fn reset_session(&mut self) {
        self.armed = false;
        self.initialized = false;
        self.queue.clear();
        self.unit_ids.clear();
        self.building_ids.clear();
        self.projectile_ids.clear();
        self.status_ids.clear();
        self.formation_ids.clear();
        self.next_unit_id = 1;
        self.next_building_id = 1;
        self.next_projectile_id = 1;
        self.next_status_id = 1;
        self.next_formation_id = 1;
        self.in_update = false;
        self.traces.clear();
        self.visual = None;
        self.pending_visual = None;
        self.render_completed = false;
    }

    fn push(&mut self, message: CaptureMessage) -> Result<(), String> {
        if self.queue.len() >= QUEUE_CAPACITY {
            self.armed = false;
            return Err("capture queue overflowed before MCFR writer drained it".into());
        }
        self.queue.push_back(message);
        Ok(())
    }

    fn fail(&mut self, reason: String) {
        self.armed = false;
        if self.queue.len() < QUEUE_CAPACITY {
            self.queue.push_back(CaptureMessage::Failure(reason));
        }
    }
}

impl VisualCapture {
    #[allow(clippy::too_many_lines)]
    fn new(runtime: &Runtime) -> Result<Self, String> {
        let api = runtime.api;
        let screen_class = api
            .class("UnityEngine.CoreModule.dll", "UnityEngine", "Screen")
            .map_err(|error| error.to_string())?;
        let application_class = api
            .class("UnityEngine.CoreModule.dll", "UnityEngine", "Application")
            .map_err(|error| error.to_string())?;
        let original_target_frame_rate = api
            .invoke_static(application_class, "get_targetFrameRate", &mut [])
            .and_then(|value| api.unbox::<i32>(value, "Application.targetFrameRate"))
            .map_err(|error| error.to_string())?;
        let (original_screen_width, original_screen_height, original_fullscreen) =
            screen_state(api, screen_class)?;
        if original_screen_width <= 0 || original_screen_height <= 0 {
            return Err("screen dimensions must be positive".into());
        }
        let set_resolution = api
            .class_method_with_parameter_types(
                screen_class,
                "SetResolution",
                &["System.Int32", "System.Int32", "System.Boolean"],
            )
            .map_err(|error| error.to_string())?;
        let camera_class = api
            .class("UnityEngine.CoreModule.dll", "UnityEngine", "Camera")
            .map_err(|error| error.to_string())?;
        let camera = api
            .invoke_static(camera_class, "get_main", &mut [])
            .map_err(|error| error.to_string())?;
        if camera.is_null() {
            return Err("Unity main camera is unavailable".into());
        }
        let camera_transform = api
            .invoke(camera, "get_transform", &mut [])
            .map_err(|error| error.to_string())?;
        if camera_transform.is_null() {
            return Err("Unity main camera transform is unavailable".into());
        }
        let original_camera_position = api
            .invoke_value::<UnityVec3>(camera_transform, "get_position", &mut [])
            .map_err(|error| error.to_string())?;
        let original_camera_euler_angles = api
            .invoke_value::<UnityVec3>(camera_transform, "get_eulerAngles", &mut [])
            .map_err(|error| error.to_string())?;
        let original_camera_orthographic = api
            .invoke_value::<bool>(camera, "get_orthographic", &mut [])
            .map_err(|error| error.to_string())?;
        let original_camera_orthographic_size = api
            .invoke_value::<f32>(camera, "get_orthographicSize", &mut [])
            .map_err(|error| error.to_string())?;
        let original_camera_field_of_view = api
            .invoke_value::<f32>(camera, "get_fieldOfView", &mut [])
            .map_err(|error| error.to_string())?;
        let original_camera_far_clip_plane = api
            .invoke_value::<f32>(camera, "get_farClipPlane", &mut [])
            .map_err(|error| error.to_string())?;
        let (controlled, original_controlled_enabled) = controlled_camera_behaviours(api)?;
        let destroy_immediate = destroy_immediate_method(api)? as usize;
        let [
            camera_handle,
            camera_transform_handle,
            brain,
            horizontal,
            orbit,
            zoom,
        ] = gc_handles(
            api,
            [
                camera,
                camera_transform,
                controlled[0],
                controlled[1],
                controlled[2],
                controlled[3],
            ],
        )?;
        let capture = Self {
            api,
            camera_handle,
            camera_transform_handle,
            controlled_handles: [brain, horizontal, orbit, zoom],
            original_controlled_enabled,
            texture_handle: 0,
            screen_class: screen_class as usize,
            application_class: application_class as usize,
            set_resolution: set_resolution as usize,
            destroy_immediate,
            width: 0,
            height: 0,
            original_screen_width,
            original_screen_height,
            original_fullscreen,
            original_target_frame_rate,
            original_camera_position,
            original_camera_euler_angles,
            original_camera_orthographic,
            original_camera_orthographic_size,
            original_camera_field_of_view,
            original_camera_far_clip_plane,
        };
        let setup = capture.configure();
        if let Err(error) = setup {
            return match capture.restore(true) {
                Ok(()) => Err(error),
                Err(restore) => Err(format!("{error}; cannot restore capture state: {restore}")),
            };
        }
        Ok(capture)
    }

    fn configure(&self) -> Result<(), String> {
        self.set_target_frame_rate(CAPTURE_FRAME_RATE)
            .and_then(|()| {
                self.set_screen_resolution(
                    i32::from(CAPTURE_WIDTH),
                    i32::from(CAPTURE_HEIGHT),
                    false,
                )
            })
            .and_then(|()| self.apply_calibration())
    }

    fn apply_calibration(&self) -> Result<(), String> {
        let mut disabled = false;
        for handle in self.controlled_handles {
            let controller = self
                .api
                .gc_handle_target(handle)
                .map_err(|error| error.to_string())?;
            self.api
                .invoke_void(controller, "set_enabled", &mut [argument(&mut disabled)])
                .map_err(|error| error.to_string())?;
        }
        let camera = self
            .api
            .gc_handle_target(self.camera_handle)
            .map_err(|error| error.to_string())?;
        let transform = self
            .api
            .gc_handle_target(self.camera_transform_handle)
            .map_err(|error| error.to_string())?;
        let mut position = UnityVec3 {
            x: 0.0,
            y: CALIBRATION_CAMERA_HEIGHT,
            z: CALIBRATION_CAMERA_Z,
        };
        let mut rotation = UnityVec3 {
            x: CALIBRATION_CAMERA_PITCH_DEGREES,
            y: 0.0,
            z: 0.0,
        };
        let mut orthographic = false;
        let mut field_of_view = CALIBRATION_FIELD_OF_VIEW_DEGREES;
        let mut far_clip_plane = 4_000.0_f32;
        self.api
            .invoke_void(transform, "set_position", &mut [argument(&mut position)])
            .and_then(|()| {
                self.api
                    .invoke_void(transform, "set_eulerAngles", &mut [argument(&mut rotation)])
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_orthographic",
                    &mut [argument(&mut orthographic)],
                )
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_fieldOfView",
                    &mut [argument(&mut field_of_view)],
                )
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_farClipPlane",
                    &mut [argument(&mut far_clip_plane)],
                )
            })
            .map_err(|error| error.to_string())
    }

    fn frame(&mut self) -> Result<Vec<u8>, String> {
        self.ensure_capture_texture()?;
        let texture = self
            .api
            .gc_handle_target(self.texture_handle)
            .map_err(|error| error.to_string())?;
        let mut source = UnityRect {
            x: 0.0,
            y: 0.0,
            width: f32::from(self.width),
            height: f32::from(self.height),
        };
        let mut destination_x = 0_i32;
        let mut destination_y = 0_i32;
        let mut recalculate_mipmaps = false;
        self.api
            .invoke_void(
                texture,
                "ReadPixels",
                &mut [
                    argument(&mut source),
                    argument(&mut destination_x),
                    argument(&mut destination_y),
                    argument(&mut recalculate_mipmaps),
                ],
            )
            .map_err(|error| error.to_string())?;
        let mut update_mipmaps = false;
        let mut make_no_longer_readable = false;
        self.api
            .invoke_void(
                texture,
                "Apply",
                &mut [
                    argument(&mut update_mipmaps),
                    argument(&mut make_no_longer_readable),
                ],
            )
            .map_err(|error| error.to_string())?;
        let raw = self
            .api
            .invoke(texture, "GetRawTextureData", &mut [])
            .and_then(|bytes| self.api.byte_array(bytes))
            .map_err(|error| error.to_string());
        let width = self.width;
        let height = self.height;
        let raw = raw?;
        let pixels = usize::from(width)
            .checked_mul(usize::from(height))
            .ok_or("captured image dimensions overflow")?;
        let channels = raw
            .len()
            .checked_div(pixels)
            .filter(|channels| pixels * channels == raw.len() && matches!(channels, 3 | 4))
            .ok_or_else(|| {
                format!(
                    "unsupported captured texture layout: {} bytes for {width}x{height}",
                    raw.len()
                )
            })?;
        let row_bytes = usize::from(width) * channels;
        let mut rgb = Vec::with_capacity(pixels * 3);
        for row in raw.chunks_exact(row_bytes).rev() {
            for pixel in row.chunks_exact(channels) {
                rgb.extend_from_slice(&pixel[..3]);
            }
        }
        let mut jpeg = Vec::new();
        Encoder::new(&mut jpeg, 90)
            .encode(&rgb, width, height, ColorType::Rgb)
            .map_err(|error| format!("cannot encode captured JPEG: {error}"))?;
        Ok(jpeg)
    }

    fn restore(mut self, restore_camera: bool) -> Result<(), String> {
        let texture_result = self.destroy_texture();
        let camera_result = if restore_camera {
            self.restore_camera_and_controls()
        } else {
            Ok(())
        };
        let resolution_result = self.set_screen_resolution(
            self.original_screen_width,
            self.original_screen_height,
            self.original_fullscreen,
        );
        let frame_rate_result = self.set_target_frame_rate(self.original_target_frame_rate);
        self.api.free_gc_handle(self.camera_handle);
        self.api.free_gc_handle(self.camera_transform_handle);
        for handle in self.controlled_handles {
            self.api.free_gc_handle(handle);
        }
        if self.texture_handle != 0 {
            self.api.free_gc_handle(self.texture_handle);
        }
        self.texture_handle = 0;
        let failures: Vec<_> = [
            texture_result,
            camera_result,
            resolution_result,
            frame_rate_result,
        ]
        .into_iter()
        .filter_map(Result::err)
        .collect();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    fn destroy_texture(&self) -> Result<(), String> {
        if self.texture_handle == 0 {
            return Ok(());
        }
        let texture = self
            .api
            .gc_handle_target(self.texture_handle)
            .map_err(|error| error.to_string())?;
        let mut allow_destroying_assets = false;
        self.api
            .invoke_raw(
                self.destroy_immediate as *const MethodInfo,
                ptr::null_mut(),
                &mut [
                    object_argument(texture),
                    argument(&mut allow_destroying_assets),
                ],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn ensure_capture_texture(&mut self) -> Result<(), String> {
        if self.texture_handle != 0 {
            return Ok(());
        }
        let screen_class = self.screen_class as *mut crate::il2cpp::Class;
        let width = self
            .api
            .invoke_static(screen_class, "get_width", &mut [])
            .and_then(|value| self.api.unbox::<i32>(value, "Screen.width"))
            .map_err(|error| error.to_string())?;
        let height = self
            .api
            .invoke_static(screen_class, "get_height", &mut [])
            .and_then(|value| self.api.unbox::<i32>(value, "Screen.height"))
            .map_err(|error| error.to_string())?;
        if (width, height) != (i32::from(CAPTURE_WIDTH), i32::from(CAPTURE_HEIGHT)) {
            return Err(format!(
                "capture resolution did not settle at {CAPTURE_WIDTH}x{CAPTURE_HEIGHT}: {width}x{height}"
            ));
        }
        let texture_class = self
            .api
            .class("UnityEngine.CoreModule.dll", "UnityEngine", "Texture2D")
            .map_err(|error| error.to_string())?;
        let texture =
            create_capture_texture(self.api, texture_class, CAPTURE_WIDTH, CAPTURE_HEIGHT)?;
        self.texture_handle = self
            .api
            .gc_handle(texture)
            .map_err(|error| error.to_string())?;
        self.width = CAPTURE_WIDTH;
        self.height = CAPTURE_HEIGHT;
        Ok(())
    }

    fn set_screen_resolution(
        &self,
        mut width: i32,
        mut height: i32,
        mut fullscreen: bool,
    ) -> Result<(), String> {
        self.api
            .invoke_raw(
                self.set_resolution as *const MethodInfo,
                ptr::null_mut(),
                &mut [
                    argument(&mut width),
                    argument(&mut height),
                    argument(&mut fullscreen),
                ],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn set_target_frame_rate(&self, mut frame_rate: i32) -> Result<(), String> {
        self.api
            .invoke_static(
                self.application_class as *mut crate::il2cpp::Class,
                "set_targetFrameRate",
                &mut [argument(&mut frame_rate)],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn restore_camera_and_controls(&self) -> Result<(), String> {
        let camera = self
            .api
            .gc_handle_target(self.camera_handle)
            .map_err(|error| error.to_string())?;
        let transform = self
            .api
            .gc_handle_target(self.camera_transform_handle)
            .map_err(|error| error.to_string())?;
        let mut position = self.original_camera_position;
        let mut rotation = self.original_camera_euler_angles;
        let mut orthographic = self.original_camera_orthographic;
        let mut orthographic_size = self.original_camera_orthographic_size;
        let mut field_of_view = self.original_camera_field_of_view;
        let mut far_clip_plane = self.original_camera_far_clip_plane;
        self.api
            .invoke_void(transform, "set_position", &mut [argument(&mut position)])
            .and_then(|()| {
                self.api
                    .invoke_void(transform, "set_eulerAngles", &mut [argument(&mut rotation)])
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_orthographic",
                    &mut [argument(&mut orthographic)],
                )
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_orthographicSize",
                    &mut [argument(&mut orthographic_size)],
                )
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_fieldOfView",
                    &mut [argument(&mut field_of_view)],
                )
            })
            .and_then(|()| {
                self.api.invoke_void(
                    camera,
                    "set_farClipPlane",
                    &mut [argument(&mut far_clip_plane)],
                )
            })
            .map_err(|error| error.to_string())?;
        for (index, handle) in self.controlled_handles.iter().copied().enumerate() {
            let controller = self
                .api
                .gc_handle_target(handle)
                .map_err(|error| error.to_string())?;
            let mut enabled = self.original_controlled_enabled[index];
            self.api
                .invoke_void(controller, "set_enabled", &mut [argument(&mut enabled)])
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

fn controlled_camera_behaviours(api: Api) -> Result<([*mut Object; 4], [bool; 4]), String> {
    let manual_camera_class = api
        .class("GRClient.dll", "GameRiver.Client", "GROverAllManualCam")
        .map_err(|error| error.to_string())?;
    let manual_camera = api
        .find_object_of_class(manual_camera_class)
        .map_err(|error| error.to_string())?;
    let horizontal = api
        .invoke(
            manual_camera,
            "GetHorizontalCameraMovementController",
            &mut [],
        )
        .map_err(|error| error.to_string())?;
    let orbit = api
        .invoke(manual_camera, "GetOrbitCameraController", &mut [])
        .map_err(|error| error.to_string())?;
    let zoom = api
        .invoke(manual_camera, "GetZoomCameraController", &mut [])
        .map_err(|error| error.to_string())?;
    let brain_class = api
        .class("Cinemachine.dll", "Cinemachine", "CinemachineBrain")
        .map_err(|error| error.to_string())?;
    let brain = api
        .find_object_of_class(brain_class)
        .map_err(|error| error.to_string())?;
    let controlled = [brain, horizontal, orbit, zoom];
    if controlled.iter().any(|object| object.is_null()) {
        return Err("one or more native camera controllers are unavailable".into());
    }
    let mut enabled = [false; 4];
    for (index, object) in controlled.iter().copied().enumerate() {
        enabled[index] = api
            .invoke_value::<bool>(object, "get_enabled", &mut [])
            .map_err(|error| error.to_string())?;
    }
    Ok((controlled, enabled))
}

fn destroy_immediate_method(api: Api) -> Result<*const MethodInfo, String> {
    let object = api
        .class("UnityEngine.CoreModule.dll", "UnityEngine", "Object")
        .map_err(|error| error.to_string())?;
    api.method(object, "DestroyImmediate", 2)
        .map_err(|error| error.to_string())
}

fn gc_handles<const N: usize>(api: Api, objects: [*mut Object; N]) -> Result<[u32; N], String> {
    let mut handles = [0_u32; N];
    for (index, object) in objects.into_iter().enumerate() {
        match api.gc_handle(object) {
            Ok(handle) => handles[index] = handle,
            Err(error) => {
                for handle in handles {
                    api.free_gc_handle(handle);
                }
                return Err(error.to_string());
            }
        }
    }
    Ok(handles)
}

fn screen_state(api: Api, screen: *mut crate::il2cpp::Class) -> Result<(i32, i32, bool), String> {
    let width = api
        .invoke_static(screen, "get_width", &mut [])
        .and_then(|value| api.unbox::<i32>(value, "Screen.width"))
        .map_err(|error| error.to_string())?;
    let height = api
        .invoke_static(screen, "get_height", &mut [])
        .and_then(|value| api.unbox::<i32>(value, "Screen.height"))
        .map_err(|error| error.to_string())?;
    let fullscreen = api
        .invoke_static(screen, "get_fullScreen", &mut [])
        .and_then(|value| api.unbox::<bool>(value, "Screen.fullScreen"))
        .map_err(|error| error.to_string())?;
    Ok((width, height, fullscreen))
}

fn create_capture_texture(
    api: Api,
    texture_class: *mut crate::il2cpp::Class,
    width: u16,
    height: u16,
) -> Result<*mut Object, String> {
    let texture = api
        .allocate_object(texture_class)
        .map_err(|error| error.to_string())?;
    let constructor = api
        .method_with_parameter_types(
            texture,
            ".ctor",
            &[
                "System.Int32",
                "System.Int32",
                "UnityEngine.TextureFormat",
                "System.Boolean",
            ],
        )
        .map_err(|error| error.to_string())?;
    let mut texture_width = i32::from(width);
    let mut texture_height = i32::from(height);
    let mut rgb24 = 3_i32;
    let mut mip_chain = false;
    api.invoke_raw(
        constructor,
        texture.cast(),
        &mut [
            argument(&mut texture_width),
            argument(&mut texture_height),
            argument(&mut rgb24),
            argument(&mut mip_chain),
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(texture)
}

static CAPTURE: OnceLock<Mutex<CaptureState>> = OnceLock::new();
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_UPDATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_MATCH_UPDATE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_POST_RENDER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_PROJECTILE_ADD: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_PROJECTILE_DESTROY: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_DAMAGE_PERFORM: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

enum NativeTrace {
    ProjectileReleased {
        projectile_id: u64,
        owner: usize,
        target: usize,
    },
    ProjectileRemoved {
        projectile_id: u64,
        owner: usize,
        target: usize,
        position: Vec3,
        intercepted: bool,
    },
    Damage {
        source: Option<ObjectRef>,
        target: usize,
        amount: i64,
    },
}

fn capture_state() -> &'static Mutex<CaptureState> {
    CAPTURE.get_or_init(|| Mutex::new(CaptureState::default()))
}

pub(crate) fn initialize(runtime: &mut Runtime) {
    RUNTIME.store(ptr::from_mut(runtime), Ordering::Release);
    let result = initialize_inner(runtime);
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match result {
        Ok(metadata) => {
            state.metadata = metadata;
            state.availability = None;
        }
        Err(error) => state.availability = Some(error),
    }
}

#[allow(clippy::too_many_lines)]
fn initialize_inner(runtime: &Runtime) -> Result<Metadata, String> {
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    return Err("logic-tick recording is supported only by the macOS aarch64 adapter".into());

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        let api = runtime.api;
        let fight = api
            .class("GRFight.dll", "GameRiver.Fight", "FightController")
            .map_err(|error| error.to_string())?;
        let update = api
            .method(fight, "Update", 0)
            .map_err(|error| error.to_string())?;
        let match_client = api
            .class("GRClient.dll", "GameRiver.Client", "MatchClient")
            .map_err(|error| error.to_string())?;
        let match_update = api
            .method(match_client, "Update", 0)
            .map_err(|error| error.to_string())?;
        let camera = api
            .class("UnityEngine.CoreModule.dll", "UnityEngine", "Camera")
            .map_err(|error| error.to_string())?;
        let post_render = api
            .method(camera, "FireOnPostRender", 1)
            .map_err(|error| error.to_string())?;
        let projectile_system = api
            .class("GRFight.dll", "GameRiver.Fight", "ProjectileSystem")
            .map_err(|error| error.to_string())?;
        let projectile_controllers = api
            .field(projectile_system, "projectileControllers")
            .map_err(|error| error.to_string())?;
        let buff_manager = api
            .class("GRFight.dll", "GameRiver.Fight", "BuffManager")
            .map_err(|error| error.to_string())?;
        let buff_list = api
            .field(buff_manager, "buffs")
            .map_err(|error| error.to_string())?;
        let buff = api
            .class("GRFight.dll", "GameRiver.Fight", "Buff")
            .map_err(|error| error.to_string())?;
        let buff_duration_time = api
            .field(buff, "durationTime")
            .map_err(|error| error.to_string())?;
        let buff_max_duration_time = api
            .field(buff, "maxDurationtime")
            .map_err(|error| error.to_string())?;
        let buff_step_time = api
            .field(buff, "stepTime")
            .map_err(|error| error.to_string())?;
        let buff_step_time_config = api
            .field(buff, "stepTimeConfig")
            .map_err(|error| error.to_string())?;
        let motion_controller = api
            .class("GRFight.dll", "GameRiver.Fight", "MotionController")
            .map_err(|error| error.to_string())?;
        let motion_fsm = api
            .field(motion_controller, "fsm")
            .map_err(|error| error.to_string())?;
        let motion_idle_state = api
            .class("GRFight.dll", "GameRiver.Fight", "MotionIdleState")
            .map_err(|error| error.to_string())?;
        let motion_move_state = api
            .class("GRFight.dll", "GameRiver.Fight", "MotionMoveState")
            .map_err(|error| error.to_string())?;
        let motion_attack_state = api
            .class("GRFight.dll", "GameRiver.Fight", "MotionAttackState")
            .map_err(|error| error.to_string())?;
        let motion_stop_state = api
            .class("GRFight.dll", "GameRiver.Fight", "MotionStopState")
            .map_err(|error| error.to_string())?;
        let damage_performer = api
            .class("GRFight.dll", "GameRiver.Fight", "DamagePerformer")
            .map_err(|error| error.to_string())?;
        let projectile_add = api
            .method(projectile_system, "AddProjectile", 1)
            .map_err(|error| error.to_string())?;
        let projectile_destroy = api
            .method(projectile_system, "Destroy", 2)
            .map_err(|error| error.to_string())?;
        let damage_perform = api
            .method(damage_performer, "Perform", 3)
            .map_err(|error| error.to_string())?;
        install_projectile_add_hook(api, projectile_add)?;
        install_projectile_destroy_hook(api, projectile_destroy)?;
        install_damage_perform_hook(api, damage_perform)?;
        install_update_hook(api, update)?;
        install_match_update_hook(api, match_update)?;
        install_post_render_hook(api, post_render)?;
        Ok(Metadata {
            projectile_system_class: projectile_system as usize,
            projectile_controllers: projectile_controllers as usize,
            buff_list: buff_list as usize,
            buff_duration_time: buff_duration_time as usize,
            buff_max_duration_time: buff_max_duration_time as usize,
            buff_step_time: buff_step_time as usize,
            buff_step_time_config: buff_step_time_config as usize,
            motion_fsm: motion_fsm as usize,
            motion_idle_state_class: motion_idle_state as usize,
            motion_move_state_class: motion_move_state as usize,
            motion_attack_state_class: motion_attack_state as usize,
            motion_stop_state_class: motion_stop_state as usize,
        })
    }
}

pub(crate) fn start(runtime: &Runtime, visual: bool) -> Result<(), String> {
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(error) = &state.availability {
        return Err(format!("native capture is unavailable: {error}"));
    }
    if state.armed {
        return Err("a battle recording is already active".into());
    }
    let fight = runtime.current_fight();
    if fight.is_null() {
        return Err("fight controller is unavailable".into());
    }
    let deploying = runtime
        .api
        .invoke_value::<bool>(fight, "IsDeploying", &mut [])
        .map_err(|error| error.to_string())?;
    let fighting = runtime
        .api
        .invoke_value::<bool>(fight, "IsFighting", &mut [])
        .map_err(|error| error.to_string())?;
    if !deploying || fighting {
        return Err("recording requires Training Ground deployment before fighting".into());
    }
    let current_match = runtime.current_match();
    if current_match.is_null() {
        return Err("active match disappeared before recording started".into());
    }
    state.reset_session();
    if visual {
        state.visual = Some(VisualCapture::new(runtime)?);
    }
    state.armed = true;
    drop(state);

    if let Err(error) = runtime
        .api
        .invoke_void(current_match, "ChangeProcessState", &mut [])
    {
        let message = format!("cannot start fight: {error}");
        abort(&message);
        if let Err(restore_error) = stop() {
            return Err(format!("{message}; cannot restore camera: {restore_error}"));
        }
        return Err(message);
    }
    Ok(())
}

pub(crate) fn stop() -> Result<(), String> {
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.armed = false;
    state.pending_visual = None;
    state.traces.clear();
    let visual = state.visual.take();
    drop(state);
    if let Some(visual) = visual {
        visual.restore(true)?;
    }
    Ok(())
}

pub(crate) fn poll() -> Option<CaptureMessage> {
    capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .queue
        .pop_front()
}

pub(crate) fn abort(reason: &str) {
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.fail(reason.to_owned());
}

type UpdateFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type MatchUpdateFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type PostRenderFn = unsafe extern "C" fn(*mut Object, *const MethodInfo);
type ProjectileAddFn = unsafe extern "C" fn(*mut Object, *mut Object, *const MethodInfo);
type ProjectileDestroyFn = unsafe extern "C" fn(*mut Object, *mut Object, bool, *const MethodInfo);
type DamagePerformFn = unsafe extern "C" fn(
    *mut Object,
    *mut Object,
    *mut Object,
    *mut Object,
    *const MethodInfo,
) -> i32;

#[allow(clippy::too_many_lines)]
unsafe extern "C" fn update_hook(controller: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_UPDATE.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: install_update_hook stores the trampoline for this exact method ABI.
    let original: UpdateFn = unsafe { std::mem::transmute(original) };
    let runtime = RUNTIME.load(Ordering::Acquire);
    let mut skip_update = false;
    {
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Keep the pending logic state on screen until the main camera renders it.
        // Pixel readback happens on the following update so screen-space UI is complete.
        if state.armed && state.visual.is_some() && state.pending_visual.is_some() {
            if state.render_completed {
                state.render_completed = false;
                if let Err(error) = flush_visual_frame(&mut state) {
                    state.fail(error);
                }
            } else {
                skip_update = true;
            }
        }
        if !skip_update {
            state.in_update = state.armed;
            state.traces.clear();
        }
    }
    if skip_update {
        return;
    }
    // SAFETY: controller and MethodInfo are forwarded unchanged from IL2CPP.
    unsafe { original(controller, method) };

    let _ = catch_unwind(AssertUnwindSafe(|| {
        if runtime.is_null() {
            return;
        }
        // SAFETY: runtime is boxed for the adapter process lifetime.
        let runtime = unsafe { &*runtime };
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.in_update = false;
        if !state.armed {
            return;
        }
        let result = (|| {
            let fighting = runtime
                .api
                .invoke_value::<bool>(controller, "IsFighting", &mut [])
                .map_err(|error| error.to_string())?;
            if !state.initialized {
                if !fighting {
                    state.traces.clear();
                    return Ok(());
                }
                if !state.traces.is_empty() {
                    return Err("combat events occurred before the fighting-entry snapshot".into());
                }
                let initial = snapshot(runtime, &mut state, true)?;
                let context = durable_context(runtime)?;
                state.initialized = true;
                if let Some(visual) = state.visual.as_ref() {
                    visual.apply_calibration()?;
                    state.render_completed = false;
                    state.pending_visual = Some(PendingVisualMessage::Initial {
                        context,
                        state: initial,
                    });
                } else {
                    state.push(CaptureMessage::Initial {
                        context,
                        state: initial,
                        frame: None,
                    })?;
                }
                return Ok(());
            }
            let next = snapshot(runtime, &mut state, false)?;
            let traces = std::mem::take(&mut state.traces);
            let events = transition_events(&traces, &state);
            let terminal = !fighting;
            if let Some(visual) = state.visual.as_ref() {
                visual.apply_calibration()?;
                state.render_completed = false;
                state.pending_visual = Some(PendingVisualMessage::Transition {
                    events,
                    state: next,
                    terminal,
                });
            } else {
                state.push(CaptureMessage::Transition {
                    events,
                    state: next,
                    terminal,
                    frame: None,
                })?;
                if terminal {
                    state.armed = false;
                }
            }
            Ok::<(), String>(())
        })();
        if let Err(error) = result {
            state.fail(error);
        }
    }));
}

unsafe extern "C" fn match_update_hook(current: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_MATCH_UPDATE.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: install_match_update_hook stores the trampoline for this exact method ABI.
    let original: MatchUpdateFn = unsafe { std::mem::transmute(original) };
    // SAFETY: current and MethodInfo are forwarded unchanged from IL2CPP.
    unsafe { original(current, method) };

    let _ = catch_unwind(AssertUnwindSafe(|| {
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.armed
            || state.visual.is_none()
            || !pending_visual_is_terminal(&state)
            || !state.render_completed
        {
            return;
        }
        state.render_completed = false;
        if let Err(error) = flush_visual_frame(&mut state) {
            state.fail(error);
        }
    }));
}

unsafe extern "C" fn post_render_hook(camera: *mut Object, method: *const MethodInfo) {
    let original = ORIGINAL_POST_RENDER.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: install_post_render_hook stores the trampoline for this exact method ABI.
    let original: PostRenderFn = unsafe { std::mem::transmute(original) };
    // SAFETY: camera and MethodInfo are forwarded unchanged from IL2CPP.
    unsafe { original(camera, method) };

    let _ = catch_unwind(AssertUnwindSafe(|| {
        let mut state = capture_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.armed || state.pending_visual.is_none() {
            return;
        }
        let Some(visual) = state.visual.as_ref() else {
            return;
        };
        match visual.api.gc_handle_target(visual.camera_handle) {
            Ok(capture_camera) if capture_camera == camera => state.render_completed = true,
            Ok(_) => {}
            Err(error) => state.fail(error.to_string()),
        }
    }));
}

fn pending_visual_is_terminal(state: &CaptureState) -> bool {
    matches!(
        state.pending_visual,
        Some(PendingVisualMessage::Transition { terminal: true, .. })
    )
}

fn flush_visual_frame(state: &mut CaptureState) -> Result<(), String> {
    let frame = state
        .visual
        .as_mut()
        .ok_or("visual capture disappeared before its pending logic frame")?
        .frame()?;
    let pending = state
        .pending_visual
        .take()
        .ok_or("visual capture has no pending logic frame")?;
    let terminal = matches!(
        pending,
        PendingVisualMessage::Transition { terminal: true, .. }
    );
    if terminal {
        state.armed = false;
        state
            .visual
            .take()
            .ok_or("visual capture disappeared before camera restoration")?
            .restore(true)?;
    }
    match pending {
        PendingVisualMessage::Initial {
            context,
            state: world,
        } => state.push(CaptureMessage::Initial {
            context,
            state: world,
            frame: Some(frame),
        }),
        PendingVisualMessage::Transition {
            events,
            state: world,
            terminal,
        } => state.push(CaptureMessage::Transition {
            events,
            state: world,
            terminal,
            frame: Some(frame),
        }),
    }
}

unsafe extern "C" fn projectile_add_hook(
    system: *mut Object,
    controller: *mut Object,
    method: *const MethodInfo,
) {
    let original = ORIGINAL_PROJECTILE_ADD.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: hook installer stored a trampoline with this method ABI.
    let original: ProjectileAddFn = unsafe { std::mem::transmute(original) };
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    unsafe { original(system, controller, method) };
    let _ = catch_unwind(AssertUnwindSafe(|| record_projectile_release(controller)));
}

unsafe extern "C" fn projectile_destroy_hook(
    system: *mut Object,
    controller: *mut Object,
    intercepted: bool,
    method: *const MethodInfo,
) {
    let original = ORIGINAL_PROJECTILE_DESTROY.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        record_projectile_removal(controller, intercepted);
    }));
    // SAFETY: hook installer stored a trampoline with this method ABI.
    let original: ProjectileDestroyFn = unsafe { std::mem::transmute(original) };
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    unsafe { original(system, controller, intercepted, method) };
}

unsafe extern "C" fn damage_perform_hook(
    performer: *mut Object,
    provider: *mut Object,
    target: *mut Object,
    advanced_shield: *mut Object,
    method: *const MethodInfo,
) -> i32 {
    let original = ORIGINAL_DAMAGE_PERFORM.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: hook installer stored a trampoline with this method ABI.
    let original: DamagePerformFn = unsafe { std::mem::transmute(original) };
    // SAFETY: IL2CPP arguments are forwarded unchanged.
    let result = unsafe { original(performer, provider, target, advanced_shield, method) };
    let _ = catch_unwind(AssertUnwindSafe(|| record_damage(provider, target, result)));
    result
}

fn record_projectile_release(controller: *mut Object) {
    let runtime = RUNTIME.load(Ordering::Acquire);
    if runtime.is_null() {
        return;
    }
    // SAFETY: runtime is boxed for the process lifetime.
    let runtime = unsafe { &*runtime };
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed || !state.in_update {
        return;
    }
    let result = (|| {
        let projectile = invoke_object(runtime.api, controller, "GetFightProjectile")?;
        let pointer = projectile as usize;
        let id = match state.projectile_ids.get(&pointer) {
            Some(id) => *id,
            None => allocate(&mut state.next_projectile_id, "projectile")?,
        };
        state.projectile_ids.entry(pointer).or_insert(id);
        let owner = runtime
            .api
            .invoke(projectile, "GetOwner", &mut [])
            .map_err(|error| error.to_string())? as usize;
        let target = runtime
            .api
            .invoke(projectile, "GetTarget", &mut [])
            .map_err(|error| error.to_string())? as usize;
        state.traces.push(NativeTrace::ProjectileReleased {
            projectile_id: id,
            owner,
            target,
        });
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        state.fail(format!("projectile release trace failed: {error}"));
    }
}

fn record_projectile_removal(controller: *mut Object, intercepted: bool) {
    let runtime = RUNTIME.load(Ordering::Acquire);
    if runtime.is_null() {
        return;
    }
    // SAFETY: runtime is boxed for the process lifetime.
    let runtime = unsafe { &*runtime };
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed || !state.in_update {
        return;
    }
    let result = (|| {
        let projectile = invoke_object(runtime.api, controller, "GetFightProjectile")?;
        let pointer = projectile as usize;
        let id = match state.projectile_ids.get(&pointer) {
            Some(id) => *id,
            None => allocate(&mut state.next_projectile_id, "projectile")?,
        };
        state.projectile_ids.entry(pointer).or_insert(id);
        let owner = runtime
            .api
            .invoke(projectile, "GetOwner", &mut [])
            .map_err(|error| error.to_string())? as usize;
        let target = runtime
            .api
            .invoke(projectile, "GetTarget", &mut [])
            .map_err(|error| error.to_string())? as usize;
        let transform = invoke_object(runtime.api, projectile, "GetFightTransform")?;
        let position = vec3(invoke_value::<FixedVec3>(
            runtime.api,
            transform,
            "GetPositionInt3D",
        )?)?;
        state.traces.push(NativeTrace::ProjectileRemoved {
            projectile_id: id,
            owner,
            target,
            position,
            intercepted,
        });
        state.projectile_ids.remove(&pointer);
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        state.fail(format!("projectile removal trace failed: {error}"));
    }
}

fn record_damage(provider: *mut Object, target: *mut Object, result: i32) {
    let mut state = capture_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.armed || !state.in_update || result <= 0 {
        return;
    }
    let source = object_ref_from_pointer(provider as usize, &state);
    state.traces.push(NativeTrace::Damage {
        source,
        target: target as usize,
        amount: i64::from(result),
    });
}

fn durable_context(runtime: &Runtime) -> Result<DurableContext, String> {
    let current_match = runtime.current_match();
    let round = runtime
        .api
        .invoke_value::<i32>(current_match, "get_RoundCount", &mut [])
        .map_err(|error| error.to_string())?;
    if round <= 0 {
        return Err(format!("invalid combat round {round}"));
    }
    let application = runtime
        .api
        .class("UnityEngine.CoreModule.dll", "UnityEngine", "Application")
        .map_err(|error| error.to_string())?;
    let version = runtime
        .api
        .invoke_static(application, "get_version", &mut [])
        .and_then(|value| runtime.api.string_to_rust(value.cast()))
        .map_err(|error| error.to_string())?;
    let random = invoke_object(runtime.api, current_match, "GetRandom")?;
    let match_seed = invoke_value::<i32>(runtime.api, random, "GetSeed")?;
    Ok(DurableContext {
        schema_version: MCFR_SCHEMA_VERSION,
        game_build: version,
        logic_step: Rational {
            numerator: 1,
            denominator: 20,
        },
        numeric_convention: NumericConvention {
            distance_units_per_meter: DISTANCE_UNITS_PER_METER,
            rotation_units_per_degree: ROTATION_UNITS_PER_DEGREE,
            time_units_per_second: TIME_UNITS_PER_SECOND,
        },
        combat_round: u32::try_from(round).map_err(|_| "combat round overflow".to_owned())?,
        match_seed,
        identity_contract: IdentityContract::TeamZxSequentialV1,
    })
}

struct RawUnit {
    pointer: usize,
    formation: usize,
    state: UnitState,
    statuses: Vec<RawStatus>,
}

struct RawStatus {
    pointer: usize,
    source: usize,
    target: usize,
    status_type_id: u32,
    additive_stack: i32,
    duration_time: i32,
    max_duration_time: i32,
    step_time: i32,
    step_time_config: i32,
    finished: bool,
    frozen: bool,
}

struct RawBuilding {
    pointer: usize,
    native_index: i32,
    state: BuildingState,
}

#[allow(clippy::too_many_lines)]
fn snapshot(
    runtime: &Runtime,
    capture: &mut CaptureState,
    initial: bool,
) -> Result<WorldSnapshot, String> {
    let fight = runtime.current_fight();
    if fight.is_null() {
        return Err("fight controller disappeared during capture".into());
    }
    let tick_before = runtime
        .api
        .invoke_value::<i32>(fight, "get_Tick", &mut [])
        .map_err(|error| error.to_string())?;
    let teams = runtime
        .api
        .invoke(fight, "GetTeamControllers", &mut [])
        .map_err(|error| error.to_string())?;
    let team_count = list_count(runtime.api, teams, 32)?;
    let mut raw_units = Vec::new();
    let mut raw_buildings = Vec::new();
    for team_offset in 0..team_count {
        let controller = list_item(runtime.api, teams, team_offset)?;
        let team_index = runtime
            .api
            .invoke_value::<i32>(controller, "GetTeamIndex", &mut [])
            .map_err(|error| error.to_string())?;
        let team_id = u32::try_from(team_index)
            .map_err(|_| format!("invalid native team index {team_index}"))?;
        let team = runtime
            .api
            .invoke(controller, "GetTeam", &mut [])
            .map_err(|error| error.to_string())?;
        let units = runtime
            .api
            .invoke(team, "GetMeches", &mut [])
            .map_err(|error| error.to_string())?;
        for index in 0..list_count(runtime.api, units, 100_000)? {
            let unit = list_item(runtime.api, units, index)?;
            raw_units.push(read_unit(runtime.api, unit, team_id, &capture.metadata)?);
        }
        let buildings = runtime
            .api
            .invoke(team, "GetTowers", &mut [])
            .map_err(|error| error.to_string())?;
        for index in 0..list_count(runtime.api, buildings, 64)? {
            let building = list_item(runtime.api, buildings, index)?;
            raw_buildings.push(read_building(runtime.api, building, team_id)?);
        }
    }
    raw_units.sort_by_key(|unit| {
        (
            unit.state.team_id,
            unit.state.position.z,
            unit.state.position.x,
            unit.pointer,
        )
    });
    if initial {
        for pair in raw_units.windows(2) {
            if pair[0].state.team_id == pair[1].state.team_id
                && pair[0].state.position.x == pair[1].state.position.x
                && pair[0].state.position.z == pair[1].state.position.z
            {
                return Err("two initial same-team units have equal world coordinates".into());
            }
        }
    }
    let mut units = Vec::with_capacity(raw_units.len());
    let mut raw_statuses = Vec::new();
    for mut unit in raw_units {
        let unit_id = match capture.unit_ids.get(&unit.pointer) {
            Some(id) => *id,
            None => allocate(&mut capture.next_unit_id, "unit")?,
        };
        capture.unit_ids.entry(unit.pointer).or_insert(unit_id);
        let formation_key = if unit.formation == 0 {
            unit.pointer
        } else {
            unit.formation
        };
        let formation_id = match capture.formation_ids.get(&formation_key) {
            Some(id) => *id,
            None => allocate(&mut capture.next_formation_id, "formation")?,
        };
        capture
            .formation_ids
            .entry(formation_key)
            .or_insert(formation_id);
        unit.state.unit_id = unit_id;
        unit.state.formation_id = formation_id;
        raw_statuses.append(&mut unit.statuses);
        units.push(unit.state);
    }

    raw_buildings.sort_by_key(|building| (building.state.team_id, building.native_index));
    let mut buildings = Vec::with_capacity(raw_buildings.len());
    for mut building in raw_buildings {
        let id = match capture.building_ids.get(&building.pointer) {
            Some(id) => *id,
            None => allocate(&mut capture.next_building_id, "building")?,
        };
        capture.building_ids.entry(building.pointer).or_insert(id);
        building.state.building_id = id;
        buildings.push(building.state);
    }
    let projectiles = read_projectiles(runtime, capture)?;
    let mut seen_statuses = BTreeSet::new();
    let mut statuses = Vec::with_capacity(raw_statuses.len());
    for status in raw_statuses {
        seen_statuses.insert(status.pointer);
        let status_id = match capture.status_ids.get(&status.pointer) {
            Some(id) => *id,
            None => allocate(&mut capture.next_status_id, "status")?,
        };
        capture
            .status_ids
            .entry(status.pointer)
            .or_insert(status_id);
        let Some(target) = object_ref_from_pointer(status.target, capture) else {
            continue;
        };
        statuses.push(StatusState {
            status_id,
            status_type_id: status.status_type_id,
            source: object_ref_from_pointer(status.source, capture),
            target,
            additive_stack: status.additive_stack,
            duration_time: status.duration_time,
            max_duration_time: status.max_duration_time,
            step_time: status.step_time,
            step_time_config: status.step_time_config,
            finished: status.finished,
            frozen: status.frozen,
        });
    }
    capture
        .status_ids
        .retain(|pointer, _| seen_statuses.contains(pointer));
    let tick_after = runtime
        .api
        .invoke_value::<i32>(fight, "get_Tick", &mut [])
        .map_err(|error| error.to_string())?;
    if tick_before != tick_after {
        return Err(format!(
            "logic tick changed during snapshot ({tick_before} -> {tick_after})"
        ));
    }
    Ok(WorldSnapshot {
        units,
        projectiles,
        buildings,
        statuses,
    })
}

#[allow(clippy::too_many_lines)]
fn read_unit(
    api: Api,
    unit: *mut Object,
    team_id: u32,
    metadata: &Metadata,
) -> Result<RawUnit, String> {
    if unit.is_null() {
        return Err("team contains a null unit".into());
    }
    let transform = invoke_object(api, unit, "GetFightTransform")?;
    let fixed_position = invoke_value::<FixedVec3>(api, transform, "GetPositionInt3D")?;
    let fixed_rotation = invoke_value::<FixedPoint>(api, transform, "GetRotationInt")?;
    let motion = invoke_object(api, unit, "GetMotionController")?;
    let velocity = invoke_value::<FixedVec3>(api, motion, "GetCurrentVelocity")?;
    let alive = invoke_value::<bool>(api, unit, "IsAlive")?;
    let active = invoke_value::<bool>(api, unit, "get_IsActive")?;
    let fsm: *mut Object = api
        .field_value(motion, metadata.motion_fsm as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let current_motion_state = invoke_object(api, fsm, "GetCurrentState")?;
    let current_motion_class = api
        .object_class(current_motion_state)
        .ok_or_else(|| "native MotionFSM current state has no class".to_owned())?
        as usize;
    let motion_state = if current_motion_class == metadata.motion_idle_state_class {
        MotionState::Idle
    } else if current_motion_class == metadata.motion_move_state_class {
        MotionState::Moving
    } else if current_motion_class == metadata.motion_attack_state_class {
        MotionState::Attacking
    } else if current_motion_class == metadata.motion_stop_state_class {
        MotionState::Stopped
    } else {
        return Err("unsupported native MotionFSM current state".into());
    };
    let mut visibility = 0_i32;
    let targetable = api
        .invoke_value::<bool>(unit, "IsValidTarget", &mut [argument(&mut visibility)])
        .map_err(|error| error.to_string())?;
    let visibility = match invoke_value::<i32>(api, unit, "GetVisibility")? {
        0 => Visibility::Normal,
        1 => Visibility::Disappear,
        2 => Visibility::Stealth,
        3 => Visibility::Hide,
        value => return Err(format!("unsupported native visibility {value}")),
    };
    let position = vec3(fixed_position)?;
    let body_rotation = q32_to_units(fixed_rotation.raw, ROTATION_UNITS_PER_DEGREE)?;
    let main_skill = invoke_object(api, unit, "GetMainSkill")?;
    let aim_transform = invoke_object(api, main_skill, "GetMainTransform")?;
    let aim_position = vec3(invoke_value::<FixedVec3>(
        api,
        aim_transform,
        "GetPositionInt3D",
    )?)?;
    let aim_rotation = q32_to_units(
        invoke_value::<FixedPoint>(api, aim_transform, "GetRotationInt")?.raw,
        ROTATION_UNITS_PER_DEGREE,
    )?;
    let aim_pose = Pose {
        position: aim_position,
        rotation: aim_rotation,
    };
    let shield = invoke_object(api, unit, "GetEnergyShieldController")?;
    let max_energy = invoke_value::<i32>(api, shield, "GetMaxEnergy")?;
    let personal_shield = PersonalShieldState {
        active: invoke_value::<bool>(api, shield, "IsActive")?,
        enabled: invoke_value::<bool>(api, shield, "IsEnable")?,
        energy: i64::from(invoke_value::<i32>(api, shield, "GetEnergy")?),
        max_energy: i64::from(max_energy),
    };
    let buff_manager = invoke_object(api, unit, "GetBuffManager")?;
    let buffs: *mut Object = api
        .field_value(buff_manager, metadata.buff_list as *mut FieldInfo)
        .map_err(|error| error.to_string())?;
    let mut statuses = Vec::new();
    for index in 0..list_count(api, buffs, 1_024)? {
        let buff = list_item(api, buffs, index)?;
        let status_type = invoke_value::<i32>(api, buff, "GetBuffID")?;
        let source = api
            .invoke(buff, "GetSource", &mut [])
            .map_err(|error| error.to_string())?;
        statuses.push(RawStatus {
            pointer: buff as usize,
            source: source as usize,
            target: unit as usize,
            status_type_id: u32::try_from(status_type)
                .map_err(|_| format!("invalid buff type {status_type}"))?,
            additive_stack: invoke_value::<i32>(api, buff, "GetAdditiveStack")?,
            duration_time: api
                .field_value(buff, metadata.buff_duration_time as *mut FieldInfo)
                .map_err(|error| error.to_string())?,
            max_duration_time: api
                .field_value(buff, metadata.buff_max_duration_time as *mut FieldInfo)
                .map_err(|error| error.to_string())?,
            step_time: api
                .field_value(buff, metadata.buff_step_time as *mut FieldInfo)
                .map_err(|error| error.to_string())?,
            step_time_config: api
                .field_value(buff, metadata.buff_step_time_config as *mut FieldInfo)
                .map_err(|error| error.to_string())?,
            finished: invoke_value::<bool>(api, buff, "IsFinish")?,
            frozen: invoke_value::<bool>(api, buff, "IsFreeze")?,
        });
    }
    let formation = api
        .invoke(unit, "GetMechTeam", &mut [])
        .map_err(|error| error.to_string())?;
    let unit_type = invoke_value::<i32>(api, unit, "GetMechID")?;
    let life = invoke_value::<i32>(api, unit, "GetLife")?;
    let max_life = invoke_value::<i32>(api, unit, "GetMaxLife")?;
    Ok(RawUnit {
        pointer: unit as usize,
        formation: formation as usize,
        state: UnitState {
            unit_id: 0,
            team_id,
            formation_id: 0,
            unit_type_id: u32::try_from(unit_type)
                .map_err(|_| format!("invalid unit type {unit_type}"))?,
            domain: if invoke_value::<bool>(api, unit, "IsFly")? {
                Domain::Air
            } else {
                Domain::Ground
            },
            position,
            body_rotation,
            aim_pose,
            velocity: vec3(velocity)?,
            motion_state,
            collision_radius: q32_to_units(
                invoke_value::<FixedPoint>(api, unit, "GetRadius")?.raw,
                DISTANCE_UNITS_PER_METER,
            )?,
            life: i64::from(life),
            max_life: i64::from(max_life),
            alive,
            active,
            targetable,
            visibility,
            personal_shield,
        },
        statuses,
    })
}

fn read_building(api: Api, building: *mut Object, team_id: u32) -> Result<RawBuilding, String> {
    let transform = invoke_object(api, building, "GetFightTransform")?;
    let position = vec3(invoke_value::<FixedVec3>(
        api,
        transform,
        "GetPositionInt3D",
    )?)?;
    let rotation = q32_to_units(
        invoke_value::<FixedPoint>(api, transform, "GetRotationInt")?.raw,
        ROTATION_UNITS_PER_DEGREE,
    )?;
    let bounds = invoke_value::<FixedRect>(api, building, "GetBoundsRect")?;
    let native_index = invoke_value::<i32>(api, building, "GetBuildingIndex")?;
    let building_type = invoke_value::<i32>(api, building, "GetBuildingType")?;
    let life = invoke_value::<i32>(api, building, "GetLife")?;
    let max_life = invoke_value::<i32>(api, building, "GetMaxLife")?;
    let alive = invoke_value::<bool>(api, building, "IsAlive")?;
    let destroyed = invoke_value::<bool>(api, building, "IsDestroyed")?;
    let available = invoke_value::<bool>(api, building, "IsAvaliable")?;
    let mut visibility = 0_i32;
    let targetable = api
        .invoke_value::<bool>(building, "IsValidTarget", &mut [argument(&mut visibility)])
        .map_err(|error| error.to_string())?;
    let data = invoke_object(api, building, "GetBuildingData")?;
    let collision_enabled = invoke_value::<bool>(api, data, "get_EnableCollision")?;
    Ok(RawBuilding {
        pointer: building as usize,
        native_index,
        state: BuildingState {
            building_id: 0,
            team_id,
            building_type_id: u32::try_from(building_type)
                .map_err(|_| format!("invalid building type {building_type}"))?,
            position,
            rotation,
            bounds_width: q32_to_units(bounds.size.x.raw, DISTANCE_UNITS_PER_METER)?,
            bounds_height: q32_to_units(bounds.size.y.raw, DISTANCE_UNITS_PER_METER)?,
            life: i64::from(life),
            max_life: i64::from(max_life),
            alive,
            destroyed,
            available,
            targetable,
            collision_enabled,
        },
    })
}

fn read_projectiles(
    runtime: &Runtime,
    capture: &mut CaptureState,
) -> Result<Vec<ProjectileState>, String> {
    let fight = runtime.current_fight();
    let modules = runtime
        .api
        .invoke(fight, "GetModules", &mut [])
        .map_err(|error| error.to_string())?;
    let mut system = ptr::null_mut();
    for index in 0..list_count(runtime.api, modules, 128)? {
        let candidate = list_item(runtime.api, modules, index)?;
        if runtime
            .api
            .object_class(candidate)
            .map(|class| class as usize)
            == Some(capture.metadata.projectile_system_class)
        {
            system = candidate;
            break;
        }
    }
    if system.is_null() {
        return Err("ProjectileSystem module is unavailable".into());
    }
    let controllers: *mut Object = runtime
        .api
        .field_value(
            system,
            capture.metadata.projectile_controllers as *mut FieldInfo,
        )
        .map_err(|error| error.to_string())?;
    let mut seen = BTreeSet::new();
    let mut projectiles = Vec::new();
    for index in 0..list_count(runtime.api, controllers, 100_000)? {
        let controller = list_item(runtime.api, controllers, index)?;
        let projectile = invoke_object(runtime.api, controller, "GetFightProjectile")?;
        let pointer = projectile as usize;
        seen.insert(pointer);
        let id = match capture.projectile_ids.get(&pointer) {
            Some(id) => *id,
            None => allocate(&mut capture.next_projectile_id, "projectile")?,
        };
        capture.projectile_ids.entry(pointer).or_insert(id);
        projectiles.push(read_projectile(
            runtime.api,
            controller,
            projectile,
            id,
            capture,
        )?);
    }
    capture
        .projectile_ids
        .retain(|pointer, _| seen.contains(pointer));
    Ok(projectiles)
}

fn read_projectile(
    api: Api,
    controller: *mut Object,
    projectile: *mut Object,
    id: u64,
    capture: &CaptureState,
) -> Result<ProjectileState, String> {
    let owner = api
        .invoke(projectile, "GetOwner", &mut [])
        .map_err(|error| error.to_string())?;
    let target = api
        .invoke(projectile, "GetTarget", &mut [])
        .map_err(|error| error.to_string())?;
    let owner_ref = object_ref_from_pointer(owner as usize, capture);
    let target_ref = object_ref_from_pointer(target as usize, capture);
    let team_controller = invoke_object(api, controller, "GetTeamController")?;
    let team_index = invoke_value::<i32>(api, team_controller, "GetTeamIndex")?;
    let team_id = u32::try_from(team_index)
        .map_err(|_| format!("invalid projectile team index {team_index}"))?;
    let transform = invoke_object(api, projectile, "GetFightTransform")?;
    let position = vec3(invoke_value::<FixedVec3>(
        api,
        transform,
        "GetPositionInt3D",
    )?)?;
    let orientation = q32_to_units(
        invoke_value::<FixedPoint>(api, transform, "GetRotationInt")?.raw,
        ROTATION_UNITS_PER_DEGREE,
    )?;
    let target_info = invoke_object(api, projectile, "GetTargetInfo")?;
    let cached_target_position = vec3(invoke_value::<FixedVec3>(api, target_info, "GetPosition")?)?;
    let cached_target_radius = q32_to_units(
        invoke_value::<FixedPoint>(api, target_info, "GetRadius")?.raw,
        DISTANCE_UNITS_PER_METER,
    )?;
    let life = invoke_value::<i32>(api, projectile, "GetLife")?;
    let max_life = invoke_value::<i32>(api, projectile, "GetMaxLife")?;
    Ok(ProjectileState {
        projectile_id: id,
        team_id,
        owner: owner_ref,
        position,
        orientation,
        target: target_ref,
        cached_target_position,
        cached_target_radius,
        released: invoke_value::<bool>(api, projectile, "IsRelease")?,
        life: Gauge {
            current: i64::from(life),
            maximum: i64::from(max_life),
        },
    })
}

fn transition_events(traces: &[NativeTrace], capture: &CaptureState) -> TransitionEvents {
    let mut events = Vec::new();
    for trace in traces {
        match *trace {
            NativeTrace::ProjectileReleased {
                projectile_id,
                owner,
                target,
            } => {
                events.push(event(
                    Some(ObjectRef::new(ObjectKind::Projectile, projectile_id)),
                    object_ref_from_pointer(owner, capture),
                    object_ref_from_pointer(target, capture),
                    EventPayload::ProjectileReleased,
                ));
            }
            NativeTrace::ProjectileRemoved {
                projectile_id,
                owner,
                target,
                position,
                intercepted,
            } => {
                let subject = Some(ObjectRef::new(ObjectKind::Projectile, projectile_id));
                events.push(event(
                    subject,
                    object_ref_from_pointer(owner, capture),
                    object_ref_from_pointer(target, capture),
                    EventPayload::ProjectileRemoved {
                        position,
                        intercepted,
                    },
                ));
            }
            NativeTrace::Damage {
                source,
                target,
                amount,
            } => {
                let Some(target) = object_ref_from_pointer(target, capture) else {
                    continue;
                };
                events.push(event(
                    None,
                    source,
                    Some(target),
                    EventPayload::Damage { amount },
                ));
            }
        }
    }
    TransitionEvents { events }
}

fn object_ref_from_pointer(pointer: usize, capture: &CaptureState) -> Option<ObjectRef> {
    if pointer == 0 {
        return None;
    }
    capture
        .unit_ids
        .get(&pointer)
        .map(|id| ObjectRef::new(ObjectKind::Unit, *id))
        .or_else(|| {
            capture
                .building_ids
                .get(&pointer)
                .map(|id| ObjectRef::new(ObjectKind::Building, *id))
        })
        .or_else(|| {
            capture
                .projectile_ids
                .get(&pointer)
                .map(|id| ObjectRef::new(ObjectKind::Projectile, *id))
        })
}

const fn event(
    subject: Option<ObjectRef>,
    source: Option<ObjectRef>,
    target: Option<ObjectRef>,
    payload: EventPayload,
) -> Event {
    Event {
        subject,
        source,
        target,
        payload,
    }
}

fn list_count(api: Api, list: *mut Object, maximum: i32) -> Result<i32, String> {
    if list.is_null() {
        return Err("managed list is null".into());
    }
    let count = invoke_value::<i32>(api, list, "get_Count")?;
    if (0..=maximum).contains(&count) {
        Ok(count)
    } else {
        Err(format!("managed list count {count} exceeds {maximum}"))
    }
}

fn list_item(api: Api, list: *mut Object, mut index: i32) -> Result<*mut Object, String> {
    api.invoke(list, "get_Item", &mut [argument(&mut index)])
        .map_err(|error| error.to_string())
}

fn invoke_object(api: Api, object: *mut Object, method: &str) -> Result<*mut Object, String> {
    let value = api
        .invoke(object, method, &mut [])
        .map_err(|error| error.to_string())?;
    if value.is_null() {
        Err(format!("{method} returned null"))
    } else {
        Ok(value)
    }
}

fn invoke_value<T: Copy>(api: Api, object: *mut Object, method: &str) -> Result<T, String> {
    api.invoke_value(object, method, &mut [])
        .map_err(|error| error.to_string())
}

fn vec3(value: FixedVec3) -> Result<Vec3, String> {
    Ok(Vec3 {
        x: q32_to_units(value.x.raw, DISTANCE_UNITS_PER_METER)?,
        y: q32_to_units(value.y.raw, DISTANCE_UNITS_PER_METER)?,
        z: q32_to_units(value.z.raw, DISTANCE_UNITS_PER_METER)?,
    })
}

fn q32_to_units(raw: i64, scale: u64) -> Result<i64, String> {
    let scaled = i128::from(raw)
        .checked_mul(i128::from(scale))
        .ok_or_else(|| "fixed-point conversion overflow".to_owned())?;
    let rounded = if scaled >= 0 {
        (scaled + Q32_ONE / 2) / Q32_ONE
    } else {
        (scaled - Q32_ONE / 2) / Q32_ONE
    };
    i64::try_from(rounded).map_err(|_| "fixed-point conversion exceeds i64".into())
}

fn allocate(next: &mut u64, label: &str) -> Result<u64, String> {
    let id = *next;
    *next = next
        .checked_add(1)
        .ok_or_else(|| format!("{label} identity overflow"))?;
    Ok(id)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_update_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf4, 0x4f, 0xbe, 0xa9, 0xfd, 0x7b, 0x01, 0xa9, 0xfd, 0x43, 0x00, 0x91, 0xf3, 0x03, 0x00,
        0xaa,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        update_hook as *const c_void,
        &ORIGINAL_UPDATE,
        "FightController.Update",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_match_update_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf4, 0x4f, 0xbe, 0xa9, 0xfd, 0x7b, 0x01, 0xa9, 0xfd, 0x43, 0x00, 0x91, 0xf3, 0x03, 0x00,
        0xaa,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        match_update_hook as *const c_void,
        &ORIGINAL_MATCH_UPDATE,
        "MatchClient.Update",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_post_render_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf6, 0x57, 0xbd, 0xa9, 0xf4, 0x4f, 0x01, 0xa9, 0xfd, 0x7b, 0x02, 0xa9, 0xfd, 0x83, 0x00,
        0x91,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        post_render_hook as *const c_void,
        &ORIGINAL_POST_RENDER,
        "Camera.FireOnPostRender",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_projectile_add_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xf6, 0x57, 0xbd, 0xa9, 0xf4, 0x4f, 0x01, 0xa9, 0xfd, 0x7b, 0x02, 0xa9, 0xfd, 0x83, 0x00,
        0x91,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        projectile_add_hook as *const c_void,
        &ORIGINAL_PROJECTILE_ADD,
        "ProjectileSystem.AddProjectile",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_projectile_destroy_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xff, 0x43, 0x01, 0xd1, 0xf8, 0x5f, 0x01, 0xa9, 0xf6, 0x57, 0x02, 0xa9, 0xf4, 0x4f, 0x03,
        0xa9,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        projectile_destroy_hook as *const c_void,
        &ORIGINAL_PROJECTILE_DESTROY,
        "ProjectileSystem.Destroy",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_damage_perform_hook(api: Api, method: *const MethodInfo) -> Result<(), String> {
    const EXPECTED: [u8; 16] = [
        0xff, 0xc3, 0x04, 0xd1, 0xfc, 0x6f, 0x0d, 0xa9, 0xfa, 0x67, 0x0e, 0xa9, 0xf8, 0x5f, 0x0f,
        0xa9,
    ];
    install_inline_hook(
        api,
        method,
        &EXPECTED,
        damage_perform_hook as *const c_void,
        &ORIGINAL_DAMAGE_PERFORM,
        "DamagePerformer.Perform",
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_inline_hook(
    api: Api,
    method: *const MethodInfo,
    expected: &[u8; 16],
    replacement: *const c_void,
    original_slot: &AtomicPtr<c_void>,
    label: &str,
) -> Result<(), String> {
    let target = api
        .method_pointer(method)
        .map_err(|error| error.to_string())?;
    // SAFETY: target points to at least the generated method prologue.
    let actual = unsafe { std::slice::from_raw_parts(target.cast::<u8>(), expected.len()) };
    if actual != expected {
        return Err(format!("{label} prologue mismatch: {}", bytes_hex(actual)));
    }
    // SAFETY: anonymous mapping is checked before use.
    let trampoline = unsafe {
        libc::mmap(
            ptr::null_mut(),
            32,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };
    if trampoline == libc::MAP_FAILED {
        return Err(format!("cannot allocate {label} trampoline"));
    }
    // SAFETY: both source and destination are valid for the fixed lengths.
    unsafe { ptr::copy_nonoverlapping(target.cast::<u8>(), trampoline.cast::<u8>(), 16) };
    write_absolute_jump(unsafe { trampoline.cast::<u8>().add(16) }, unsafe {
        target.cast::<u8>().add(16).cast()
    });
    // SAFETY: trampoline is the mapping created above.
    if unsafe { libc::mprotect(trampoline, 32, libc::PROT_READ | libc::PROT_EXEC) } != 0 {
        // SAFETY: mapping belongs to this function after mprotect failure.
        unsafe { libc::munmap(trampoline, 32) };
        return Err(format!("cannot make {label} trampoline executable"));
    }
    let mut jump = [0_u8; 16];
    write_absolute_jump(jump.as_mut_ptr(), replacement);
    set_code_bytes(target, &jump)?;
    original_slot.store(trampoline, Ordering::Release);
    Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn bytes_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        },
    )
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn write_absolute_jump(destination: *mut u8, target: *const c_void) {
    let load_x16_pc_plus_8 = 0x5800_0050_u32;
    let branch_x16 = 0xd61f_0200_u32;
    // SAFETY: caller provides a writable 16-byte destination.
    unsafe {
        destination
            .cast::<u32>()
            .write_unaligned(load_x16_pc_plus_8);
        destination.add(4).cast::<u32>().write_unaligned(branch_x16);
        destination
            .add(8)
            .cast::<*const c_void>()
            .write_unaligned(target);
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn set_code_bytes(address: *mut c_void, bytes: &[u8]) -> Result<(), String> {
    const VM_PROT_READ: i32 = 1;
    const VM_PROT_WRITE: i32 = 2;
    const VM_PROT_EXECUTE: i32 = 4;
    const VM_PROT_COPY: i32 = 0x10;
    unsafe extern "C" {
        static mach_task_self_: u32;
        fn vm_protect(
            task: u32,
            address: usize,
            size: usize,
            set_maximum: bool,
            new_protection: i32,
        ) -> i32;
        fn sys_icache_invalidate(start: *mut c_void, length: usize);
    }
    // SAFETY: sysconf has no memory-safety preconditions.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return Err("cannot determine code page size".into());
    }
    let page_size = usize::try_from(page_size).map_err(|_| "invalid page size".to_owned())?;
    let begin = (address as usize) & !(page_size - 1);
    let end = (address as usize + bytes.len() + page_size - 1) & !(page_size - 1);
    // SAFETY: the page range contains the validated generated method target.
    let writable = unsafe {
        vm_protect(
            mach_task_self_,
            begin,
            end - begin,
            false,
            VM_PROT_READ | VM_PROT_WRITE | VM_PROT_COPY,
        )
    };
    if writable != 0 {
        return Err(format!("vm_protect writable failed with {writable}"));
    }
    // SAFETY: page is writable and destination covers bytes.len().
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), address.cast::<u8>(), bytes.len());
        sys_icache_invalidate(address, bytes.len());
        let _ = vm_protect(
            mach_task_self_,
            begin,
            end - begin,
            false,
            VM_PROT_READ | VM_PROT_EXECUTE,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(id: u64, team: u32, formation: u64) -> UnitState {
        UnitState {
            unit_id: id,
            team_id: team,
            formation_id: formation,
            unit_type_id: 1,
            domain: Domain::Ground,
            position: Vec3 {
                x: i64::from(team) * 1_000,
                y: 0,
                z: 0,
            },
            body_rotation: 0,
            aim_pose: Pose {
                position: Vec3 {
                    x: i64::from(team) * 1_000,
                    y: 0,
                    z: 0,
                },
                rotation: 0,
            },
            velocity: Vec3 { x: 0, y: 0, z: 0 },
            motion_state: MotionState::Idle,
            collision_radius: 100,
            life: 10,
            max_life: 10,
            alive: true,
            active: true,
            targetable: true,
            visibility: Visibility::Normal,
            personal_shield: PersonalShieldState {
                active: false,
                enabled: false,
                energy: 0,
                max_energy: 0,
            },
        }
    }

    #[test]
    fn q32_conversion_rounds_to_mcfr_scale() {
        assert_eq!(q32_to_units(1_i64 << 32, 1_000).unwrap(), 1_000);
        assert_eq!(q32_to_units(-(1_i64 << 32), 1_000).unwrap(), -1_000);
        assert_eq!(q32_to_units(1_i64 << 31, 1_000).unwrap(), 500);
    }

    #[test]
    fn same_tick_native_projectile_events_are_preserved_in_order() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("same-tick.mcfr");
        let state = WorldSnapshot {
            units: vec![unit(1, 0, 1), unit(2, 1, 2)],
            ..WorldSnapshot::default()
        };
        let mut capture = CaptureState::default();
        capture.unit_ids.insert(11, 1);
        capture.unit_ids.insert(22, 2);
        let traces = vec![
            NativeTrace::ProjectileReleased {
                projectile_id: 1,
                owner: 11,
                target: 22,
            },
            NativeTrace::Damage {
                source: Some(ObjectRef::new(ObjectKind::Projectile, 1)),
                target: 22,
                amount: 10,
            },
            NativeTrace::ProjectileRemoved {
                projectile_id: 1,
                owner: 11,
                target: 22,
                position: Vec3 { x: 1, y: 2, z: 0 },
                intercepted: false,
            },
        ];
        let events = transition_events(&traces, &capture);
        let context = DurableContext {
            schema_version: MCFR_SCHEMA_VERSION,
            game_build: "test".into(),
            logic_step: Rational {
                numerator: 1,
                denominator: 20,
            },
            numeric_convention: NumericConvention {
                distance_units_per_meter: 1_000,
                rotation_units_per_degree: 1_000,
                time_units_per_second: 2_000,
            },
            combat_round: 1,
            match_seed: 0,
            identity_contract: IdentityContract::TeamZxSequentialV1,
        };
        let mut writer = mechcore_mcfr::McfrWriter::create(&path, &context).unwrap();
        writer
            .append_tick(
                state.clone(),
                &mechcore_mcfr::TransitionEvents { events: Vec::new() },
            )
            .unwrap();
        writer.append_tick(state, &events).unwrap();
        writer.finish().unwrap();
        let reader = mechcore_mcfr::McfrReader::open_verified(path).unwrap();
        let events = reader.events(1).unwrap();
        assert_eq!(events.events.len(), 3);
        assert!(matches!(
            events.events[0].payload,
            EventPayload::ProjectileReleased
        ));
        assert_eq!(
            events.events[1].source,
            Some(ObjectRef::new(ObjectKind::Projectile, 1))
        );
        assert!(matches!(
            events.events[1].payload,
            EventPayload::Damage { amount: 10 }
        ));
        assert!(matches!(
            events.events[2].payload,
            EventPayload::ProjectileRemoved { .. }
        ));
    }
}
