use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const DEFAULT_UNITS: [&str; 23] = [
    include_str!("../../../config/units/marksman.yaml"),
    include_str!("../../../config/units/rhino.yaml"),
    include_str!("../../../config/units/wasp.yaml"),
    include_str!("../../../config/units/mustang.yaml"),
    include_str!("../../../config/units/steel_ball.yaml"),
    include_str!("../../../config/units/fang.yaml"),
    include_str!("../../../config/units/crawler.yaml"),
    include_str!("../../../config/units/stormcaller.yaml"),
    include_str!("../../../config/units/sledgehammer.yaml"),
    include_str!("../../../config/units/hacker.yaml"),
    include_str!("../../../config/units/arclight.yaml"),
    include_str!("../../../config/units/phoenix.yaml"),
    include_str!("../../../config/units/wraith.yaml"),
    include_str!("../../../config/units/scorpion.yaml"),
    include_str!("../../../config/units/fire_badger.yaml"),
    include_str!("../../../config/units/sabertooth.yaml"),
    include_str!("../../../config/units/typhoon.yaml"),
    include_str!("../../../config/units/tarantula.yaml"),
    include_str!("../../../config/units/phantom_ray.yaml"),
    include_str!("../../../config/units/farseer.yaml"),
    include_str!("../../../config/units/hound.yaml"),
    include_str!("../../../config/units/void_eye.yaml"),
    include_str!("../../../config/units/vortex.yaml"),
];
const DEFAULT_CONFIG: &str = include_str!("../../../config/config.yaml");
const DEFAULT_TRAINING_GROUND: &str = include_str!("../../../config/training_ground.yaml");
const CURRENT_KERNEL_SUPPORTED_UNIT_CONFIGS: [&str; 3] = [
    include_str!("../../../config/units/marksman.yaml"),
    include_str!("../../../config/units/arclight.yaml"),
    include_str!("../../../config/units/rhino.yaml"),
];

