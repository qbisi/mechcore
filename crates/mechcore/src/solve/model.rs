//! Core domain types, service/hook traits, and shared [`SolveContext`] for `solve`.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use mechcore_document::{
    Position, Region,
    economy::Economy,
    mask::Mask,
    r#match::{Action, Offers, SideState, State},
    normalize,
};
use serde::{Deserialize, Serialize};

use crate::{cli::Failure, turn::Side};

use super::{
    predictor::{PlacementProposal, PlacementQuery},
    templates::{GeneratedLayout, JitterOverride, TemplateRequest},
};

/// Handle to one candidate branch stored in the solver's in-memory arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct BranchId(pub(crate) usize);

/// Human- and agent-readable semantic record of why a branch was created.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct IntentRecord {
    pub(crate) operator: String,
    pub(crate) summary: String,
}

/// One weighted opponent deployment hypothesis inside an opponent belief set.
#[derive(Clone, Debug)]
pub(crate) struct OpponentHypothesis {
    #[allow(dead_code)]
    pub(crate) label: String,
    pub(crate) weight: f64,
    pub(crate) state: SideState,
}

/// Fight evaluation metrics from the solver side's perspective.
///
/// For a single opponent hypothesis with weight `1.0`, `score` equals the exact
/// integer `core_delta = opponent_core_damage - own_core_damage`. For multi-belief
/// sets, `score` is the weighted objective across all hypotheses.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) struct Valuation {
    pub(crate) score: f64,
    pub(crate) min_core_delta: i32,
    pub(crate) own_core_damage: f64,
    pub(crate) opponent_core_damage: f64,
    pub(crate) ticks: u32,
    pub(crate) winner: &'static str,
}

/// Reversible scope constraints pushed onto [`SolveContext::scope_stack`].
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Constraints {
    #[serde(default)]
    pub(crate) min_x: Option<i32>,
    #[serde(default)]
    pub(crate) max_x: Option<i32>,
    #[serde(default)]
    pub(crate) min_y: Option<i32>,
    #[serde(default)]
    pub(crate) max_y: Option<i32>,
    #[serde(default)]
    pub(crate) regions: Option<Vec<String>>,
}

impl Constraints {
    /// Intersects `self` with a nested child scope's constraints.
    #[must_use]
    pub(crate) fn tighten(&self, child: &Self) -> Self {
        let min_x = match (self.min_x, child.min_x) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        let max_x = match (self.max_x, child.max_x) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let min_y = match (self.min_y, child.min_y) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        let max_y = match (self.max_y, child.max_y) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let regions = match (&self.regions, &child.regions) {
            (Some(a), Some(b)) => Some(
                a.iter()
                    .filter(|region| b.contains(region))
                    .cloned()
                    .collect(),
            ),
            (Some(a), None) => Some(a.clone()),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        };
        Self {
            min_x,
            max_x,
            min_y,
            max_y,
            regions,
        }
    }

    /// Whether `position` lies inside these active spatial constraints.
    #[must_use]
    pub(crate) fn admits(&self, position: Position) -> bool {
        if self.min_x.is_some_and(|min| position.x < min) {
            return false;
        }
        if self.max_x.is_some_and(|max| position.x > max) {
            return false;
        }
        if self.min_y.is_some_and(|min| position.y < min) {
            return false;
        }
        if self.max_y.is_some_and(|max| position.y > max) {
            return false;
        }
        if let Some(regions) = &self.regions {
            let name = match Region::of(position) {
                Region::Main => "main",
                Region::LeftFlank => "left_flank",
                Region::RightFlank => "right_flank",
            };
            if !regions.iter().any(|allowed| allowed == name) {
                return false;
            }
        }
        true
    }
}

/// One candidate deployment draft inside the solver's branch arena.
#[derive(Clone, Debug)]
pub(crate) struct Branch {
    pub(crate) id: BranchId,
    pub(crate) parent: Option<BranchId>,
    pub(crate) state: SideState,
    pub(crate) unpinned: BTreeSet<i32>,
    pub(crate) prior: f64,
    pub(crate) intent: IntentRecord,
    pub(crate) valuation: Option<Valuation>,
    /// Effective action history from round opening, retained only as a fallback
    /// when a warm-started human replay carries non-rational economic orderings.
    pub(crate) history: Vec<Action>,
}

