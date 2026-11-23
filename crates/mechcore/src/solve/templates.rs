//! Data-driven Round 1 macro layout templates and unit behavioral profiles.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use mechcore_document::{
    Position, Region,
    r#match::{SideState, StateUnit},
};
use serde::Deserialize;

use crate::cli::Failure;

use super::model::{Constraints, SpaceService, TemplateService};

const UNIT_PROFILES_SCHEMA: &str = "mechcore.unit_profiles/v1";
const R1_OPENING_SCHEMA: &str = "mechcore.r1_opening/v1";

/// Soft 4-axis tactical role weights summing to `1.0` for unit profiles.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoleWeights {
    pub(crate) chaff: f64,
    pub(crate) tank: f64,
    pub(crate) clear: f64,
    pub(crate) carry: f64,
}

impl RoleWeights {
    fn validate(&self, context: &str, require_simplex: bool) -> Result<(), Failure> {
        let values = [self.chaff, self.tank, self.clear, self.carry];
        if values.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return Err(Failure::refused(format!(
                "{context}: role weights must be finite and non-negative"
            )));
        }
        let sum: f64 = values.iter().sum();
        if require_simplex && (sum - 1.0).abs() > 1e-3 {
            return Err(Failure::refused(format!(
                "{context}: unit role weights must sum to 1.0 (got {sum:.4})"
            )));
        }
        Ok(())
    }

    fn dot(self, other: Self) -> f64 {
        self.chaff * other.chaff
            + self.tank * other.tank
            + self.clear * other.clear
            + self.carry * other.carry
    }

    fn priority_bucket(self) -> (usize, f64) {
        // Claim prime frontline tank/clear slots before carry and delayed chaff.
        let candidates = [
            (0_usize, self.tank),
            (1_usize, self.clear),
            (2_usize, self.carry),
            (3_usize, self.chaff),
        ];
        let mut best = candidates[0];
        for cand in candidates.into_iter().skip(1) {
            if cand.1 > best.1 {
                best = cand;
            }
        }
        best
    }

    fn is_chaff_dominant(self) -> bool {
        self.chaff > self.tank.max(self.clear).max(self.carry)
    }
}

/// Behavioral placement profile for a single unit type.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnitProfile {
    pub(crate) roles: RoleWeights,
    pub(crate) depth_offset: i32,
    pub(crate) rotate_prob: f64,
    pub(crate) allow_rotate: bool,
    pub(crate) edge_bias: f64,
    #[serde(default)]
    pub(crate) atk_embed: Vec<f64>,
    #[serde(default)]
    pub(crate) def_embed: Vec<f64>,
}

impl UnitProfile {
    fn validate(
        &self,
        name: &str,
        embed_dim: usize,
        allow_empty_embed: bool,
    ) -> Result<(), Failure> {
        self.roles
            .validate(&format!("unit profile {name:?}"), true)?;
        if !self.rotate_prob.is_finite() || !(0.0..=1.0).contains(&self.rotate_prob) {
            return Err(Failure::refused(format!(
                "unit profile {name:?}: rotate_prob must lie in [0.0, 1.0]"
            )));
        }
        if !self.edge_bias.is_finite() || !(-1.0..=1.0).contains(&self.edge_bias) {
            return Err(Failure::refused(format!(
                "unit profile {name:?}: edge_bias must lie in [-1.0, 1.0]"
            )));
        }
        let valid_len = |vec: &[f64]| {
            if embed_dim == 0 {
                vec.is_empty()
            } else {
                vec.len() == embed_dim || (allow_empty_embed && vec.is_empty())
            }
        };
        if !valid_len(&self.atk_embed) || !valid_len(&self.def_embed) {
            return Err(Failure::refused(format!(
                "unit profile {name:?}: embedding length must match embed_dim={embed_dim}"
            )));
        }
        if self
            .atk_embed
            .iter()
            .chain(&self.def_embed)
            .any(|v| !v.is_finite())
        {
            return Err(Failure::refused(format!(
                "unit profile {name:?}: embedding entries must be finite"
            )));
        }
        Ok(())
    }
}

/// Deserialized `profiles/data/unit_profiles.yaml` table.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnitProfilesFile {
    pub(crate) schema: String,
    #[allow(dead_code)]
    pub(crate) version: String,
    pub(crate) embed_dim: usize,
    pub(crate) default: UnitProfile,
    pub(crate) units: BTreeMap<String, UnitProfile>,
}

