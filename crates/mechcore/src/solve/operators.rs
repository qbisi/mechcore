//! Composable control-flow combinators and essential search operators for `solve`.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use mechcore_document::{Position, landing, mask::Mask, r#match::Action, transition};
use serde::Deserialize;

use crate::cli::Failure;

use super::{
    model::{
        BranchId, Constraints, IntentRecord, Operator, PrunedCandidate, SearchEvent, SolveContext,
        Valuation, state_digest,
    },
    predictor::{PlacementProposal, PlacementQuery},
};

/// Sequential composition `P_n |> ... |> P_1`.
pub(crate) struct SeqOperator {
    steps: Vec<Arc<dyn Operator>>,
}

impl SeqOperator {
    pub(crate) const fn new(steps: Vec<Arc<dyn Operator>>) -> Self {
        Self { steps }
    }
}

impl Operator for SeqOperator {
    fn name(&self) -> &'static str {
        "combinator.seq"
    }

    fn apply(
        &self,
        ctx: &mut SolveContext<'_>,
        mut beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        for step in &self.steps {
            beam = ctx.run_operator(step.as_ref(), beam)?;
        }
        Ok(beam)
    }
}

/// Calls a named operator registered in `services.operators`.
pub(crate) struct CallOperator {
    target: String,
    label: String,
}

impl CallOperator {
    pub(crate) fn new(target: String) -> Self {
        let label = format!("call:{target}");
        Self { target, label }
    }
}

impl Operator for CallOperator {
    fn name(&self) -> &str {
        &self.label
    }

    fn apply(
        &self,
        ctx: &mut SolveContext<'_>,
        beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        ctx.call_operator(&self.target, beam)
    }
}

/// Parallel branching union `P_1 (+) P_2 (+) ...`: feeds the incoming beam
/// into each arm independently and concatenates the resulting beams.
pub(crate) struct BranchOperator {
    arms: Vec<Arc<dyn Operator>>,
}

impl BranchOperator {
    pub(crate) const fn new(arms: Vec<Arc<dyn Operator>>) -> Self {
        Self { arms }
    }
}

impl Operator for BranchOperator {
    fn name(&self) -> &'static str {
        "combinator.branch"
    }

    fn apply(
        &self,
        ctx: &mut SolveContext<'_>,
        beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        let mut combined = Vec::new();
        for arm in &self.arms {
            let out = ctx.run_operator(arm.as_ref(), beam.clone())?;
            combined.extend(out);
        }
        Ok(combined)
    }
}

/// Bounded iterative loop combinator honoring `StopGuard` (`bail`) hooks.
pub(crate) struct RepeatOperator {
    max_iters: usize,
    stop_on_bail: bool,
    body: Arc<dyn Operator>,
}

impl RepeatOperator {
    pub(crate) const fn new(max_iters: usize, stop_on_bail: bool, body: Arc<dyn Operator>) -> Self {
        Self {
            max_iters,
            stop_on_bail,
            body,
        }
    }
}

impl Operator for RepeatOperator {
    fn name(&self) -> &'static str {
        "combinator.repeat"
    }

    fn apply(
        &self,
        ctx: &mut SolveContext<'_>,
        mut beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        for _ in 0..self.max_iters {
            if self.stop_on_bail && ctx.check_stop().is_some() {
                break;
            }
            let best_before = ctx.stats.best_score;
            beam = ctx.run_operator(self.body.as_ref(), beam)?;
            ctx.record_sweep_end(best_before);
            if self.stop_on_bail && ctx.check_stop().is_some() {
                break;
            }
        }
        Ok(beam)
    }
}

/// Reversible scope combinator pushing [`Constraints`] on entry and popping on exit.
pub(crate) struct ScopeOperator {
    constraints: Constraints,
    body: Arc<dyn Operator>,
}

impl ScopeOperator {
    pub(crate) const fn new(constraints: Constraints, body: Arc<dyn Operator>) -> Self {
        Self { constraints, body }
    }
}