/// One step along the winning lineage back to the round opening.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct LineageStep {
    pub(crate) node_id: usize,
    pub(crate) operator: String,
    pub(crate) summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) score: Option<f64>,
}

/// Per-stage funnel statistics recorded by `observer.trace`.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct StageTrace {
    pub(crate) index: usize,
    pub(crate) operator: String,
    pub(crate) in_beam: usize,
    pub(crate) out_beam: usize,
    pub(crate) simulations: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) best_score: Option<f64>,
}

/// A high-scoring alternative pruned by an evaluation stage.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct PrunedCandidate {
    pub(crate) node_id: usize,
    pub(crate) score: f64,
    pub(crate) own_core_damage: f64,
    pub(crate) opponent_core_damage: f64,
    pub(crate) intent: String,
}

/// Runtime search statistics inspected by `StopGuard` (`bail`) hooks.
#[derive(Clone, Debug, Default)]
pub(crate) struct SearchStats {
    pub(crate) simulations: usize,
    pub(crate) sweep: usize,
    pub(crate) best_score: Option<f64>,
    pub(crate) stagnant_sweeps: usize,
    pub(crate) stop_reason: Option<String>,
}

/// Passive lifecycle events emitted to `SearchObserver` (`emit`) hooks.
pub(crate) enum SearchEvent<'a> {
    StageFinished(StageTrace),
    CandidatesPruned(&'a [PrunedCandidate]),
    Stopped(&'a str),
}

/// In-memory storage for all candidate branches explored during one solve run.
#[derive(Default)]
pub(crate) struct Arena {
    branches: Vec<Branch>,
}

impl Arena {
    pub(crate) fn root(&mut self, state: SideState, unpinned: BTreeSet<i32>) -> BranchId {
        let id = BranchId(self.branches.len());
        self.branches.push(Branch {
            id,
            parent: None,
            state,
            unpinned,
            prior: 0.0,
            intent: IntentRecord {
                operator: "init".to_owned(),
                summary: "round opening".to_owned(),
            },
            valuation: None,
            history: Vec::new(),
        });
        id
    }

    pub(crate) fn fork(
        &mut self,
        parent: BranchId,
        state: SideState,
        unpinned: BTreeSet<i32>,
        prior: f64,
        intent: IntentRecord,
        history: Vec<Action>,
    ) -> BranchId {
        let id = BranchId(self.branches.len());
        self.branches.push(Branch {
            id,
            parent: Some(parent),
            state,
            unpinned,
            prior,
            intent,
            valuation: None,
            history,
        });
        id
    }

    #[must_use]
    pub(crate) fn get(&self, id: BranchId) -> &Branch {
        &self.branches[id.0]
    }

    pub(crate) fn get_mut(&mut self, id: BranchId) -> &mut Branch {
        &mut self.branches[id.0]
    }

    /// Reconstructs the causal chain of intents from the root to `id`.
    #[must_use]
    pub(crate) fn lineage(&self, id: BranchId) -> Vec<LineageStep> {
        let mut steps = Vec::new();
        let mut cursor = Some(id);
        while let Some(current) = cursor {
            let branch = self.get(current);
            steps.push(LineageStep {
                node_id: branch.id.0,
                operator: branch.intent.operator.clone(),
                summary: branch.intent.summary.clone(),
                score: branch.valuation.map(|valuation| valuation.score),
            });
            cursor = branch.parent;
        }
        steps.reverse();
        steps
    }
}

/// Static problem context for one match round.
pub(crate) struct Problem<'a> {
    pub(crate) economy: &'a Economy,
    pub(crate) round: i32,
    pub(crate) side: Side,
    pub(crate) map_id: i32,
    pub(crate) seed: i32,
    pub(crate) opening: &'a State,
    pub(crate) own_actions: &'a [Action],
}