impl UnitProfilesFile {
    pub(crate) fn load(path: &Path) -> Result<Self, Failure> {
        let bytes = std::fs::read(path).map_err(|error| {
            Failure::failed(format!(
                "cannot read unit profiles {}: {error}",
                path.display()
            ))
        })?;
        let parsed: Self = serde_yaml::from_slice(&bytes).map_err(|error| {
            Failure::refused(format!("invalid unit profiles {}: {error}", path.display()))
        })?;
        if parsed.schema != UNIT_PROFILES_SCHEMA {
            return Err(Failure::refused(format!(
                "unsupported unit_profiles schema {:?}; expected {UNIT_PROFILES_SCHEMA:?}",
                parsed.schema
            )));
        }
        parsed.default.validate("default", parsed.embed_dim, true)?;
        for (name, profile) in &parsed.units {
            profile.validate(name, parsed.embed_dim, false)?;
        }
        Ok(parsed)
    }

    #[must_use]
    pub(crate) fn profile(&self, unit_type: &str) -> &UnitProfile {
        self.units.get(unit_type).unwrap_or(&self.default)
    }
}

/// Complete spatial jitter parameters used when sampling macro variants.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JitterConfig {
    pub(crate) shift_x: [i32; 2],
    pub(crate) shift_y: [i32; 2],
    pub(crate) layer_scale: [f64; 2],
    pub(crate) unit_jitter_x: i32,
    pub(crate) unit_jitter_y: i32,
    pub(crate) flip_rotate: bool,
}

impl JitterConfig {
    fn validate(&self, context: &str) -> Result<(), Failure> {
        validate_grid_range(context, "shift_x", self.shift_x)?;
        validate_grid_range(context, "shift_y", self.shift_y)?;
        if self.layer_scale[0] > self.layer_scale[1]
            || self.layer_scale.iter().any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(Failure::refused(format!(
                "{context}: layer_scale must be finite non-negative [min, max] with min <= max"
            )));
        }
        if self.unit_jitter_x < 0 || self.unit_jitter_x % 10 != 0 {
            return Err(Failure::refused(format!(
                "{context}: unit_jitter_x must be a non-negative multiple of 10m"
            )));
        }
        if self.unit_jitter_y < 0 || self.unit_jitter_y % 10 != 0 {
            return Err(Failure::refused(format!(
                "{context}: unit_jitter_y must be a non-negative multiple of 10m"
            )));
        }
        Ok(())
    }
}

fn validate_grid_range(context: &str, name: &str, range: [i32; 2]) -> Result<(), Failure> {
    if range[0] > range[1] {
        return Err(Failure::refused(format!(
            "{context}: {name} min ({}) exceeds max ({})",
            range[0], range[1]
        )));
    }
    if range[0] % 10 != 0 || range[1] % 10 != 0 {
        return Err(Failure::refused(format!(
            "{context}: {name} bounds {range:?} must be multiples of the 10m grid"
        )));
    }
    Ok(())
}

/// Optional per-plugin overrides merged on top of `r1_opening.yaml` defaults.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JitterOverride {
    #[serde(default)]
    pub(crate) shift_x: Option<[i32; 2]>,
    #[serde(default)]
    pub(crate) shift_y: Option<[i32; 2]>,
    #[serde(default)]
    pub(crate) layer_scale: Option<[f64; 2]>,
    #[serde(default)]
    pub(crate) unit_jitter_x: Option<i32>,
    #[serde(default)]
    pub(crate) unit_jitter_y: Option<i32>,
    #[serde(default)]
    pub(crate) flip_rotate: Option<bool>,
}

impl JitterOverride {
    pub(crate) fn merge(&self, base: &JitterConfig) -> Result<JitterConfig, Failure> {
        let merged = JitterConfig {
            shift_x: self.shift_x.unwrap_or(base.shift_x),
            shift_y: self.shift_y.unwrap_or(base.shift_y),
            layer_scale: self.layer_scale.unwrap_or(base.layer_scale),
            unit_jitter_x: self.unit_jitter_x.unwrap_or(base.unit_jitter_x),
            unit_jitter_y: self.unit_jitter_y.unwrap_or(base.unit_jitter_y),
            flip_rotate: self.flip_rotate.unwrap_or(base.flip_rotate),
        };
        merged.validate("jitter")?;
        Ok(merged)
    }
}

/// Biases asymmetric templates toward existing static constructions.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConstructionBiasConfig {
    pub(crate) enabled: bool,
    pub(crate) weight_boost: f64,
    pub(crate) anchor_pull: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChaffMode {
    Delayed,
    FrontScreen,
}

/// One spatial zone inside a macro opening template.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ZoneSpec {
    pub(crate) name: String,
    pub(crate) x_center: i32,
    pub(crate) x_span: i32,
    #[serde(default)]
    pub(crate) y_bias: i32,
    #[serde(default)]
    pub(crate) quota_ratio: Option<f64>,
    #[serde(default)]
    pub(crate) max_units: Option<usize>,
    pub(crate) role_affinity: RoleWeights,
}