impl Operator for ScopeOperator {
    fn name(&self) -> &'static str {
        "combinator.scope"
    }

    fn apply(
        &self,
        ctx: &mut SolveContext<'_>,
        beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        ctx.push_scope(self.constraints.clone());
        let result = ctx.run_operator(self.body.as_ref(), beam);
        ctx.pop_scope();
        result
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WarmStartConfig {
    #[serde(default = "default_true")]
    unpin_all_movable: bool,
}

const fn default_true() -> bool {
    true
}

/// Replays our own recorded round actions onto each incoming branch and
/// optionally unpins every standing formation with movement permission.
pub(crate) struct WarmStartOwnActions {
    unpin_all_movable: bool,
}

impl WarmStartOwnActions {
    pub(crate) fn from_value(value: &serde_json::Value) -> Result<Self, Failure> {
        let config: WarmStartConfig = parse_params("warm_start.own_actions", value)?;
        Ok(Self {
            unpin_all_movable: config.unpin_all_movable,
        })
    }
}

impl Operator for WarmStartOwnActions {
    fn name(&self) -> &'static str {
        "warm_start.own_actions"
    }

    fn apply(
        &self,
        ctx: &mut SolveContext<'_>,
        beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        let effective: Vec<Action> = ctx
            .problem
            .own_actions
            .iter()
            .filter(|action| !matches!(action, Action::Concede))
            .cloned()
            .collect();
        let mut next = Vec::with_capacity(beam.len());
        for parent in beam {
            let base_state = ctx.arena.get(parent).state.clone();
            let base_prior = ctx.arena.get(parent).prior;
            let mut history = ctx.arena.get(parent).history.clone();
            let deployed = transition::deployed(
                ctx.problem.economy,
                &base_state,
                &effective,
                ctx.problem.side.red(),
                ctx.problem.offers(),
            )
            .map_err(|error| {
                Failure::refused(format!(
                    "warm_start.own_actions cannot replay {} actions: {error}",
                    ctx.problem.side.name()
                ))
            })?;
            let unpinned = if self.unpin_all_movable {
                deployed
                    .active_units()
                    .filter(|unit| unit.movable(&deployed))
                    .map(|unit| unit.index)
                    .collect()
            } else {
                transition::unpinned_units(&deployed, &effective)
            };
            let movable_count = unpinned.len();
            history.extend(effective.iter().cloned());
            let summary = if self.unpin_all_movable {
                format!(
                    "replayed {} own action(s) and unpinned {movable_count} movable formation(s)",
                    effective.len()
                )
            } else {
                format!(
                    "replayed {} own action(s), leaving {movable_count} unpinned formation(s)",
                    effective.len()
                )
            };
            let child = ctx.arena.fork(
                parent,
                deployed,
                unpinned,
                base_prior,
                IntentRecord {
                    operator: self.name().to_owned(),
                    summary,
                },
                history,
            );
            next.push(child);
        }
        Ok(next)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoordinateSweepConfig {
    #[serde(default = "default_keep_per_unit")]
    keep_per_unit: usize,
    #[serde(default)]
    beam_width: Option<usize>,
}

const fn default_keep_per_unit() -> usize {
    1
}

/// Iterative coordinate-ascent placement sweep over all movable formations.
///
/// For each formation in deterministic `unit.index` order:
/// 1. Checks `StopGuard` (`bail`) hooks.
/// 2. Queries `SpaceService::move_mask` for every active branch.
/// 3. Collects & filters proposals from all mounted `proposers` & `filters`.
/// 4. Applies valid poses, evaluates the whole candidate batch in parallel via
///    `EvaluatorService`, retains top `keep_per_unit` children per parent,
///    deduplicates, and optionally caps the beam at `beam_width`.
pub(crate) struct CoordinateSweepOperator {
    keep_per_unit: usize,
    beam_width: Option<usize>,
}

impl CoordinateSweepOperator {
    pub(crate) fn from_value(value: &serde_json::Value) -> Result<Self, Failure> {
        let config: CoordinateSweepConfig = parse_params("placement.coordinate_sweep", value)?;
        if config.keep_per_unit == 0 {
            return Err(Failure::refused(
                "placement.coordinate_sweep requires keep_per_unit >= 1",
            ));
        }
        if config.beam_width == Some(0) {
            return Err(Failure::refused(
                "placement.coordinate_sweep requires beam_width >= 1",
            ));
        }
        Ok(Self {
            keep_per_unit: config.keep_per_unit,
            beam_width: config.beam_width,
        })
    }

    fn sweep_one_unit(
        &self,
        ctx: &mut SolveContext<'_>,
        beam: &[BranchId],
        unit_index: i32,
    ) -> Result<Vec<BranchId>, Failure> {
        let constraints = ctx.active_constraints();
        let mut prepared = Vec::with_capacity(beam.len());
        for &parent in beam {
            let branch = ctx.arena.get(parent);
            let movable = branch
                .state
                .active_units()
                .find(|entry| entry.index == unit_index)
                .is_some_and(|entry| entry.movable(&branch.state));
            if !movable {
                prepared.push(None);
                continue;
            }
            let mask = ctx.services.space.move_mask(
                ctx.problem.round,
                ctx.problem.side.red(),
                &branch.state,
                unit_index,
            )?;
            prepared.push(Some((
                parent,
                branch.state.clone(),
                branch.unpinned.clone(),
                mask,
            )));
        }

        let proposals_by_slot = propose_prepared(ctx, &prepared, unit_index)?;
        let mut per_parent_children: Vec<Vec<BranchId>> = Vec::with_capacity(beam.len());

        for (slot_idx, slot) in prepared.into_iter().enumerate() {
            let Some((parent, _, _, mask)) = slot else {
                per_parent_children.push(vec![beam[slot_idx]]);
                continue;
            };
            let children = fork_proposals(
                ctx,
                self.name(),
                parent,
                unit_index,
                &mask,
                &constraints,
                &proposals_by_slot[slot_idx],
            );
            per_parent_children.push(children);
        }

        ctx.evaluate_batch(&mut per_parent_children)?;

        let mut next_beam = Vec::new();
        for mut children in per_parent_children {
            sort_by_valuation(ctx, &mut children);
            children.truncate(self.keep_per_unit);
            next_beam.extend(children);
        }
        let mut deduped = dedup_beam(ctx, next_beam)?;
        if let Some(width) = self.beam_width
            && deduped.len() > width
        {
            sort_by_valuation(ctx, &mut deduped);
            deduped.truncate(width);
        }
        Ok(deduped)
    }
}

impl Operator for CoordinateSweepOperator {
    fn name(&self) -> &'static str {
        "placement.coordinate_sweep"
    }

    fn apply(
        &self,
        ctx: &mut SolveContext<'_>,
        mut beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        let mut unit_indices = BTreeSet::new();
        for &id in &beam {
            let state = &ctx.arena.get(id).state;
            for unit in state.active_units().filter(|u| u.movable(state)) {
                unit_indices.insert(unit.index);
            }
        }
        for unit_index in unit_indices {
            if ctx.check_stop().is_some() {
                break;
            }
            beam = self.sweep_one_unit(ctx, &beam, unit_index)?;
        }
        Ok(beam)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PredictUnpinnedConfig {
    #[serde(default = "default_max_branches")]
    max_branches: usize,
}

const fn default_max_branches() -> usize {
    64
}

/// Autoregressively predicts placements for every `unpinned` formation using
/// the mounted `SpaceService`, `proposers`, and `filters`, capping beam width
/// by cumulative prior.
pub(crate) struct PredictUnpinnedPlacement {
    max_branches: usize,
}

impl PredictUnpinnedPlacement {
    pub(crate) fn from_value(value: &serde_json::Value) -> Result<Self, Failure> {
        let config: PredictUnpinnedConfig = parse_params("placement.predict_unpinned", value)?;
        if config.max_branches == 0 {
            return Err(Failure::refused(
                "placement.predict_unpinned requires max_branches >= 1",
            ));
        }
        Ok(Self {
            max_branches: config.max_branches,
        })
    }
}

impl Operator for PredictUnpinnedPlacement {
    fn name(&self) -> &'static str {
        "placement.predict_unpinned"
    }

    fn apply(
        &self,
        ctx: &mut SolveContext<'_>,
        mut current: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        let constraints = ctx.active_constraints();
        loop {
            let next_unit = current
                .iter()
                .filter_map(|&id| ctx.arena.get(id).unpinned.iter().next().copied())
                .min();
            let Some(unit_index) = next_unit else {
                return Ok(current);
            };
            let mut prepared = Vec::with_capacity(current.len());
            for &parent in &current {
                let branch = ctx.arena.get(parent);
                if !branch.unpinned.contains(&unit_index) {
                    prepared.push(None);
                    continue;
                }
                let mask = ctx.services.space.move_mask(
                    ctx.problem.round,
                    ctx.problem.side.red(),
                    &branch.state,
                    unit_index,
                )?;
                prepared.push(Some((
                    parent,
                    branch.state.clone(),
                    branch.unpinned.clone(),
                    mask,
                )));
            }
            let proposals_by_slot = propose_prepared(ctx, &prepared, unit_index)?;
            let mut next_beam = Vec::new();
            for (slot_idx, slot) in prepared.into_iter().enumerate() {
                let Some((parent, _, _, mask)) = slot else {
                    next_beam.push(current[slot_idx]);
                    continue;
                };
                next_beam.extend(fork_proposals(
                    ctx,
                    self.name(),
                    parent,
                    unit_index,
                    &mask,
                    &constraints,
                    &proposals_by_slot[slot_idx],
                ));
            }
            next_beam = dedup_beam(ctx, next_beam)?;
            if next_beam.len() > self.max_branches {
                next_beam.sort_by(|left, right| {
                    let a = ctx.arena.get(*left);
                    let b = ctx.arena.get(*right);
                    b.prior.total_cmp(&a.prior).then_with(|| a.id.cmp(&b.id))
                });
                next_beam.truncate(self.max_branches);
            }
            current = next_beam;
        }
    }
}

type PreparedSlot = Option<(
    BranchId,
    mechcore_document::r#match::SideState,
    BTreeSet<i32>,
    Mask,
)>;

fn propose_prepared(
    ctx: &SolveContext<'_>,
    prepared: &[PreparedSlot],
    unit_index: i32,
) -> Result<Vec<Vec<PlacementProposal>>, Failure> {
    let mut queries = Vec::new();
    let mut query_slots = Vec::new();
    for (slot_idx, slot) in prepared.iter().enumerate() {
        if let Some((_, state, unpinned, mask)) = slot {
            queries.push(PlacementQuery {
                round: ctx.problem.round,
                side: ctx.problem.side,
                state,
                opponent_opening: ctx.problem.opponent_opening(),
                unit_index,
                unpinned,
                legal_mask: mask,
            });
            query_slots.push(slot_idx);
        }
    }
    let mut result = vec![Vec::new(); prepared.len()];
    if !queries.is_empty() {
        let batch = ctx.propose_batch(&queries)?;
        for (slot_idx, proposals) in query_slots.into_iter().zip(batch) {
            result[slot_idx] = proposals;
        }
    }
    Ok(result)
}

fn fork_proposals(
    ctx: &mut SolveContext<'_>,
    operator_name: &str,
    parent: BranchId,
    unit_index: i32,
    mask: &Mask,
    constraints: &Constraints,
    proposals: &[PlacementProposal],
) -> Vec<BranchId> {
    let (state, unpinned, parent_prior, history) = {
        let branch = ctx.arena.get(parent);
        (
            branch.state.clone(),
            branch.unpinned.clone(),
            branch.prior,
            branch.history.clone(),
        )
    };
    let Some(unit) = state.active_units().find(|entry| entry.index == unit_index) else {
        let mut remaining = unpinned;
        remaining.remove(&unit_index);
        return vec![ctx.arena.fork(
            parent,
            state,
            remaining,
            parent_prior,
            IntentRecord {
                operator: operator_name.to_owned(),
                summary: format!("skip missing formation #{unit_index}"),
            },
            history,
        )];
    };
    let Some(current_pos) = unit.position else {
        return Vec::new();
    };
    let current_rot = unit.rotated == Some(true);
    let type_name = unit.type_name.clone();

    let mut children = Vec::new();
    for proposal in proposals {
        if let Some(child) = apply_proposal(
            ctx,
            operator_name,
            parent,
            &state,
            &unpinned,
            parent_prior,
            &history,
            unit_index,
            &type_name,
            current_pos,
            current_rot,
            mask,
            constraints,
            proposal,
        ) {
            children.push(child);
        }
    }
    if children.is_empty() {
        let mut next_unpinned = unpinned;
        next_unpinned.remove(&unit_index);
        children.push(ctx.arena.fork(
            parent,
            state,
            next_unpinned,
            parent_prior,
            IntentRecord {
                operator: operator_name.to_owned(),
                summary: format!(
                    "unit #{unit_index} ({type_name}): stay at ({}, {}) (proposals blocked)",
                    current_pos.x, current_pos.y
                ),
            },
            history,
        ));
    }
    children
}

#[allow(clippy::too_many_arguments)]
fn apply_proposal(
    ctx: &mut SolveContext<'_>,
    operator_name: &str,
    parent: BranchId,
    state: &mechcore_document::r#match::SideState,
    unpinned: &BTreeSet<i32>,
    parent_prior: f64,
    history: &[Action],
    unit_index: i32,
    type_name: &str,
    current_pos: Position,
    current_rot: bool,
    mask: &Mask,
    constraints: &Constraints,
    proposal: &PlacementProposal,
) -> Option<BranchId> {
    let changed = proposal.position != current_pos || proposal.rotated != current_rot;
    let mut next_unpinned = unpinned.clone();
    next_unpinned.remove(&unit_index);
    let next_prior = parent_prior + proposal.prior.max(1e-9).ln();
    if !changed {
        return Some(ctx.arena.fork(
            parent,
            state.clone(),
            next_unpinned,
            next_prior,
            IntentRecord {
                operator: operator_name.to_owned(),
                summary: format!(
                    "unit #{unit_index} ({type_name}): {} at ({}, {})",
                    proposal.label, proposal.position.x, proposal.position.y
                ),
            },
            history.to_vec(),
        ));
    }
    if !ctx
        .services
        .space
        .can_place_move(mask, proposal.position, proposal.rotated, constraints)
    {
        return None;
    }
    let action = Action::MoveUnit {
        index: unit_index,
        position: proposal.position,
        rotated: proposal.rotated,
    };
    let mut placement = landing::placement(ctx.problem.side.red());
    let next_state = transition::step_placing(
        ctx.problem.economy,
        state,
        &action,
        ctx.problem.offers(),
        &mut placement,
    )
    .ok()?;
    let mut next_history = history.to_vec();
    next_history.push(action);
    Some(ctx.arena.fork(
        parent,
        next_state,
        next_unpinned,
        next_prior,
        IntentRecord {
            operator: operator_name.to_owned(),
            summary: format!(
                "unit #{unit_index} ({type_name}): {} -> ({}, {})",
                proposal.label, proposal.position.x, proposal.position.y
            ),
        },
        next_history,
    ))
}

/// Deduplicates branches with identical `(SideState, unpinned)` keys, keeping
/// the branch with the highest valuation (or prior if unevaluated).
pub(crate) struct DedupOperator;

impl Operator for DedupOperator {
    fn name(&self) -> &'static str {
        "flow.dedup"
    }

    fn apply(
        &self,
        ctx: &mut SolveContext<'_>,
        beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        dedup_beam(ctx, beam)
    }
}

fn dedup_beam(ctx: &SolveContext<'_>, beam: Vec<BranchId>) -> Result<Vec<BranchId>, Failure> {
    let mut best: BTreeMap<([u8; 32], BTreeSet<i32>), BranchId> = BTreeMap::new();
    let mut order = Vec::new();
    for id in beam {
        let branch = ctx.arena.get(id);
        let digest = state_digest(&branch.state)?;
        let key = (digest, branch.unpinned.clone());
        match best.entry(key.clone()) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(id);
                order.push(key);
            }
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                let existing = ctx.arena.get(*slot.get());
                let better = match (branch.valuation, existing.valuation) {
                    (Some(a), Some(b)) => a.score > b.score,
                    (Some(_), None) => true,
                    (None, Some(_)) => false,
                    (None, None) => branch.prior > existing.prior,
                };
                if better {
                    slot.insert(id);
                }
            }
        }
    }
    Ok(order.into_iter().map(|key| best[&key]).collect())
}