impl<'a> Problem<'a> {
    #[must_use]
    pub(crate) const fn own_opening(&self) -> &'a SideState {
        match self.side {
            Side::Blue => &self.opening.blue,
            Side::Red => &self.opening.red,
        }
    }

    #[must_use]
    pub(crate) const fn opponent_opening(&self) -> &'a SideState {
        match self.side {
            Side::Blue => &self.opening.red,
            Side::Red => &self.opening.blue,
        }
    }

    #[must_use]
    pub(crate) const fn offers(&self) -> Option<&'a Offers> {
        self.opening.reinforce_offers.as_ref()
    }
}

/// Read-only context handed to an [`OpponentService`] before the pipeline runs.
pub(crate) struct OpponentContext<'a> {
    pub(crate) economy: &'a Economy,
    pub(crate) round: i32,
    pub(crate) seed: i32,
    pub(crate) opponent_side: Side,
    pub(crate) opponent_opening: &'a SideState,
    pub(crate) opponent_actions: &'a [Action],
    pub(crate) offers: Option<&'a Offers>,
    pub(crate) space: &'a dyn SpaceService,
    pub(crate) templates: Option<&'a dyn TemplateService>,
}

/// Service resolving the weighted opponent belief set used by fight evaluation.
pub(crate) trait OpponentService: Send + Sync {
    /// Resolves one or more weighted opponent hypotheses.
    ///
    /// # Errors
    /// Returns a refusal when the opponent belief cannot be materialized.
    fn resolve(&self, context: &OpponentContext<'_>) -> Result<Vec<OpponentHypothesis>, Failure>;
}

/// Service answering movement placement masks and constrained collision checks.
pub(crate) trait SpaceService: Send + Sync {
    /// Computes the legal move mask for standing formation `unit_index`,
    /// excluding that formation's own current footprint.
    ///
    /// # Errors
    /// Returns a refusal when the formation is absent or unsupported.
    fn move_mask(
        &self,
        round: i32,
        red: bool,
        state: &SideState,
        unit_index: i32,
    ) -> Result<Mask, Failure>;

    /// Computes the legal move mask for standing formation `unit_index`,
    /// excluding `unit_index` and every formation in `ignored_units`.
    ///
    /// # Errors
    /// Returns a refusal when the formation is absent or unsupported.
    fn move_mask_excluding(
        &self,
        round: i32,
        red: bool,
        state: &SideState,
        unit_index: i32,
        ignored_units: &BTreeSet<i32>,
    ) -> Result<Mask, Failure>;

    /// Whether `(position, rotated)` is legal under both `mask` and active
    /// scope `constraints`.
    fn can_place_move(
        &self,
        mask: &Mask,
        position: Position,
        rotated: bool,
        constraints: &Constraints,
    ) -> bool;
}

/// Shared service generating legal macro-template layouts for unpinned formations.
pub(crate) trait TemplateService: Send + Sync {
    /// Validates caller template selection and jitter overrides against loaded tables.
    ///
    /// # Errors
    /// Returns a refusal when any template name or jitter bound is invalid.
    fn validate_request(
        &self,
        templates: &[String],
        variants_per_template: usize,
        jitter: &JitterOverride,
    ) -> Result<(), Failure>;

    /// Materializes weighted, collision-free candidate layouts for `request`.
    ///
    /// # Errors
    /// Returns a refusal when generation fails.
    fn generate(
        &self,
        space: &dyn SpaceService,
        request: &TemplateRequest<'_>,
    ) -> Result<Vec<GeneratedLayout>, Failure>;
}

/// Service evaluating a candidate state against the sealed opponent belief set.
pub(crate) trait EvaluatorService: Send + Sync {
    /// Evaluates all distinct `own_states` across `opponents`, preserving
    /// `own_states` order.
    ///
    /// # Errors
    /// Returns a refusal or failure when projection or simulation fails.
    fn evaluate_batch(
        &self,
        problem: &Problem<'_>,
        opponents: &[OpponentHypothesis],
        own_states: &[&SideState],
    ) -> Result<Vec<Valuation>, Failure>;
}

/// Batch-first placement proposal seam (implemented by stubs or external NNs).
pub(crate) trait PlacementProposer: Send + Sync {
    #[allow(dead_code)]
    fn name(&self) -> &'static str;