/// One parameterized macro opening skeleton.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MacroTemplateSpec {
    pub(crate) id: String,
    pub(crate) prior: f64,
    #[serde(default)]
    pub(crate) mirror: bool,
    pub(crate) base_y: i32,
    pub(crate) chaff_mode: ChaffMode,
    pub(crate) zones: Vec<ZoneSpec>,
}

/// Deserialized `profiles/data/r1_opening.yaml` table.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct R1OpeningFile {
    pub(crate) schema: String,
    #[allow(dead_code)]
    pub(crate) version: String,
    pub(crate) default_jitter: JitterConfig,
    pub(crate) construction_bias: ConstructionBiasConfig,
    pub(crate) templates: Vec<MacroTemplateSpec>,
}

impl R1OpeningFile {
    pub(crate) fn load(path: &Path) -> Result<Self, Failure> {
        let bytes = std::fs::read(path).map_err(|error| {
            Failure::failed(format!(
                "cannot read r1 opening templates {}: {error}",
                path.display()
            ))
        })?;
        let parsed: Self = serde_yaml::from_slice(&bytes).map_err(|error| {
            Failure::refused(format!(
                "invalid r1 opening templates {}: {error}",
                path.display()
            ))
        })?;
        if parsed.schema != R1_OPENING_SCHEMA {
            return Err(Failure::refused(format!(
                "unsupported r1_opening schema {:?}; expected {R1_OPENING_SCHEMA:?}",
                parsed.schema
            )));
        }
        parsed.default_jitter.validate("default_jitter")?;
        if !parsed.construction_bias.weight_boost.is_finite()
            || parsed.construction_bias.weight_boost <= 0.0
            || !parsed.construction_bias.anchor_pull.is_finite()
            || !(0.0..=1.0).contains(&parsed.construction_bias.anchor_pull)
        {
            return Err(Failure::refused(
                "construction_bias requires positive weight_boost and anchor_pull in [0.0, 1.0]",
            ));
        }
        if parsed.templates.is_empty() {
            return Err(Failure::refused(
                "r1_opening requires at least one macro template",
            ));
        }
        let mut seen = BTreeSet::new();
        for tpl in &parsed.templates {
            if !seen.insert(tpl.id.as_str()) {
                return Err(Failure::refused(format!(
                    "duplicate macro template id {:?}",
                    tpl.id
                )));
            }
            if !tpl.prior.is_finite() || tpl.prior <= 0.0 {
                return Err(Failure::refused(format!(
                    "template {:?} requires finite positive prior",
                    tpl.id
                )));
            }
            if tpl.zones.is_empty() {
                return Err(Failure::refused(format!(
                    "template {:?} requires at least one zone",
                    tpl.id
                )));
            }
            for zone in &tpl.zones {
                zone.role_affinity.validate(
                    &format!("template {:?} zone {:?}", tpl.id, zone.name),
                    false,
                )?;
                if zone.x_span < 0 {
                    return Err(Failure::refused(format!(
                        "template {:?} zone {:?} requires non-negative x_span",
                        tpl.id, zone.name
                    )));
                }
                if let Some(ratio) = zone.quota_ratio
                    && (!ratio.is_finite() || ratio < 0.0)
                {
                    return Err(Failure::refused(format!(
                        "template {:?} zone {:?} requires finite non-negative quota_ratio",
                        tpl.id, zone.name
                    )));
                }
            }
        }
        Ok(parsed)
    }
}

/// Request passed to [`TemplateService::generate`].
pub(crate) struct TemplateRequest<'a> {
    pub(crate) round: i32,
    pub(crate) red: bool,
    pub(crate) state: &'a SideState,
    pub(crate) unpinned: &'a BTreeSet<i32>,
    pub(crate) constraints: &'a Constraints,
    pub(crate) templates: &'a [String],
    pub(crate) variants_per_template: usize,
    pub(crate) include_clean_base: bool,
    pub(crate) jitter: &'a JitterOverride,
    pub(crate) seed: u64,
}

/// One generated, collision-free candidate side state with normalized weight.
#[derive(Clone, Debug)]
pub(crate) struct GeneratedLayout {
    pub(crate) label: String,
    pub(crate) weight: f64,
    pub(crate) state: SideState,
}

/// Data-driven Round 1 macro-template layout generator.
pub(crate) struct R1MacroTemplateService {
    profiles: UnitProfilesFile,
    opening: R1OpeningFile,
}

impl R1MacroTemplateService {
    pub(crate) fn from_files(profiles_path: &Path, opening_path: &Path) -> Result<Self, Failure> {
        Ok(Self {
            profiles: UnitProfilesFile::load(profiles_path)?,
            opening: R1OpeningFile::load(opening_path)?,
        })
    }

