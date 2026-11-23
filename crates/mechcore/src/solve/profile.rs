//! Declarative profile manifest (`mechcore.profile/v1`) and static plugin assembly.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::Deserialize;

use crate::cli::Failure;

use super::{
    hooks::{
        DedupPosesFilter, MaxSimulationsGuard, ScopeBoundsFilter, StagnationGuard, TraceBuffer,
        TraceObserver,
    },
    model::{
        Constraints, EvaluatorService, Hooks, Operator, OpponentService, PlacementProposer,
        ProposalFilter, SearchObserver, Services, SpaceService, StopGuard, TemplateService,
    },
    operators::{
        BranchOperator, CallOperator, CoordinateSweepOperator, DedupOperator, EvalScoreOperator,
        EvalTopKOperator, PredictUnpinnedPlacement, RepeatOperator, ScopeOperator,
        SeedMacroTemplatesOperator, SeqOperator, WarmStartOwnActions,
    },
    predictor::{ShiftStubProposer, StayProposer},
    services::{
        ExactSimEvaluator, ReplayTruthOpponent, ReplayUnpinnedTemplatesOpponent, StrictGridSpace,
    },
    templates::R1MacroTemplateService,
};

pub(crate) const SCHEMA: &str = "mechcore.profile/v1";

/// Declarative solver profile manifest (`--profile <profile.yaml>`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    pub(crate) schema: String,
    pub(crate) name: String,
    pub(crate) services: ServicesSpec,
    #[serde(default)]
    pub(crate) proposers: Vec<PluginSpec>,
    #[serde(default)]
    pub(crate) guards: Vec<PluginSpec>,
    #[serde(default)]
    pub(crate) filters: Vec<PluginSpec>,
    #[serde(default)]
    pub(crate) observers: Vec<PluginSpec>,
    #[serde(default)]
    pub(crate) operators: BTreeMap<String, StageSpec>,
    pub(crate) pipeline: Vec<StageSpec>,
}

/// Mounted capability services declared in a profile.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServicesSpec {
    #[serde(default)]
    pub(crate) templates: Option<PluginSpec>,
    pub(crate) opponent: PluginSpec,
    #[serde(default)]
    pub(crate) space: Option<PluginSpec>,
    #[serde(default)]
    pub(crate) evaluator: Option<PluginSpec>,
}

/// Reference to one plugin and its parameter map.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginSpec {
    #[serde(rename = "use")]
    pub(crate) plugin: String,
    #[serde(default)]
    pub(crate) with: serde_json::Value,
}