    /// Proposes candidate placements for a batch of queries.
    ///
    /// # Errors
    /// Returns a failure when an external predictor fails or times out.
    fn propose_batch(
        &self,
        queries: &[PlacementQuery<'_>],
    ) -> Result<Vec<Vec<PlacementProposal>>, Failure>;
}

/// `bail` hook: first guard returning `Some(reason)` stops an active loop.
pub(crate) trait StopGuard: Send + Sync {
    fn check(&self, _stats: &SearchStats) -> Option<String> {
        None
    }

    /// Global cap applied before dispatching any pipeline fight batch.
    fn simulation_limit(&self) -> Option<usize> {
        None
    }
}

/// `waterfall` hook: transforms or filters proposals before evaluation.
pub(crate) trait ProposalFilter: Send + Sync {
    fn filter(
        &self,
        query: &PlacementQuery<'_>,
        constraints: &Constraints,
        proposals: Vec<PlacementProposal>,
    ) -> Vec<PlacementProposal>;
}

/// `emit` hook: passive observer for tracing or sample export.
pub(crate) trait SearchObserver: Send + Sync {
    fn on_event(&mut self, event: &SearchEvent<'_>);
}

/// Composable operator transforming a beam of branches into a new beam.
pub(crate) trait Operator: Send + Sync {
    fn name(&self) -> &str;

    /// Applies this operator to `beam` using the shared [`SolveContext`].
    ///
    /// # Errors
    /// Returns a refusal or failure when the stage cannot be executed.
    fn apply(
        &self,
        ctx: &mut SolveContext<'_>,
        beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure>;
}

/// Mounted capability services shared across all operators and nested loops.
pub(crate) struct Services {
    pub(crate) opponent: Box<dyn OpponentService>,
    pub(crate) space: Arc<dyn SpaceService>,
    pub(crate) evaluator: Box<dyn EvaluatorService>,
    pub(crate) templates: Option<Arc<dyn TemplateService>>,
    pub(crate) proposers: Vec<Box<dyn PlacementProposer>>,
    pub(crate) operators: BTreeMap<String, Arc<dyn Operator>>,
}

/// Mounted Cordis-style typed hook chains (`bail` / `waterfall` / `emit`).
#[derive(Default)]
pub(crate) struct Hooks {
    pub(crate) should_stop: Vec<Box<dyn StopGuard>>,
    pub(crate) filter_proposals: Vec<Box<dyn ProposalFilter>>,
    pub(crate) observers: Vec<Box<dyn SearchObserver>>,
}

/// Canonical digest of a [`SideState`] for transposition deduplication and sim caching.
///
/// # Errors
/// Returns a failure when the state cannot be serialized for hashing.
pub(crate) fn state_digest(state: &SideState) -> Result<[u8; 32], Failure> {
    let bytes = serde_json::to_vec(state)
        .map_err(|error| Failure::failed(format!("cannot hash state: {error}")))?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

/// Shared execution context passed into every operator and search loop.
///
/// `opponent_beliefs` stays private inside this struct so proposal and placement
/// plugins only observe `problem.opponent_opening()` and receive fight metrics
/// through `evaluate_batch()`.
pub(crate) struct SolveContext<'a> {
    pub(crate) problem: &'a Problem<'a>,
    opponent_beliefs: Vec<OpponentHypothesis>,
    pub(crate) services: &'a Services,
    pub(crate) hooks: Hooks,
    pub(crate) arena: Arena,
    sim_cache: BTreeMap<[u8; 32], Valuation>,
    pub(crate) stats: SearchStats,
    scope_stack: Vec<Constraints>,
    stage_counter: usize,
}

impl<'a> SolveContext<'a> {
    pub(crate) fn new(
        problem: &'a Problem<'a>,
        opponent_beliefs: Vec<OpponentHypothesis>,
        services: &'a Services,
        hooks: Hooks,
    ) -> Self {
        Self {
            problem,
            opponent_beliefs,
            services,
            hooks,
            arena: Arena::default(),
            sim_cache: BTreeMap::new(),
            stats: SearchStats::default(),
            scope_stack: Vec::new(),
            stage_counter: 0,
        }
    }

    /// Pushes a reversible constraint scope onto the context stack.
    pub(crate) fn push_scope(&mut self, constraints: Constraints) {
        self.scope_stack.push(constraints);
    }

    /// Pops the innermost constraint scope from the context stack.
    pub(crate) fn pop_scope(&mut self) {
        self.scope_stack.pop();
    }