    fn select_templates<'a>(
        &'a self,
        requested: &[String],
    ) -> Result<Vec<&'a MacroTemplateSpec>, Failure> {
        if requested.is_empty() {
            return Ok(self.opening.templates.iter().collect());
        }
        let mut selected = Vec::with_capacity(requested.len());
        for name in requested {
            let tpl = self
                .opening
                .templates
                .iter()
                .find(|entry| entry.id == *name)
                .ok_or_else(|| Failure::refused(format!("unknown macro template {name:?}")))?;
            selected.push(tpl);
        }
        Ok(selected)
    }
}

impl TemplateService for R1MacroTemplateService {
    fn validate_request(
        &self,
        templates: &[String],
        variants_per_template: usize,
        jitter: &JitterOverride,
    ) -> Result<(), Failure> {
        if variants_per_template == 0 {
            return Err(Failure::refused(
                "macro template generation requires at least 1 variant per template",
            ));
        }
        self.select_templates(templates)?;
        jitter.merge(&self.opening.default_jitter)?;
        Ok(())
    }

    fn generate(
        &self,
        space: &dyn SpaceService,
        request: &TemplateRequest<'_>,
    ) -> Result<Vec<GeneratedLayout>, Failure> {
        if request.variants_per_template == 0 {
            return Err(Failure::refused(
                "macro template generation requires variants_per_template >= 1",
            ));
        }
        let selected = self.select_templates(request.templates)?;
        let jitter = request.jitter.merge(&self.opening.default_jitter)?;
        let constructions = ConstructionStats::from_state(request.state);
        let oriented =
            expand_oriented_templates(&selected, &constructions, &self.opening.construction_bias);
        let sorted_units = sort_unpinned_units(request.state, request.unpinned, &self.profiles);

        if sorted_units.is_empty() {
            return Ok(Vec::new());
        }

        let mut rng = SplitMix64::new(request.seed);
        let mut generated = Vec::new();
        let count = oriented
            .len()
            .checked_mul(request.variants_per_template)
            .ok_or_else(|| Failure::refused("too many macro template variants"))?;
        generated
            .try_reserve(count)
            .map_err(|_| Failure::refused("too many macro template variants"))?;
        for tpl in oriented {
            let Some(zone_assignments) = assign_units_to_zones(&sorted_units, &tpl.spec.zones)
            else {
                continue;
            };
            let variant_weight = tpl.weight / usize_to_f64(request.variants_per_template);
            for variant_idx in 0..request.variants_per_template {
                let apply_jitter = !(request.include_clean_base && variant_idx == 0);
                let label = if request.variants_per_template == 1 {
                    tpl.label.clone()
                } else if !apply_jitter {
                    format!("{}#base", tpl.label)
                } else {
                    format!("{}#v{variant_idx}", tpl.label)
                };
                let targets = plan_zone_targets(
                    &tpl,
                    &zone_assignments,
                    &sorted_units,
                    &constructions,
                    &self.opening.construction_bias,
                    &jitter,
                    apply_jitter,
                    &mut rng,
                );
                if let Some(state) =
                    place_units_sequentially(space, request, &sorted_units, &targets)?
                {
                    generated.push(GeneratedLayout {
                        label,
                        weight: variant_weight,
                        state,
                    });
                }
            }
        }

        normalize_weights(&mut generated);
        Ok(generated)
    }
}

struct OrientedTemplate<'a> {
    label: String,
    spec: &'a MacroTemplateSpec,
    x_sign: i32,
    weight: f64,
}

struct ConstructionStats {
    left_xs: Vec<i32>,
    right_xs: Vec<i32>,
}

impl ConstructionStats {
    fn from_state(state: &SideState) -> Self {
        let mut left_xs = Vec::new();
        let mut right_xs = Vec::new();
        for item in &state.constructions {
            if Region::of(item.position) != Region::Main {
                continue;
            }
            if item.position.x < 0 {
                left_xs.push(item.position.x);
            } else if item.position.x > 0 {
                right_xs.push(item.position.x);
            }
        }
        Self { left_xs, right_xs }
    }

    fn pull_x(&self, x_center: i64, anchor_pull: f64) -> i64 {
        let side_xs = if x_center < 0 && !self.left_xs.is_empty() {
            &self.left_xs
        } else if x_center > 0 && !self.right_xs.is_empty() {
            &self.right_xs
        } else {
            return x_center;
        };
        let sum: i64 = side_xs.iter().map(|&x| i64::from(x)).sum();
        let mean_x = i64_to_f64(sum) / usize_to_f64(side_xs.len());
        let blended = (1.0 - anchor_pull) * i64_to_f64(x_center) + anchor_pull * mean_x;
        i64::from(round_to_grid_10(blended))
    }
}