const SPACE_UNITS_PER_METER: f64 = 1_000.0;
const Q32_UNITS_PER_ONE: f64 = 4_294_967_296.0;
const TIME_UNITS_PER_SECOND: f64 = 2_000.0;
const MILLIDEGREES_PER_DEGREE: f64 = 1_000.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnitConfig {
    schema: String,
    pub(crate) type_name: String,
    pub(crate) unit_type_id: u32,
    pub(crate) formation: FormationConfig,
    pub(crate) domain: UnitDomain,
    pub(crate) max_life: i64,
    pub(crate) collision_radius: f64,
    pub(crate) move_speed: f64,
    pub(crate) rotate_speed: f64,
    pub(crate) has_body: bool,
    pub(crate) independent_aim: Option<bool>,
    pub(crate) rvo: RvoConfig,
    pub(crate) attack: AttackConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RvoConfig {
    pub(crate) outer_radius: f64,
    pub(crate) inner_radius: f64,
    pub(crate) size: RvoSize,
    pub(crate) collider_priority: i32,
    pub(crate) priority: f64,
}

impl RvoConfig {
    pub(crate) fn outer_radius(&self) -> i64 {
        quantize_i64(self.outer_radius, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn inner_radius(&self) -> i64 {
        quantize_i64(self.inner_radius, SPACE_UNITS_PER_METER)
    }

    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn priority_q32(&self) -> i64 {
        (self.priority * Q32_UNITS_PER_ONE).trunc() as i64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RvoSize {
    Xs,
    S,
    M,
    L,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FormationConfig {
    pub(crate) members: u32,
    pub(crate) slot_size: f64,
    pub(crate) footprint: FormationFootprint,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FormationFootprint {
    pub(crate) width: f64,
    pub(crate) depth: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UnitDomain {
    Ground,
    Air,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttackConfig {
    pub(crate) base_damage: i64,
    pub(crate) min_range: f64,
    pub(crate) range: f64,
    pub(crate) attack_half_angle: f64,
    pub(crate) targets: AttackTargets,
    pub(crate) lock_target: bool,
    pub(crate) quick_switch_target: bool,
    pub(crate) timing: AttackTiming,
    pub(crate) splash_radius: f64,
    pub(crate) weapons: WeaponTopology,
    pub(crate) path: AttackPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttackTargets {
    pub(crate) ground: bool,
    pub(crate) air: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttackTiming {
    pub(crate) interval: f64,
    pub(crate) interval_offset: f64,
    pub(crate) initial_cooldown: f64,
    pub(crate) prepare: f64,
    pub(crate) attack_point: f64,
    pub(crate) backswing: f64,
    pub(crate) cooling: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WeaponMode {
    Normal,
    Group,
    Standalone,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WeaponTopology {
    pub(crate) mode: WeaponMode,
    pub(crate) count: u32,
    pub(crate) per_skill: u32,
    pub(crate) fusillade: Option<bool>,
    pub(crate) allow_same_target: Option<bool>,
    pub(crate) rotation_speed: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum AttackPath {
    Projectile {
        count: u32,
        release_interval: f64,
        speed: f64,
        target_offset_radius: f64,
        evenly_allocate_targets: bool,
        extra_search_range: f64,
        pre_flight_height: f64,
        simulated_motion: bool,
        interceptible: bool,
        max_life: i64,
    },
    Direct {
        melee: bool,
    },
    Laser {
        damage_multipliers: Vec<f64>,
    },
    ControlBeam {
        warmup_attack_count: u32,
        warmup_damage_multiplier: f64,
    },
}

pub(crate) struct UnitConfigs {
    units: BTreeMap<String, UnitConfig>,
}

pub(crate) struct SimulationConfig {
    pub(crate) game_build: String,
    pub(crate) units: UnitConfigs,
    pub(crate) training_ground: TrainingGroundConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TopLevelConfig {
    game_build: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TrainingGroundConfig {
    schema: String,
    pub(crate) buildings: Vec<BuildingConfig>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BuildingConfig {
    pub(crate) team_id: u32,
    pub(crate) building_type_id: u32,
    pub(crate) position: BuildingPosition,
    pub(crate) life: i64,
    radius: f64,
    pub(crate) collision_enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BuildingPosition {
    x: f64,
    z: f64,
}

impl SimulationConfig {
    pub(crate) fn load(directory: Option<&Path>) -> Result<Self> {
        let (top_level, unit_directory, training_ground) = match directory {
            Some(directory) => {
                let path = directory.join("config.yaml");
                let bytes = fs::read(&path).map_err(|error| {
                    Error::new(format!("failed to read {}: {error}", path.display()))
                })?;
                let training_ground_path = directory.join("training_ground.yaml");
                let training_ground_bytes = fs::read(&training_ground_path).map_err(|error| {
                    Error::new(format!(
                        "failed to read {}: {error}",
                        training_ground_path.display()
                    ))
                })?;
                (
                    parse_top_level(&bytes, &path.display().to_string())?,
                    Some(directory.join("units")),
                    parse_training_ground(
                        &training_ground_bytes,
                        &training_ground_path.display().to_string(),
                    )?,
                )
            }
            None => (
                parse_top_level(DEFAULT_CONFIG.as_bytes(), "embedded config")?,
                None,
                parse_training_ground(
                    DEFAULT_TRAINING_GROUND.as_bytes(),
                    "embedded training-ground config",
                )?,
            ),
        };
        if top_level.game_build.trim().is_empty() {
            return Err(Error::new("top-level config game_build must not be empty"));
        }
        training_ground.validate()?;
        Ok(Self {
            game_build: top_level.game_build,
            units: UnitConfigs::load(unit_directory.as_deref())?,
            training_ground,
        })
    }
}

impl TrainingGroundConfig {
    fn validate(&self) -> Result<()> {
        if self.schema != "mechcore.training_ground" {
            return Err(Error::new("unsupported training-ground config type"));
        }
        if self.buildings.is_empty() {
            return Err(Error::new(
                "training-ground config must contain native building rows",
            ));
        }
        for building in &self.buildings {
            if building.team_id > 1 || building.building_type_id == 0 || building.life <= 0 {
                return Err(Error::new(
                    "training-ground building contains invalid identities or life",
                ));
            }
            validate_signed_scaled(building.position.x, SPACE_UNITS_PER_METER, "position.x")?;
            validate_signed_scaled(building.position.z, SPACE_UNITS_PER_METER, "position.z")?;
            validate_scaled(building.radius, SPACE_UNITS_PER_METER, "radius", false)?;
        }
        Ok(())
    }
}

impl BuildingConfig {
    pub(crate) fn x(&self) -> i64 {
        quantize_i64(self.position.x, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn z(&self) -> i64 {
        quantize_i64(self.position.z, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn radius(&self) -> i64 {
        quantize_i64(self.radius, SPACE_UNITS_PER_METER)
    }
}

impl UnitConfigs {
    pub(crate) fn load(directory: Option<&Path>) -> Result<Self> {
        let configs = match directory {
            Some(directory) => load_directory(directory)?,
            None => DEFAULT_UNITS
                .iter()
                .map(|text| parse(text.as_bytes(), "embedded unit config"))
                .collect::<Result<Vec<_>>>()?,
        };
        Self::from_configs(configs)
    }

    pub(crate) fn get(&self, type_name: &str) -> Option<&UnitConfig> {
        self.units.get(type_name)
    }

    fn from_configs(configs: Vec<UnitConfig>) -> Result<Self> {
        if configs.is_empty() {
            return Err(Error::new("unit config directory contains no YAML files"));
        }
        let mut ids = BTreeSet::new();
        let mut units = BTreeMap::new();
        for config in configs {
            config.validate()?;
            if !ids.insert(config.unit_type_id)
                || units.insert(config.type_name.clone(), config).is_some()
            {
                return Err(Error::new("unit configs contain duplicate identities"));
            }
        }
        Ok(Self { units })
    }
}

impl UnitConfig {
    fn validate(&self) -> Result<()> {
        if self.schema != "mechcore.unit" {
            return Err(Error::new("unsupported unit config type"));
        }
        if self.type_name.trim().is_empty()
            || self.unit_type_id == 0
            || self.max_life <= 0
            || self.formation.members == 0
            || self.attack.base_damage <= 0
        {
            return Err(Error::new(format!(
                "unit config for {:?} contains invalid values",
                self.type_name
            )));
        }
        validate_scaled(
            self.collision_radius,
            SPACE_UNITS_PER_METER,
            "collision_radius",
            true,
        )?;
        validate_scaled(self.move_speed, SPACE_UNITS_PER_METER, "move_speed", true)?;
        validate_scaled(
            self.rotate_speed,
            MILLIDEGREES_PER_DEGREE,
            "rotate_speed",
            true,
        )?;
        validate_scaled(
            self.formation.slot_size,
            SPACE_UNITS_PER_METER,
            "formation.slot_size",
            false,
        )?;
        let _ = self.formation_slot_size_meters()?;
        validate_scaled(
            self.formation.footprint.width,
            SPACE_UNITS_PER_METER,
            "formation.footprint.width",
            false,
        )?;
        validate_scaled(
            self.formation.footprint.depth,
            SPACE_UNITS_PER_METER,
            "formation.footprint.depth",
            false,
        )?;
        if !self.has_body && self.independent_aim.is_some() {
            return Err(Error::new(
                "independent_aim is not applicable when the unit has no mech body",
            ));
        }
        validate_scaled(
            self.rvo.outer_radius,
            SPACE_UNITS_PER_METER,
            "rvo.outer_radius",
            false,
        )?;
        validate_scaled(
            self.rvo.inner_radius,
            SPACE_UNITS_PER_METER,
            "rvo.inner_radius",
            false,
        )?;
        if self.rvo.inner_radius > self.rvo.outer_radius {
            return Err(Error::new(
                "rvo.inner_radius cannot exceed rvo.outer_radius",
            ));
        }
        if !(1..=10).contains(&self.rvo.collider_priority)
            || !(0.0..=1.0).contains(&self.rvo.priority)
        {
            return Err(Error::new(
                "rvo collider_priority or priority is outside its native range",
            ));
        }
        self.attack.validate()
    }

    pub(crate) fn ensure_current_kernel_support(&self) -> Result<()> {
        let reference = CURRENT_KERNEL_SUPPORTED_UNIT_CONFIGS
            .iter()
            .map(|text| parse(text.as_bytes(), "embedded closed unit config"))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .find(|config| config.unit_type_id == self.unit_type_id)
            .ok_or_else(|| {
                Error::new(format!(
                    "unit {:?} is not in the current kernel's supported behavior set",
                    self.type_name
                ))
            })?;
        let mut normalized = self.clone();
        normalized.type_name.clone_from(&reference.type_name);
        if normalized != reference {
            return Err(Error::new(format!(
                "unit {:?} differs from the behavior config supported by the current kernel",
                self.type_name
            )));
        }
        Ok(())
    }

    pub(crate) fn collision_radius(&self) -> i64 {
        quantize_i64(self.collision_radius, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn formation_footprint_meters(&self) -> Result<(i64, i64)> {
        fn whole_meters(value: f64, field: &str) -> Result<i64> {
            let raw = quantize_i64(value, SPACE_UNITS_PER_METER);
            let scale = SPACE_UNITS_PER_METER as i64;
            if raw.rem_euclid(scale) != 0 {
                return Err(Error::new(format!(
                    "unit config field {field} must use whole meters for layout placement"
                )));
            }
            Ok(raw / scale)
        }

        Ok((
            whole_meters(self.formation.footprint.width, "formation.footprint.width")?,
            whole_meters(self.formation.footprint.depth, "formation.footprint.depth")?,
        ))
    }

    pub(crate) fn formation_slot_size_meters(&self) -> Result<i64> {
        let raw = quantize_i64(self.formation.slot_size, SPACE_UNITS_PER_METER);
        let scale = SPACE_UNITS_PER_METER as i64;
        if raw.rem_euclid(scale) != 0 {
            return Err(Error::new(
                "unit config field formation.slot_size must use whole meters for member generation",
            ));
        }
        Ok(raw / scale)
    }

    pub(crate) fn move_speed(&self) -> i64 {
        quantize_i64(self.move_speed, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn rotate_speed_mdeg_per_second(&self) -> i64 {
        quantize_i64(self.rotate_speed, MILLIDEGREES_PER_DEGREE)
    }
}

impl AttackConfig {
    fn validate(&self) -> Result<()> {
        if !self.targets.ground && !self.targets.air {
            return Err(Error::new("attack must target ground, air, or both"));
        }
        validate_scaled(self.min_range, SPACE_UNITS_PER_METER, "min_range", true)?;
        validate_scaled(self.range, SPACE_UNITS_PER_METER, "range", true)?;
        if self.min_range > self.range {
            return Err(Error::new("attack min_range must not exceed range"));
        }
        validate_scaled(
            self.attack_half_angle,
            MILLIDEGREES_PER_DEGREE,
            "attack_half_angle",
            true,
        )?;
        if self.attack_half_angle > 360.0 {
            return Err(Error::new("attack_half_angle must not exceed 360 degrees"));
        }
        validate_scaled(
            self.timing.interval,
            TIME_UNITS_PER_SECOND,
            "timing.interval",
            false,
        )?;
        validate_scaled(
            self.timing.interval_offset,
            TIME_UNITS_PER_SECOND,
            "timing.interval_offset",
            true,
        )?;
        for (value, field) in [
            (self.timing.initial_cooldown, "timing.initial_cooldown"),
            (self.timing.prepare, "timing.prepare"),
            (self.timing.attack_point, "timing.attack_point"),
            (self.timing.backswing, "timing.backswing"),
            (self.timing.cooling, "timing.cooling"),
        ] {
            validate_scaled(value, TIME_UNITS_PER_SECOND, field, true)?;
        }
        validate_scaled(
            self.splash_radius,
            SPACE_UNITS_PER_METER,
            "splash_radius",
            true,
        )?;
        self.weapons.validate()?;
        match &self.path {
            AttackPath::Projectile {
                count,
                release_interval,
                speed,
                target_offset_radius,
                extra_search_range,
                pre_flight_height,
                interceptible,
                max_life,
                ..
            } => {
                if *count == 0 || *max_life < 0 || (*interceptible && *max_life == 0) {
                    return Err(Error::new(
                        "projectile path requires a count and valid projectile life",
                    ));
                }
                validate_scaled(
                    *release_interval,
                    TIME_UNITS_PER_SECOND,
                    "path.release_interval",
                    true,
                )?;
                validate_scaled(*speed, SPACE_UNITS_PER_METER, "path.speed", false)?;
                validate_scaled(
                    *target_offset_radius,
                    SPACE_UNITS_PER_METER,
                    "path.target_offset_radius",
                    true,
                )?;
                validate_scaled(
                    *extra_search_range,
                    SPACE_UNITS_PER_METER,
                    "path.extra_search_range",
                    true,
                )?;
                validate_scaled(
                    *pre_flight_height,
                    SPACE_UNITS_PER_METER,
                    "path.pre_flight_height",
                    true,
                )
            }
            AttackPath::Direct { .. } => Ok(()),
            AttackPath::Laser { damage_multipliers } => {
                if damage_multipliers.is_empty()
                    || damage_multipliers
                        .iter()
                        .any(|value| !value.is_finite() || *value <= 0.0)
                {
                    return Err(Error::new(
                        "laser path requires positive finite damage multipliers",
                    ));
                }
                Ok(())
            }
            AttackPath::ControlBeam {
                warmup_attack_count,
                warmup_damage_multiplier,
            } => {
                if *warmup_attack_count == 0
                    || !warmup_damage_multiplier.is_finite()
                    || *warmup_damage_multiplier <= 0.0
                {
                    return Err(Error::new(
                        "control beam path requires a positive warmup definition",
                    ));
                }
                Ok(())
            }
        }
    }

    pub(crate) fn min_range(&self) -> i64 {
        quantize_i64(self.min_range, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn range(&self) -> i64 {
        quantize_i64(self.range, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn attack_half_angle_mdeg(&self) -> i64 {
        quantize_i64(self.attack_half_angle, MILLIDEGREES_PER_DEGREE)
    }

    pub(crate) fn splash_radius(&self) -> i64 {
        quantize_i64(self.splash_radius, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn interval_time_units(&self) -> u64 {
        quantize_u64(self.timing.interval, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn interval_offset_time_units(&self) -> u64 {
        quantize_u64(self.timing.interval_offset, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn prepare_time_units(&self) -> u64 {
        quantize_u64(self.timing.prepare, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn attack_point_time_units(&self) -> u64 {
        quantize_u64(self.timing.attack_point, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn backswing_time_units(&self) -> u64 {
        quantize_u64(self.timing.backswing, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn projectile_speed(&self) -> i64 {
        let AttackPath::Projectile { speed, .. } = self.path else {
            unreachable!("the current kernel validates the projectile path")
        };
        quantize_i64(speed, SPACE_UNITS_PER_METER)
    }

    pub(crate) const fn accepts(&self, domain: UnitDomain) -> bool {
        match domain {
            UnitDomain::Ground => self.targets.ground,
            UnitDomain::Air => self.targets.air,
        }
    }
}

impl WeaponTopology {
    fn validate(&self) -> Result<()> {
        if self.count == 0 || self.per_skill == 0 || self.per_skill > self.count {
            return Err(Error::new("weapon topology contains invalid counts"));
        }
        let group_fields_are_complete =
            self.fusillade.is_some() && self.allow_same_target.is_some();
        let has_any_group_field = self.fusillade.is_some() || self.allow_same_target.is_some();
        if (self.mode == WeaponMode::Group && !group_fields_are_complete)
            || (self.mode != WeaponMode::Group && has_any_group_field)
        {
            return Err(Error::new(
                "group weapon topology requires fusillade and allow_same_target only for group mode",
            ));
        }
        if let Some(rotation_speed) = self.rotation_speed {
            validate_scaled(
                rotation_speed,
                MILLIDEGREES_PER_DEGREE,
                "weapons.rotation_speed",
                false,
            )?;
        }
        Ok(())
    }
}

fn validate_scaled(value: f64, scale: f64, field: &str, allow_zero: bool) -> Result<()> {
    let scaled = value * scale;
    if !value.is_finite()
        || value < 0.0
        || (!allow_zero && value == 0.0)
        || scaled > 9_000_000_000_000_000.0
        || (scaled - scaled.round()).abs() > 1.0e-9
    {
        return Err(Error::new(format!(
            "unit config field {field} cannot be represented exactly by the kernel"
        )));
    }
    Ok(())
}

fn validate_signed_scaled(value: f64, scale: f64, field: &str) -> Result<()> {
    let scaled = value * scale;
    if !value.is_finite()
        || scaled.abs() > 9_000_000_000_000_000.0
        || (scaled - scaled.round()).abs() > 1.0e-9
    {
        return Err(Error::new(format!(
            "training-ground config field {field} cannot be represented exactly by the kernel"
        )));
    }
    Ok(())
}

#[allow(clippy::cast_possible_truncation)]
fn quantize_i64(value: f64, scale: f64) -> i64 {
    (value * scale).round() as i64
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn quantize_u64(value: f64, scale: f64) -> u64 {
    (value * scale).round() as u64
}

fn load_directory(directory: &Path) -> Result<Vec<UnitConfig>> {
    let entries = fs::read_dir(directory).map_err(|error| {
        Error::new(format!(
            "failed to read unit config directory {}: {error}",
            directory.display()
        ))
    })?;
    let mut paths = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| Error::new(format!("failed to read unit config entry: {error}")))
        })
        .collect::<Result<Vec<_>>>()?;
    paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "yaml")
    });
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path).map_err(|error| {
                Error::new(format!("failed to read {}: {error}", path.display()))
            })?;
            let config = parse(&bytes, &path.display().to_string())?;
            if path.file_stem().and_then(|stem| stem.to_str()) != Some(&config.type_name) {
                return Err(Error::new(format!(
                    "unit config filename {} must match type_name {:?}",
                    path.display(),
                    config.type_name
                )));
            }
            Ok(config)
        })
        .collect()
}

fn parse(bytes: &[u8], source: &str) -> Result<UnitConfig> {
    serde_yaml::from_slice(bytes)
        .map_err(|error| Error::new(format!("invalid unit config {source}: {error}")))
}

fn parse_top_level(bytes: &[u8], source: &str) -> Result<TopLevelConfig> {
    serde_yaml::from_slice(bytes)
        .map_err(|error| Error::new(format!("invalid top-level config {source}: {error}")))
}

fn parse_training_ground(bytes: &[u8], source: &str) -> Result<TrainingGroundConfig> {
    serde_yaml::from_slice(bytes)
        .map_err(|error| Error::new(format!("invalid training-ground config {source}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn si_values_quantize_to_the_internal_integer_grid() {
        let config = SimulationConfig::load(None).unwrap();
        assert_eq!(config.game_build, "1.11.1.3.2259");
        assert_eq!(config.units.units.len(), 23);
        let arclight = config.units.get("arclight").unwrap();
        assert_eq!(arclight.collision_radius(), 9_000);
        assert_eq!(arclight.move_speed(), 7_000);
        assert_eq!(arclight.attack.interval_time_units(), 1_800);
        assert_eq!(arclight.attack.interval_offset_time_units(), 600);
        assert_eq!(arclight.independent_aim, Some(false));
        assert_eq!(
            config.units.get("marksman").unwrap().independent_aim,
            Some(false)
        );
        assert_eq!(config.training_ground.buildings.len(), 4);
        assert_eq!(config.training_ground.buildings[0].x(), -140_000);
        assert_eq!(config.training_ground.buildings[0].radius(), 10_000);
    }

    #[test]
    fn formation_slot_size_is_required_whole_meter_data() {
        let source = include_str!("../../../config/units/marksman.yaml");
        let missing = source.replace("  slot_size: 20\n", "");
        assert!(parse(missing.as_bytes(), "missing slot size").is_err());

        let fractional = source.replace("  slot_size: 20\n", "  slot_size: 20.5\n");
        let config = parse(fractional.as_bytes(), "fractional slot size").unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rvo_profile_is_required_and_inner_radius_cannot_exceed_outer_radius() {
        let source = include_str!("../../../config/units/marksman.yaml");
        let missing = source.replace(
            "rvo: {outer_radius: 8, inner_radius: 4, size: l, collider_priority: 6, priority: 0.006}\n",
            "",
        );
        assert!(parse(missing.as_bytes(), "missing RVO profile").is_err());

        let mut invalid = parse(source.as_bytes(), "invalid RVO radii").unwrap();
        invalid.rvo.inner_radius = invalid.rvo.outer_radius + 0.5;
        assert!(invalid.validate().is_err());

        let mut invalid = parse(source.as_bytes(), "invalid RVO priority").unwrap();
        invalid.rvo.collider_priority = 11;
        assert!(invalid.validate().is_err());
        invalid.rvo.collider_priority = 10;
        invalid.rvo.priority = 1.000_001;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn readable_rvo_priorities_preserve_native_q32_values() {
        let config = SimulationConfig::load(None).unwrap();
        let expected = [
            ("marksman", 25_769_803),
            ("rhino", 4_294_967_296),
            ("wasp", 17_179_869),
            ("mustang", 8_589_934),
            ("steel_ball", 21_474_836),
            ("fang", 4_294_967),
            ("crawler", 8_589_934),
            ("stormcaller", 17_179_869),
            ("sledgehammer", 858_993_459),
            ("hacker", 30_064_771),
            ("arclight", 2_147_483_648),
            ("phoenix", 25_769_803),
            ("wraith", 3_435_973),
            ("scorpion", 34_359_738),
            ("fire_badger", 1_717_986_918),
            ("sabertooth", 1_288_490_188),
            ("typhoon", 429_496_729),
            ("tarantula", 2_147_483_648),
            ("phantom_ray", 25_769_803),
            ("farseer", 858_993_459),
            ("hound", 21_474_836),
            ("void_eye", 21_474_836),
            ("vortex", 2_147_483_648),
        ];
        for (unit, priority_q32) in expected {
            assert_eq!(
                config.units.get(unit).unwrap().rvo.priority_q32(),
                priority_q32
            );
        }
    }

    #[test]
    fn p0_configs_preserve_path_and_topology_discriminants() {
        let config = SimulationConfig::load(None).unwrap();
        let rhino = config.units.get("rhino").unwrap();
        assert_eq!(rhino.formation.members, 1);
        assert_eq!(rhino.formation_slot_size_meters().unwrap(), 30);
        assert!(!rhino.has_body);
        assert_eq!(rhino.independent_aim, None);
        assert!(matches!(
            rhino.attack.path,
            AttackPath::Direct { melee: true }
        ));

        let wraith = config.units.get("wraith").unwrap();
        assert_eq!(wraith.attack.weapons.mode, WeaponMode::Group);
        assert_eq!(wraith.attack.weapons.count, 4);
        assert_eq!(wraith.attack.weapons.fusillade, Some(false));
        assert_eq!(wraith.attack.weapons.allow_same_target, Some(true));
        assert_eq!(wraith.attack.weapons.rotation_speed, Some(90.0));
        assert!(matches!(
            wraith.attack.path,
            AttackPath::Projectile { count: 1, .. }
        ));

        let vortex = config.units.get("vortex").unwrap();
        assert_eq!(vortex.attack.weapons.mode, WeaponMode::Group);
        assert_eq!(vortex.attack.weapons.fusillade, Some(true));
        assert_eq!(vortex.attack.weapons.allow_same_target, Some(false));
        assert!(matches!(
            vortex.attack.path,
            AttackPath::Direct { melee: false }
        ));

        let crawler = config.units.get("crawler").unwrap();
        assert_eq!(crawler.formation.members, 24);
        assert_eq!(crawler.formation_slot_size_meters().unwrap(), 6);
    }

    #[test]
    fn current_kernel_support_follows_the_explicit_config_set() {
        let config = SimulationConfig::load(None).unwrap();
        for (type_name, rules) in &config.units.units {
            let is_supported = matches!(type_name.as_str(), "marksman" | "arclight" | "rhino");
            assert_eq!(
                rules.ensure_current_kernel_support().is_ok(),
                is_supported,
                "{type_name} support must follow the explicit behavior set"
            );
        }

        let mut changed_marksman = config.units.get("marksman").unwrap().clone();
        changed_marksman.attack.splash_radius = 1.0;
        assert!(changed_marksman.ensure_current_kernel_support().is_err());
    }

    #[test]
    fn group_fields_are_all_or_nothing_and_group_only() {
        let config = SimulationConfig::load(None).unwrap();
        let mut partial_group = config.units.get("wraith").unwrap().clone();
        partial_group.attack.weapons.allow_same_target = None;
        assert!(partial_group.validate().is_err());

        let mut group_field_on_normal = config.units.get("marksman").unwrap().clone();
        group_field_on_normal.attack.weapons.fusillade = Some(false);
        assert!(group_field_on_normal.validate().is_err());
    }
}