    /// Effective constraints formed by intersecting all active scopes.
    #[must_use]
    pub(crate) fn active_constraints(&self) -> Constraints {
        self.scope_stack
            .iter()
            .fold(Constraints::default(), |acc, layer| acc.tighten(layer))
    }

    /// Evaluates all `StopGuard` (`bail`) hooks; latches the first stop reason.
    pub(crate) fn check_stop(&mut self) -> Option<String> {
        if let Some(reason) = &self.stats.stop_reason {
            return Some(reason.clone());
        }
        for guard in &self.hooks.should_stop {
            if let Some(reason) = guard.check(&self.stats) {
                self.record_stop(&reason);
                return Some(reason);
            }
        }
        None
    }

    fn record_stop(&mut self, reason: &str) {
        if self.stats.stop_reason.is_none() {
            self.stats.stop_reason = Some(reason.to_owned());
            self.emit(&SearchEvent::Stopped(reason));
        }
    }

    /// Broadcasts a passive lifecycle event to all mounted `SearchObserver`s.
    pub(crate) fn emit(&mut self, event: &SearchEvent<'_>) {
        for observer in &mut self.hooks.observers {
            observer.on_event(event);
        }
    }

    /// Invokes a named operator registered in `services.operators`.
    ///
    /// # Errors
    /// Returns a refusal if `name` is not registered, or any operator error.
    pub(crate) fn call_operator(
        &mut self,
        name: &str,
        beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        let operator = self
            .services
            .operators
            .get(name)
            .cloned()
            .ok_or_else(|| Failure::refused(format!("unknown named operator {name:?}")))?;
        self.run_operator(operator.as_ref(), beam)
    }

    /// Runs an operator and emits its [`StageTrace`] to observers.
    ///
    /// # Errors
    /// Returns any failure raised by `operator`.
    pub(crate) fn run_operator(
        &mut self,
        operator: &dyn Operator,
        beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        let index = self.stage_counter;
        self.stage_counter += 1;
        let in_beam = beam.len();
        let sims_before = self.stats.simulations;
        let out = operator.apply(self, beam)?;
        let best_score = out
            .iter()
            .filter_map(|&id| self.arena.get(id).valuation.map(|val| val.score))
            .max_by(f64::total_cmp);
        self.emit(&SearchEvent::StageFinished(StageTrace {
            index,
            operator: operator.name().to_owned(),
            in_beam,
            out_beam: out.len(),
            simulations: self.stats.simulations - sims_before,
            best_score,
        }));
        Ok(out)
    }