fn expand_oriented_templates<'a>(
    selected: &[&'a MacroTemplateSpec],
    constructions: &ConstructionStats,
    bias: &ConstructionBiasConfig,
) -> Vec<OrientedTemplate<'a>> {
    let mut out = Vec::new();
    let boost_left = bias.enabled && constructions.left_xs.len() > constructions.right_xs.len();
    let boost_right = bias.enabled && constructions.right_xs.len() > constructions.left_xs.len();

    for &spec in selected {
        if spec.mirror {
            let left_w = spec.prior * 0.5 * if boost_left { bias.weight_boost } else { 1.0 };
            let right_w = spec.prior * 0.5 * if boost_right { bias.weight_boost } else { 1.0 };
            out.push(OrientedTemplate {
                label: format!("{}_left", spec.id),
                spec,
                x_sign: 1,
                weight: left_w,
            });
            out.push(OrientedTemplate {
                label: format!("{}_right", spec.id),
                spec,
                x_sign: -1,
                weight: right_w,
            });
        } else {
            out.push(OrientedTemplate {
                label: spec.id.clone(),
                spec,
                x_sign: 1,
                weight: spec.prior,
            });
        }
    }
    out
}

struct SortedUnit<'a> {
    unit: &'a StateUnit,
    profile: &'a UnitProfile,
    bucket: usize,
    dominant_score: f64,
}

fn sort_unpinned_units<'a>(
    state: &'a SideState,
    unpinned: &BTreeSet<i32>,
    profiles: &'a UnitProfilesFile,
) -> Vec<SortedUnit<'a>> {
    let mut list: Vec<SortedUnit<'a>> = state
        .active_units()
        .filter(|unit| unpinned.contains(&unit.index))
        .map(|unit| {
            let profile = profiles.profile(&unit.type_name);
            let (bucket, dominant_score) = profile.roles.priority_bucket();
            SortedUnit {
                unit,
                profile,
                bucket,
                dominant_score,
            }
        })
        .collect();
    list.sort_by(|a, b| {
        a.bucket
            .cmp(&b.bucket)
            .then_with(|| b.dominant_score.total_cmp(&a.dominant_score))
            .then_with(|| a.unit.type_name.cmp(&b.unit.type_name))
            .then_with(|| a.unit.index.cmp(&b.unit.index))
    });
    list
}