/// Recursive stage AST supporting leaf plugins, named calls, parallel branches,
/// bounded loops, reversible constraint scopes, and inline sequences.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum StageSpec {
    Use {
        #[serde(rename = "use")]
        plugin: String,
        #[serde(default)]
        with: serde_json::Value,
    },
    Call {
        call: String,
    },
    Branch {
        branch: Vec<StageSpec>,
    },
    Repeat {
        repeat: RepeatSpec,
    },
    Scope {
        scope: ScopeSpec,
    },
    Seq {
        seq: Vec<StageSpec>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RepeatSpec {
    pub(crate) max_iters: usize,
    #[serde(default = "default_true")]
    pub(crate) stop_on_bail: bool,
    pub(crate) body: Vec<StageSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScopeSpec {
    pub(crate) constraints: Constraints,
    pub(crate) body: Vec<StageSpec>,
}

const fn default_true() -> bool {
    true
}

/// Instantiated services, hooks, trace handle, and root pipeline operator.
pub(crate) struct BuiltProfile {
    pub(crate) services: Services,
    pub(crate) hooks: Hooks,
    pub(crate) pipeline: Vec<Arc<dyn Operator>>,
    pub(crate) trace: Arc<Mutex<TraceBuffer>>,
}

impl Profile {
    /// Reads and validates a YAML/JSON solver profile from disk.
    ///
    /// # Errors
    /// Returns a failure when the file cannot be read, or a refusal when its
    /// schema or structure is invalid.
    pub(crate) fn load(path: &Path) -> Result<Self, Failure> {
        let bytes = std::fs::read(path).map_err(|error| {
            Failure::failed(format!("cannot read profile {}: {error}", path.display()))
        })?;
        let profile: Self = serde_yaml::from_slice(&bytes).map_err(|error| {
            Failure::refused(format!("invalid profile {}: {error}", path.display()))
        })?;
        if profile.schema != SCHEMA {
            return Err(Failure::refused(format!(
                "unsupported profile schema {:?}; expected {SCHEMA:?}",
                profile.schema
            )));
        }
        if profile.pipeline.is_empty() {
            return Err(Failure::refused(
                "profile pipeline must contain at least one stage",
            ));
        }
        Ok(profile)
    }

    /// Instantiates all services, hooks, named operators, and the pipeline AST,
    /// statically validating every `call` reference and checking for call cycles.
    ///
    /// # Errors
    /// Returns a refusal for unknown plugins, invalid parameters, dangling
    /// operator calls, or cyclic `call` definitions.
    pub(crate) fn build(&self, base_dir: &Path) -> Result<BuiltProfile, Failure> {
        let templates = match &self.services.templates {
            Some(spec) => Some(build_templates(spec, base_dir)?),
            None => None,
        };
        let opponent = build_opponent(&self.services.opponent, templates.as_deref())?;
        let space: Arc<dyn SpaceService> = match &self.services.space {
            Some(spec) => build_space(spec)?,
            None => Arc::new(StrictGridSpace),
        };
        let evaluator = match &self.services.evaluator {
            Some(spec) => build_evaluator(spec)?,
            None => Box::new(ExactSimEvaluator::from_value(&serde_json::Value::Null)?),
        };
        let proposers = self
            .proposers
            .iter()
            .map(build_proposer)
            .collect::<Result<Vec<_>, _>>()?;
        let should_stop = self
            .guards
            .iter()
            .map(build_guard)
            .collect::<Result<Vec<_>, _>>()?;
        let filter_proposals = self
            .filters
            .iter()
            .map(build_filter)
            .collect::<Result<Vec<_>, _>>()?;

        let trace = Arc::new(Mutex::new(TraceBuffer::default()));
        let observers = build_observers(&self.observers, &trace)?;

        let defined: BTreeSet<&str> = self.operators.keys().map(String::as_str).collect();
        validate_call_graph(&self.operators, &self.pipeline, &defined)?;

        let mut operators = BTreeMap::new();
        for (name, spec) in &self.operators {
            operators.insert(name.clone(), build_stage(spec, templates.as_deref())?);
        }
        let pipeline = self
            .pipeline
            .iter()
            .map(|spec| build_stage(spec, templates.as_deref()))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(BuiltProfile {
            services: Services {
                opponent,
                space,
                evaluator,
                templates,
                proposers,
                operators,
            },
            hooks: Hooks {
                should_stop,
                filter_proposals,
                observers,
            },
            pipeline,
            trace,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct R1MacroTemplatesConfig {
    unit_profiles: PathBuf,
    opening_templates: PathBuf,
}

fn resolve_profile_path(base_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn build_templates(
    spec: &PluginSpec,
    base_dir: &Path,
) -> Result<Arc<dyn TemplateService>, Failure> {
    match spec.plugin.as_str() {
        "templates.r1_macro" => {
            let config: R1MacroTemplatesConfig = serde_json::from_value(spec.with.clone())
                .map_err(|error| {
                    Failure::refused(format!(
                        "invalid parameters for templates.r1_macro: {error}"
                    ))
                })?;
            let profiles_path = resolve_profile_path(base_dir, &config.unit_profiles);
            let opening_path = resolve_profile_path(base_dir, &config.opening_templates);
            Ok(Arc::new(R1MacroTemplateService::from_files(
                &profiles_path,
                &opening_path,
            )?))
        }
        other => Err(Failure::refused(format!(
            "unknown templates service {other:?}; expected templates.r1_macro"
        ))),
    }
}

fn ensure_no_params(plugin: &str, with: &serde_json::Value) -> Result<(), Failure> {
    if with.is_null() || with.as_object().is_some_and(serde_json::Map::is_empty) {
        return Ok(());
    }
    Err(Failure::refused(format!("{plugin} takes no parameters")))
}

fn build_opponent(
    spec: &PluginSpec,
    templates: Option<&dyn TemplateService>,
) -> Result<Box<dyn OpponentService>, Failure> {
    match spec.plugin.as_str() {
        "opponent.replay_truth" => {
            ensure_no_params("opponent.replay_truth", &spec.with)?;
            Ok(Box::new(ReplayTruthOpponent))
        }
        "opponent.replay_unpinned_templates" => Ok(Box::new(
            ReplayUnpinnedTemplatesOpponent::from_value(&spec.with, templates)?,
        )),
        other => Err(Failure::refused(format!(
            "unknown opponent service {other:?}; expected opponent.replay_truth or opponent.replay_unpinned_templates"
        ))),
    }
}

fn build_space(spec: &PluginSpec) -> Result<Arc<dyn SpaceService>, Failure> {
    match spec.plugin.as_str() {
        "space.strict_grid" => {
            ensure_no_params("space.strict_grid", &spec.with)?;
            Ok(Arc::new(StrictGridSpace))
        }
        other => Err(Failure::refused(format!(
            "unknown space service {other:?}; expected space.strict_grid"
        ))),
    }
}

fn build_evaluator(spec: &PluginSpec) -> Result<Box<dyn EvaluatorService>, Failure> {
    match spec.plugin.as_str() {
        "eval.sim_exact" => Ok(Box::new(ExactSimEvaluator::from_value(&spec.with)?)),
        other => Err(Failure::refused(format!(
            "unknown evaluator service {other:?}; expected eval.sim_exact"
        ))),
    }
}

fn build_proposer(spec: &PluginSpec) -> Result<Box<dyn PlacementProposer>, Failure> {
    match spec.plugin.as_str() {
        "proposer.stay" => {
            ensure_no_params("proposer.stay", &spec.with)?;
            Ok(Box::new(StayProposer))
        }
        "proposer.shift_stub" => Ok(Box::new(ShiftStubProposer::from_value(&spec.with)?)),
        other => Err(Failure::refused(format!(
            "unknown placement proposer {other:?}; expected proposer.stay or proposer.shift_stub"
        ))),
    }
}

fn build_guard(spec: &PluginSpec) -> Result<Box<dyn StopGuard>, Failure> {
    match spec.plugin.as_str() {
        "guard.max_simulations" => Ok(Box::new(MaxSimulationsGuard::from_value(&spec.with)?)),
        "guard.stagnation" => Ok(Box::new(StagnationGuard::from_value(&spec.with)?)),
        other => Err(Failure::refused(format!(
            "unknown guard plugin {other:?}; expected guard.max_simulations or guard.stagnation"
        ))),
    }
}

fn build_filter(spec: &PluginSpec) -> Result<Box<dyn ProposalFilter>, Failure> {
    match spec.plugin.as_str() {
        "filter.dedup_poses" => {
            ensure_no_params("filter.dedup_poses", &spec.with)?;
            Ok(Box::new(DedupPosesFilter))
        }
        "filter.scope_bounds" => {
            ensure_no_params("filter.scope_bounds", &spec.with)?;
            Ok(Box::new(ScopeBoundsFilter))
        }
        other => Err(Failure::refused(format!(
            "unknown filter plugin {other:?}; expected filter.dedup_poses or filter.scope_bounds"
        ))),
    }
}

fn build_observers(
    specs: &[PluginSpec],
    trace: &Arc<Mutex<TraceBuffer>>,
) -> Result<Vec<Box<dyn SearchObserver>>, Failure> {
    let mut out: Vec<Box<dyn SearchObserver>> = Vec::with_capacity(specs.len().max(1));
    if specs.is_empty() {
        out.push(Box::new(TraceObserver::new(Arc::clone(trace))));
        return Ok(out);
    }
    for spec in specs {
        match spec.plugin.as_str() {
            "observer.trace" => {
                ensure_no_params("observer.trace", &spec.with)?;
                out.push(Box::new(TraceObserver::new(Arc::clone(trace))));
            }
            other => {
                return Err(Failure::refused(format!(
                    "unknown observer plugin {other:?}; expected observer.trace"
                )));
            }
        }
    }
    Ok(out)
}

fn build_stage(
    spec: &StageSpec,
    templates: Option<&dyn TemplateService>,
) -> Result<Arc<dyn Operator>, Failure> {
    match spec {
        StageSpec::Use { plugin, with } => build_leaf_operator(plugin, with, templates),
        StageSpec::Call { call } => Ok(Arc::new(CallOperator::new(call.clone()))),
        StageSpec::Seq { seq } => {
            if seq.is_empty() {
                return Err(Failure::refused(
                    "seq combinator requires at least one step",
                ));
            }
            let steps = seq
                .iter()
                .map(|step| build_stage(step, templates))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Arc::new(SeqOperator::new(steps)))
        }
        StageSpec::Branch { branch } => {
            if branch.is_empty() {
                return Err(Failure::refused(
                    "branch combinator requires at least one arm",
                ));
            }
            let arms = branch
                .iter()
                .map(|arm| build_stage(arm, templates))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Arc::new(BranchOperator::new(arms)))
        }
        StageSpec::Repeat { repeat } => {
            if repeat.max_iters == 0 {
                return Err(Failure::refused(
                    "repeat combinator requires max_iters >= 1",
                ));
            }
            if repeat.body.is_empty() {
                return Err(Failure::refused(
                    "repeat combinator requires a non-empty body",
                ));
            }
            let steps = repeat
                .body
                .iter()
                .map(|step| build_stage(step, templates))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Arc::new(RepeatOperator::new(
                repeat.max_iters,
                repeat.stop_on_bail,
                Arc::new(SeqOperator::new(steps)),
            )))
        }
        StageSpec::Scope { scope } => {
            if scope.body.is_empty() {
                return Err(Failure::refused(
                    "scope combinator requires a non-empty body",
                ));
            }
            let steps = scope
                .body
                .iter()
                .map(|step| build_stage(step, templates))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Arc::new(ScopeOperator::new(
                scope.constraints.clone(),
                Arc::new(SeqOperator::new(steps)),
            )))
        }
    }
}

fn build_leaf_operator(
    plugin: &str,
    with: &serde_json::Value,
    templates: Option<&dyn TemplateService>,
) -> Result<Arc<dyn Operator>, Failure> {
    match plugin {
        "warm_start.own_actions" => Ok(Arc::new(WarmStartOwnActions::from_value(with)?)),
        "seed.macro_templates" => Ok(Arc::new(SeedMacroTemplatesOperator::from_value(
            with, templates,
        )?)),
        "placement.coordinate_sweep" => Ok(Arc::new(CoordinateSweepOperator::from_value(with)?)),
        "placement.predict_unpinned" => Ok(Arc::new(PredictUnpinnedPlacement::from_value(with)?)),
        "flow.dedup" => {
            ensure_no_params("flow.dedup", with)?;
            Ok(Arc::new(DedupOperator))
        }
        "eval.score" => {
            ensure_no_params("eval.score", with)?;
            Ok(Arc::new(EvalScoreOperator))
        }
        "eval.top_k" => Ok(Arc::new(EvalTopKOperator::from_value(with)?)),
        other => Err(Failure::refused(format!(
            "unknown pipeline operator {other:?}"
        ))),
    }
}

fn validate_call_graph(
    operators: &BTreeMap<String, StageSpec>,
    pipeline: &[StageSpec],
    defined: &BTreeSet<&str>,
) -> Result<(), Failure> {
    let mut edges: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (name, spec) in operators {
        let mut calls = BTreeSet::new();
        collect_calls(spec, &mut calls);
        for target in &calls {
            if !defined.contains(target) {
                return Err(Failure::refused(format!(
                    "operator {name:?} calls undefined operator {target:?}"
                )));
            }
        }
        edges.insert(name.as_str(), calls);
    }
    let mut pipeline_calls = BTreeSet::new();
    for stage in pipeline {
        collect_calls(stage, &mut pipeline_calls);
    }
    for target in pipeline_calls {
        if !defined.contains(target) {
            return Err(Failure::refused(format!(
                "pipeline calls undefined operator {target:?}"
            )));
        }
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for &name in defined {
        detect_cycle(name, &edges, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn collect_calls<'a>(spec: &'a StageSpec, out: &mut BTreeSet<&'a str>) {
    match spec {
        StageSpec::Use { .. } => {}
        StageSpec::Call { call } => {
            out.insert(call.as_str());
        }
        StageSpec::Seq { seq } => {
            for step in seq {
                collect_calls(step, out);
            }
        }
        StageSpec::Branch { branch } => {
            for arm in branch {
                collect_calls(arm, out);
            }
        }
        StageSpec::Repeat { repeat } => {
            for step in &repeat.body {
                collect_calls(step, out);
            }
        }
        StageSpec::Scope { scope } => {
            for step in &scope.body {
                collect_calls(step, out);
            }
        }
    }
}

fn detect_cycle<'a>(
    node: &'a str,
    edges: &BTreeMap<&'a str, BTreeSet<&'a str>>,
    visiting: &mut BTreeSet<&'a str>,
    visited: &mut BTreeSet<&'a str>,
) -> Result<(), Failure> {
    if visited.contains(node) {
        return Ok(());
    }
    if !visiting.insert(node) {
        return Err(Failure::refused(format!(
            "cyclic operator call detected at {node:?}"
        )));
    }
    if let Some(neighbors) = edges.get(node) {
        for &next in neighbors {
            detect_cycle(next, edges, visiting, visited)?;
        }
    }
    visiting.remove(node);
    visited.insert(node);
    Ok(())
}