    /// Collects proposals from all mounted `services.proposers` for a batch of
    /// queries, then passes each query's merged list through all `waterfall`
    /// `hooks.filter_proposals`.
    ///
    /// # Errors
    /// Returns any failure raised by a mounted proposer.
    pub(crate) fn propose_batch(
        &self,
        queries: &[PlacementQuery<'_>],
    ) -> Result<Vec<Vec<PlacementProposal>>, Failure> {
        let mut merged: Vec<Vec<PlacementProposal>> = vec![Vec::new(); queries.len()];
        for proposer in &self.services.proposers {
            let batch = proposer.propose_batch(queries)?;
            if batch.len() != queries.len() {
                return Err(Failure::failed(format!(
                    "proposer {} returned {} outputs for {} queries",
                    proposer.name(),
                    batch.len(),
                    queries.len()
                )));
            }
            for (slot, items) in merged.iter_mut().zip(batch) {
                slot.extend(items);
            }
        }
        let constraints = self.active_constraints();
        for (query, proposals) in queries.iter().zip(merged.iter_mut()) {
            let mut filtered = std::mem::take(proposals);
            for filter in &self.hooks.filter_proposals {
                filtered = filter.filter(query, &constraints, filtered);
            }
            *proposals = filtered;
        }
        Ok(merged)
    }

    /// Scores beams as one batch and retains fully evaluated branches in each
    /// beam's original order. Duplicate states share a score across beams.
    ///
    /// # Errors
    /// Returns the first projection or simulation failure encountered.
    pub(crate) fn evaluate_batch(&mut self, beams: &mut [Vec<BranchId>]) -> Result<(), Failure> {
        let mut digests = Vec::new();
        let mut unique_misses: Vec<([u8; 32], SideState)> = Vec::new();
        let mut seen_misses = BTreeSet::new();

        for &id in beams.iter().flatten() {
            if self.arena.get(id).valuation.is_some() {
                continue;
            }
            let state = &self.arena.get(id).state;
            let digest = state_digest(state)?;
            digests.push((id, digest));
            if !self.sim_cache.contains_key(&digest) && seen_misses.insert(digest) {
                unique_misses.push((digest, state.clone()));
            }
        }

        self.evaluate_misses(unique_misses)?;
        for (id, digest) in digests {
            self.arena.get_mut(id).valuation = self.sim_cache.get(&digest).copied();
        }
        for beam in beams {
            beam.retain(|&id| self.arena.get(id).valuation.is_some());
        }
        Ok(())
    }

    fn evaluate_misses(
        &mut self,
        mut unique_misses: Vec<([u8; 32], SideState)>,
    ) -> Result<(), Failure> {
        let cost = self.opponent_beliefs.len();
        if cost == 0 {
            return Err(Failure::refused(
                "opponent belief service returned no hypotheses",
            ));
        }
        let limit = self
            .hooks
            .should_stop
            .iter()
            .filter_map(|guard| guard.simulation_limit())
            .min();
        if let Some(limit) = limit {
            unique_misses.truncate(limit.saturating_sub(self.stats.simulations) / cost);
        }
        if !unique_misses.is_empty() {
            let miss_count = unique_misses.len();
            let state_refs: Vec<&SideState> =
                unique_misses.iter().map(|(_, state)| state).collect();
            let valuations = self.services.evaluator.evaluate_batch(
                self.problem,
                &self.opponent_beliefs,
                &state_refs,
            )?;
            for ((digest, _), valuation) in unique_misses.into_iter().zip(valuations) {
                self.sim_cache.insert(digest, valuation);
                self.update_best(valuation.score);
            }
            self.stats.simulations += miss_count * cost;
        }

        if let Some(limit) = limit
            && limit.saturating_sub(self.stats.simulations) < cost
        {
            self.record_stop(&format!(
                "simulation budget reached ({}/{limit})",
                self.stats.simulations
            ));
        }
        Ok(())
    }

    fn update_best(&mut self, score: f64) {
        if self.stats.best_score.is_none_or(|best| score > best) {
            self.stats.best_score = Some(score);
        }
    }

    /// Updates sweep counters after one `repeat` iteration completes.
    pub(crate) fn record_sweep_end(&mut self, best_before: Option<f64>) {
        self.stats.sweep += 1;
        let improved = match (best_before, self.stats.best_score) {
            (None, Some(_)) => true,
            (Some(before), Some(after)) => after > before + 1e-9,
            _ => false,
        };
        if improved {
            self.stats.stagnant_sweeps = 0;
        } else {
            self.stats.stagnant_sweeps += 1;
        }
    }

    /// Compiles a branch's closing state into a legal action list from round opening.
    ///
    /// Prefers pure two-endpoint normalization (`normalize::deployment`), falling
    /// back to compacting `branch.history` when a warm-started human replay carried
    /// non-rational economic orderings.
    ///
    /// # Errors
    /// Returns a refusal if neither normalization nor history compaction succeeds.
    pub(crate) fn compile_actions(&self, branch: &Branch) -> Result<(Vec<Action>, bool), Failure> {
        match normalize::deployment(
            self.problem.economy,
            self.problem.own_opening(),
            &branch.state,
            self.problem.offers(),
            self.problem.side.red(),
        ) {
            Ok(actions) => Ok((actions, true)),
            Err(error) if !branch.history.is_empty() => normalize::historical_deployment(
                self.problem.economy,
                self.problem.own_opening(),
                &branch.state,
                self.problem.side.red(),
                self.problem.offers(),
                &branch.history,
            )
            .map(|actions| (actions, false))
            .map_err(|historical| {
                Failure::refused(format!(
                    "cannot compile actions: {error}; historical fallback also failed: {historical}"
                ))
            }),
            Err(error) => Err(Failure::refused(format!(
                "cannot normalize closing state: {error}"
            ))),
        }
    }
}