fn assign_units_to_zones(units: &[SortedUnit<'_>], zones: &[ZoneSpec]) -> Option<Vec<Vec<usize>>> {
    let mut assigned = vec![Vec::new(); zones.len()];
    let mut used = vec![false; units.len()];

    let open_zones: Vec<usize> = zones
        .iter()
        .enumerate()
        .filter_map(|(idx, zone)| zone.max_units.is_none().then_some(idx))
        .collect();
    let reserved = usize::from(!open_zones.is_empty());
    // Pass 1: fill specialized capped zones (such as decoy_flank).
    for (z_idx, zone) in zones.iter().enumerate() {
        let Some(cap) = zone.max_units else {
            continue;
        };
        let needs_chaff = zone.role_affinity.is_chaff_dominant();
        for _ in 0..cap {
            let remaining_now = used.iter().filter(|flag| !**flag).count();
            if remaining_now <= reserved {
                break;
            }
            let best = units
                .iter()
                .enumerate()
                .filter(|(u_idx, unit)| {
                    !used[*u_idx]
                        && (!needs_chaff || unit.profile.roles.chaff >= 0.4)
                        && zone.role_affinity.dot(unit.profile.roles) > 0.0
                })
                .max_by(|(ia, ua), (ib, ub)| {
                    let sa = zone.role_affinity.dot(ua.profile.roles);
                    let sb = zone.role_affinity.dot(ub.profile.roles);
                    sa.total_cmp(&sb).then_with(|| ib.cmp(ia))
                })
                .map(|(u_idx, _)| u_idx);
            let Some(pick) = best else {
                break;
            };
            used[pick] = true;
            assigned[z_idx].push(pick);
        }
    }

    // Pass 2: distribute remaining units across uncapped zones by affinity & quota.
    if open_zones.is_empty() {
        return used.iter().all(|&placed| placed).then_some(assigned);
    }

    let remaining_units: Vec<usize> = used
        .iter()
        .enumerate()
        .filter_map(|(idx, flag)| (!*flag).then_some(idx))
        .collect();
    let targets = apportion_quotas(remaining_units.len(), &open_zones, zones);

    for u_idx in remaining_units {
        let unit = &units[u_idx];
        let best_zone = open_zones
            .iter()
            .copied()
            .filter(|&z_idx| assigned[z_idx].len() < targets[z_idx])
            .max_by(|&za, &zb| {
                let sa = zones[za].role_affinity.dot(unit.profile.roles);
                let sb = zones[zb].role_affinity.dot(unit.profile.roles);
                let fill_a = usize_to_f64(assigned[za].len()) / usize_to_f64(targets[za].max(1));
                let fill_b = usize_to_f64(assigned[zb].len()) / usize_to_f64(targets[zb].max(1));
                sa.total_cmp(&sb)
                    .then_with(|| fill_b.total_cmp(&fill_a))
                    .then_with(|| zb.cmp(&za))
            })
            .unwrap_or(open_zones[0]);
        assigned[best_zone].push(u_idx);
    }

    Some(assigned)
}

fn apportion_quotas(total: usize, open_zones: &[usize], zones: &[ZoneSpec]) -> Vec<usize> {
    let mut targets = vec![0_usize; zones.len()];
    if total == 0 || open_zones.is_empty() {
        return targets;
    }
    let weights: Vec<f64> = open_zones
        .iter()
        .map(|&z| zones[z].quota_ratio.unwrap_or(1.0).max(0.0))
        .collect();
    let sum: f64 = weights.iter().sum();
    let mut allocated = 0_usize;
    let mut remainders = Vec::with_capacity(open_zones.len());
    for (idx, &z) in open_zones.iter().enumerate() {
        let exact = if sum > 0.0 {
            usize_to_f64(total) * weights[idx] / sum
        } else {
            usize_to_f64(total) / usize_to_f64(open_zones.len())
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let base = exact.floor().max(0.0) as usize;
        targets[z] = base;
        allocated += base;
        remainders.push((idx, z, exact - usize_to_f64(base)));
    }
    remainders.sort_by(|a, b| b.2.total_cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    for &(_, z, _) in remainders.iter().take(total.saturating_sub(allocated)) {
        targets[z] += 1;
    }
    targets
}

struct PlannedPose {
    target: Position,
    preferred_rot: bool,
}

#[allow(clippy::too_many_arguments)]
fn plan_zone_targets(
    tpl: &OrientedTemplate<'_>,
    zone_assignments: &[Vec<usize>],
    units: &[SortedUnit<'_>],
    constructions: &ConstructionStats,
    bias: &ConstructionBiasConfig,
    jitter: &JitterConfig,
    apply_jitter: bool,
    rng: &mut SplitMix64,
) -> Vec<Option<PlannedPose>> {
    let (dx, dy, layer_scale) = if apply_jitter {
        (
            rng.sample_step_10(jitter.shift_x[0], jitter.shift_x[1]),
            rng.sample_step_10(jitter.shift_y[0], jitter.shift_y[1]),
            rng.sample_f64(jitter.layer_scale[0], jitter.layer_scale[1]),
        )
    } else {
        (0, 0, 1.0)
    };

    let mut poses: Vec<Option<PlannedPose>> = (0..units.len()).map(|_| None).collect();
    for (z_idx, zone_units) in zone_assignments.iter().enumerate() {
        if zone_units.is_empty() {
            continue;
        }
        let zone = &tpl.spec.zones[z_idx];
        let mirrored_x = i64::from(zone.x_center) * i64::from(tpl.x_sign);
        let zone_x = if bias.enabled {
            constructions.pull_x(mirrored_x, bias.anchor_pull)
        } else {
            mirrored_x
        };

        let mut bands: BTreeMap<i32, Vec<(usize, i32, i32, bool)>> = BTreeMap::new();
        for &u_idx in zone_units {
            let profile = units[u_idx].profile;
            let depth_term =
                if tpl.spec.chaff_mode == ChaffMode::FrontScreen && profile.roles.chaff >= 0.5 {
                    25
                } else {
                    round_to_grid_10(f64::from(profile.depth_offset) * layer_scale)
                };
            let (offset_x, offset_y) = if apply_jitter {
                (
                    rng.sample_step_10(-jitter.unit_jitter_x, jitter.unit_jitter_x),
                    rng.sample_step_10(-jitter.unit_jitter_y, jitter.unit_jitter_y),
                )
            } else {
                (0, 0)
            };
            let target_y = [tpl.spec.base_y, zone.y_bias, depth_term, dy, offset_y]
                .into_iter()
                .map(i64::from)
                .sum::<i64>()
                .clamp(-300, -20);
            let target_y = i32::try_from(target_y).expect("main board coordinate");
            let preferred_rot = if !profile.allow_rotate {
                false
            } else if !apply_jitter || !jitter.flip_rotate {
                profile.rotate_prob >= 0.5
            } else {
                rng.next_f64() < profile.rotate_prob
            };
            let band_key = target_y.div_euclid(40);
            bands
                .entry(band_key)
                .or_default()
                .push((u_idx, target_y, offset_x, preferred_rot));
        }

        for band in bands.values_mut() {
            band.sort_by(|(ia, _, _, _), (ib, _, _, _)| {
                let pa = units[*ia].profile;
                let pb = units[*ib].profile;
                let edge_cmp = if zone_x <= 0 {
                    pb.edge_bias.total_cmp(&pa.edge_bias)
                } else {
                    pa.edge_bias.total_cmp(&pb.edge_bias)
                };
                edge_cmp
                    .then_with(|| units[*ia].unit.type_name.cmp(&units[*ib].unit.type_name))
                    .then_with(|| units[*ia].unit.index.cmp(&units[*ib].unit.index))
            });
            let count = band.len();
            for (slot_idx, &(u_idx, target_y, offset_x, preferred_rot)) in band.iter().enumerate() {
                let spread_offset = if count <= 1 {
                    0
                } else {
                    let frac = usize_to_f64(slot_idx) / usize_to_f64(count - 1) - 0.5;
                    round_to_grid_10(frac * f64::from(zone.x_span))
                };
                let target_x =
                    (zone_x + i64::from(spread_offset) + i64::from(dx) + i64::from(offset_x))
                        .clamp(-280, 280);
                poses[u_idx] = Some(PlannedPose {
                    target: Position {
                        x: i32::try_from(target_x).expect("main board coordinate"),
                        y: target_y,
                    },
                    preferred_rot,
                });
            }
        }
    }
    poses
}

fn place_units_sequentially(
    space: &dyn SpaceService,
    request: &TemplateRequest<'_>,
    units: &[SortedUnit<'_>],
    targets: &[Option<PlannedPose>],
) -> Result<Option<SideState>, Failure> {
    let mut working = request.state.clone();
    let mut remaining_unpinned = request.unpinned.clone();

    for (u_idx, sorted) in units.iter().enumerate() {
        let Some(planned) = &targets[u_idx] else {
            continue;
        };
        let unit_index = sorted.unit.index;
        let mask = space.move_mask_excluding(
            request.round,
            request.red,
            &working,
            unit_index,
            &remaining_unpinned,
        )?;
        let chosen = mask.nearest_in_region(
            "main",
            planned.target,
            planned.preferred_rot,
            sorted.profile.allow_rotate,
            |p| request.constraints.admits(p),
        );
        let Some((pos, rot)) = chosen else {
            return Ok(None);
        };
        if let Some(formation) = working
            .units
            .iter_mut()
            .find(|entry| !entry.recovered && entry.index == unit_index)
        {
            let left = formation.position.map(Region::of);
            let arrived = Region::of(pos);
            formation.position = Some(pos);
            formation.rotated = rot.then_some(true);
            if left != Some(arrived) {
                formation.travelling = arrived.is_flank().then_some(true);
            }
        }
        remaining_unpinned.remove(&unit_index);
    }

    Ok(Some(working))
}

fn normalize_weights(layouts: &mut [GeneratedLayout]) {
    let total: f64 = layouts.iter().map(|entry| entry.weight).sum();
    if total > 0.0 {
        for entry in layouts {
            entry.weight /= total;
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
fn round_to_grid_10(value: f64) -> i32 {
    let steps = (value / 10.0).round().clamp(-100.0, 100.0) as i32;
    steps * 10
}

#[allow(clippy::cast_precision_loss)]
const fn usize_to_f64(value: usize) -> f64 {
    value as f64
}

#[allow(clippy::cast_precision_loss)]
const fn i64_to_f64(value: i64) -> f64 {
    value as f64
}

struct SplitMix64(u64);

impl SplitMix64 {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    #[allow(clippy::cast_precision_loss)]
    fn next_f64(&mut self) -> f64 {
        let bits = self.next_u64() >> 11;
        (bits as f64) / ((1_u64 << 53) as f64)
    }

    fn sample_step_10(&mut self, min: i32, max: i32) -> i32 {
        if min >= max {
            return min;
        }
        let steps = (i64::from(max) - i64::from(min)) / 10 + 1;
        let pick = self.next_u64() % steps.cast_unsigned();
        i32::try_from(i64::from(min) + pick.cast_signed() * 10)
            .expect("sample is within the input bounds")
    }

    fn sample_f64(&mut self, min: f64, max: f64) -> f64 {
        if max <= min {
            return min;
        }
        min + (max - min) * self.next_f64()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mechcore_document::UnitSource;

    use super::*;
    use crate::solve::services::StrictGridSpace;

    fn data_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("profiles/data")
    }

    fn unit(index: i32, type_name: &str, x: i32, y: i32) -> StateUnit {
        StateUnit {
            type_name: type_name.to_owned(),
            index,
            value: Some(100),
            level: Some(1),
            source: UnitSource::Joined,
            recovered: false,
            position: Some(Position { x, y }),
            exp: None,
            rotated: None,
            travelling: None,
            record_position: None,
            record_rotated: None,
        }
    }

    #[test]
    fn data_tables_load_and_cover_all_33_catalog_units() {
        let dir = data_dir();
        let service = R1MacroTemplateService::from_files(
            &dir.join("unit_profiles.yaml"),
            &dir.join("r1_opening.yaml"),
        )
        .unwrap();

        let prices_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("config/unit_prices.yaml");
        let prices: serde_yaml::Value =
            serde_yaml::from_slice(&std::fs::read(prices_path).unwrap()).unwrap();
        let catalog_units = prices["units"].as_sequence().unwrap();
        assert_eq!(catalog_units.len(), 33);
        for row in catalog_units {
            let name = row["type"].as_str().unwrap();
            assert!(
                service.profiles.units.contains_key(name),
                "missing unit_profiles entry for {name:?}"
            );
        }
        assert!(service.profiles.profile("steel_ball").roles.carry >= 0.45);
    }

    #[test]
    fn opponent_subset_expands_to_six_normalized_deterministic_hypotheses() {
        let dir = data_dir();
        let service = R1MacroTemplateService::from_files(
            &dir.join("unit_profiles.yaml"),
            &dir.join("r1_opening.yaml"),
        )
        .unwrap();

        let mut state = SideState::default();
        for (idx, name) in [
            "sledgehammer",
            "sledgehammer",
            "marksman",
            "marksman",
            "arclight",
            "crawler",
            "crawler",
        ]
        .iter()
        .enumerate()
        {
            state.units.push(unit(
                i32::try_from(idx).unwrap(),
                name,
                -150 + i32::try_from(idx).unwrap() * 50,
                -150,
            ));
        }
        let unpinned: BTreeSet<i32> = (0..7).collect();
        let space = StrictGridSpace;
        let requested = vec![
            "asym_front".to_owned(),
            "asym_mid".to_owned(),
            "sym_mid".to_owned(),
            "sym_deep".to_owned(),
        ];
        let jitter = JitterOverride {
            shift_x: Some([-30, 30]),
            shift_y: Some([-20, 20]),
            ..JitterOverride::default()
        };
        let req = TemplateRequest {
            round: 1,
            red: false,
            state: &state,
            unpinned: &unpinned,
            constraints: &Constraints::default(),
            templates: &requested,
            variants_per_template: 1,
            include_clean_base: false,
            jitter: &jitter,
            seed: 42,
        };
        let a = service.generate(&space, &req).unwrap();
        let b = service.generate(&space, &req).unwrap();
        assert_eq!(a.len(), 6);
        let weight_sum: f64 = a.iter().map(|h| h.weight).sum();
        assert!((weight_sum - 1.0).abs() < 1e-9, "weight_sum={weight_sum}");
        for (lhs, rhs) in a.iter().zip(&b) {
            assert_eq!(lhs.label, rhs.label);
            assert_eq!(lhs.state, rhs.state);
        }
    }

    #[test]
    fn template_respects_rotation_permission_and_capped_zones() {
        let dir = data_dir();
        let mut service = R1MacroTemplateService::from_files(
            &dir.join("unit_profiles.yaml"),
            &dir.join("r1_opening.yaml"),
        )
        .unwrap();
        let mut state = SideState::default();
        state.units.push(unit(0, "crawler", 5, -150));
        let req = TemplateRequest {
            round: 1,
            red: false,
            state: &state,
            unpinned: &BTreeSet::from([0]),
            constraints: &Constraints {
                min_x: Some(-200),
                max_x: Some(-200),
                ..Constraints::default()
            },
            templates: &["asym_front".to_owned()],
            variants_per_template: 1,
            include_clean_base: true,
            jitter: &JitterOverride::default(),
            seed: 42,
        };
        assert!(!service.generate(&StrictGridSpace, &req).unwrap().is_empty());
        service
            .profiles
            .units
            .get_mut("crawler")
            .unwrap()
            .allow_rotate = false;
        assert!(service.generate(&StrictGridSpace, &req).unwrap().is_empty());
        let req = TemplateRequest {
            constraints: &Constraints::default(),
            ..req
        };
        let zone = &mut service.opening.templates[0].zones[0];
        zone.max_units = Some(1);
        assert!(!service.generate(&StrictGridSpace, &req).unwrap().is_empty());
        service.opening.templates[0].zones[0].max_units = Some(0);
        assert!(service.generate(&StrictGridSpace, &req).unwrap().is_empty());
    }

    #[test]
    fn service_refuses_unknown_templates_and_bad_jitter() {
        let dir = data_dir();
        let service = R1MacroTemplateService::from_files(
            &dir.join("unit_profiles.yaml"),
            &dir.join("r1_opening.yaml"),
        )
        .unwrap();
        let err = service
            .validate_request(
                &["no_such_template".to_owned()],
                1,
                &JitterOverride::default(),
            )
            .unwrap_err();
        assert!(format!("{err:?}").contains("no_such_template"));

        let bad_jitter = JitterOverride {
            shift_x: Some([-5, 10]),
            ..JitterOverride::default()
        };
        let err = service.validate_request(&[], 1, &bad_jitter).unwrap_err();
        assert!(format!("{err:?}").contains("10m"));
    }
}