/// Scores as many branches as the shared simulation budget permits.
pub(crate) struct EvalScoreOperator;

impl Operator for EvalScoreOperator {
    fn name(&self) -> &'static str {
        "eval.score"
    }

    fn apply(
        &self,
        ctx: &mut SolveContext<'_>,
        mut beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        ctx.evaluate_batch(std::slice::from_mut(&mut beam))?;
        Ok(beam)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalTopKConfig {
    #[serde(default = "default_top_k")]
    top_k: usize,
}

const fn default_top_k() -> usize {
    3
}

/// Retains the top-`k` scored branches within the shared simulation budget.
pub(crate) struct EvalTopKOperator {
    top_k: usize,
}

impl EvalTopKOperator {
    pub(crate) fn from_value(value: &serde_json::Value) -> Result<Self, Failure> {
        let config: EvalTopKConfig = parse_params("eval.top_k", value)?;
        if config.top_k == 0 {
            return Err(Failure::refused("eval.top_k requires top_k >= 1"));
        }
        Ok(Self {
            top_k: config.top_k,
        })
    }
}

impl Operator for EvalTopKOperator {
    fn name(&self) -> &'static str {
        "eval.top_k"
    }

    fn apply(
        &self,
        ctx: &mut SolveContext<'_>,
        mut beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        ctx.evaluate_batch(std::slice::from_mut(&mut beam))?;
        sort_by_valuation(ctx, &mut beam);
        if beam.len() > self.top_k {
            let pruned: Vec<PrunedCandidate> = beam
                .iter()
                .skip(self.top_k)
                .take(5)
                .map(|&pruned_id| {
                    let branch = ctx.arena.get(pruned_id);
                    let val = branch.valuation.expect("evaluated");
                    PrunedCandidate {
                        node_id: branch.id.0,
                        score: val.score,
                        own_core_damage: val.own_core_damage,
                        opponent_core_damage: val.opponent_core_damage,
                        intent: branch.intent.summary.clone(),
                    }
                })
                .collect();
            ctx.emit(&SearchEvent::CandidatesPruned(&pruned));
            beam.truncate(self.top_k);
        }
        Ok(beam)
    }
}

fn sort_by_valuation(ctx: &SolveContext<'_>, beam: &mut [BranchId]) {
    beam.sort_by(|left, right| {
        let a = ctx.arena.get(*left);
        let b = ctx.arena.get(*right);
        let va: Valuation = a.valuation.expect("evaluated");
        let vb: Valuation = b.valuation.expect("evaluated");
        vb.score
            .total_cmp(&va.score)
            .then_with(|| vb.min_core_delta.cmp(&va.min_core_delta))
            .then_with(|| vb.opponent_core_damage.total_cmp(&va.opponent_core_damage))
            .then_with(|| va.ticks.cmp(&vb.ticks))
            .then_with(|| a.id.cmp(&b.id))
    });
}

fn parse_params<T: for<'de> Deserialize<'de>>(
    plugin: &str,
    value: &serde_json::Value,
) -> Result<T, Failure> {
    let normalized = if value.is_null() {
        serde_json::json!({})
    } else {
        value.clone()
    };
    serde_json::from_value(normalized)
        .map_err(|error| Failure::refused(format!("invalid parameters for {plugin}: {error}")))
}
