use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const DEFAULT_UNITS: [&str; 2] = [
    include_str!("../../../config/units/marksman.yaml"),
    include_str!("../../../config/units/arclight.yaml"),
];
const DEFAULT_CONFIG: &str = include_str!("../../../config/config.yaml");

const SPACE_UNITS_PER_METER: f64 = 1_000.0;
const TIME_UNITS_PER_SECOND: f64 = 2_000.0;
const MILLIDEGREES_PER_DEGREE: f64 = 1_000.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnitConfig {
    schema: String,
    pub(crate) type_name: String,
    pub(crate) unit_type_id: u32,
    pub(crate) domain: UnitDomain,
    pub(crate) max_life: i64,
    pub(crate) collision_radius: f64,
    pub(crate) move_speed: f64,
    pub(crate) rotate_speed: f64,
    pub(crate) independent_aim: bool,
    pub(crate) attack: AttackConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UnitDomain {
    Ground,
    Air,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttackConfig {
    pub(crate) attack_type: AttackType,
    pub(crate) target_domain: TargetDomain,
    pub(crate) damage: i64,
    pub(crate) range: f64,
    pub(crate) interval: f64,
    pub(crate) interval_offset: f64,
    pub(crate) release_delay: f64,
    pub(crate) projectile_speed: f64,
    pub(crate) effect_radius: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AttackType {
    DirectProjectile,
    AreaProjectile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TargetDomain {
    Ground,
    Air,
    Both,
}

pub(crate) struct UnitConfigs {
    units: BTreeMap<String, UnitConfig>,
}

pub(crate) struct SimulationConfig {
    pub(crate) game_build: String,
    pub(crate) units: UnitConfigs,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TopLevelConfig {
    game_build: String,
}

impl SimulationConfig {
    pub(crate) fn load(directory: Option<&Path>) -> Result<Self> {
        let (top_level, unit_directory) = match directory {
            Some(directory) => {
                let path = directory.join("config.yaml");
                let bytes = fs::read(&path).map_err(|error| {
                    Error::new(format!("failed to read {}: {error}", path.display()))
                })?;
                (
                    parse_top_level(&bytes, &path.display().to_string())?,
                    Some(directory.join("units")),
                )
            }
            None => (
                parse_top_level(DEFAULT_CONFIG.as_bytes(), "embedded config")?,
                None,
            ),
        };
        if top_level.game_build.trim().is_empty() {
            return Err(Error::new("top-level config game_build must not be empty"));
        }
        Ok(Self {
            game_build: top_level.game_build,
            units: UnitConfigs::load(unit_directory.as_deref())?,
        })
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
            || self.attack.damage <= 0
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
        validate_scaled(self.attack.range, SPACE_UNITS_PER_METER, "range", true)?;
        validate_scaled(
            self.attack.interval,
            TIME_UNITS_PER_SECOND,
            "interval",
            false,
        )?;
        validate_scaled(
            self.attack.interval_offset,
            TIME_UNITS_PER_SECOND,
            "interval_offset",
            true,
        )?;
        validate_scaled(
            self.attack.release_delay,
            TIME_UNITS_PER_SECOND,
            "release_delay",
            true,
        )?;
        validate_scaled(
            self.attack.projectile_speed,
            SPACE_UNITS_PER_METER,
            "projectile_speed",
            false,
        )?;
        validate_scaled(
            self.attack.effect_radius,
            SPACE_UNITS_PER_METER,
            "effect_radius",
            true,
        )?;
        match self.attack.attack_type {
            AttackType::DirectProjectile if self.attack.effect_radius != 0.0 => Err(Error::new(
                "direct_projectile requires effect_radius to be zero",
            )),
            AttackType::AreaProjectile if self.attack.effect_radius == 0.0 => Err(Error::new(
                "area_projectile requires a positive effect_radius",
            )),
            _ => Ok(()),
        }
    }

    pub(crate) fn collision_radius(&self) -> i64 {
        quantize_i64(self.collision_radius, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn move_speed(&self) -> i64 {
        quantize_i64(self.move_speed, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn rotate_speed_mdeg_per_second(&self) -> i64 {
        quantize_i64(self.rotate_speed, MILLIDEGREES_PER_DEGREE)
    }
}

impl AttackConfig {
    pub(crate) fn range(&self) -> i64 {
        quantize_i64(self.range, SPACE_UNITS_PER_METER)
    }

    pub(crate) fn interval_time_units(&self) -> u64 {
        quantize_u64(self.interval, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn interval_offset_time_units(&self) -> u64 {
        quantize_u64(self.interval_offset, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn release_delay_time_units(&self) -> u64 {
        quantize_u64(self.release_delay, TIME_UNITS_PER_SECOND)
    }

    pub(crate) fn projectile_speed(&self) -> i64 {
        quantize_i64(self.projectile_speed, SPACE_UNITS_PER_METER)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn si_values_quantize_to_the_internal_integer_grid() {
        let config = SimulationConfig::load(None).unwrap();
        assert_eq!(config.game_build, "1.11.1.3.2259");
        let arclight = config.units.get("arclight").unwrap();
        assert_eq!(arclight.collision_radius(), 9_000);
        assert_eq!(arclight.move_speed(), 7_000);
        assert_eq!(arclight.attack.interval_time_units(), 1_800);
        assert_eq!(arclight.attack.interval_offset_time_units(), 600);
        assert!(!arclight.independent_aim);
        assert!(!config.units.get("marksman").unwrap().independent_aim);
    }
}
