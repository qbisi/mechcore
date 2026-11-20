//! Built-in opponent belief and pipeline operator plugins for `solve`.

use std::collections::{BTreeMap, BTreeSet};

use mechcore_document::{Region, landing, r#match::Action, transition};
use serde::Deserialize;

use crate::cli::Failure;

use super::{
    model::{
        BranchId, IntentRecord, KernelSeams, Operator, OpponentBelief, OpponentContext,
        PrunedCandidate, state_digest,
    },
    predictor::{self, PlacementPredictor, PlacementQuery},
};

/// Opponent belief plugin that replays the opponent's recorded match actions.
pub(crate) struct ReplayTruthOpponent;

impl OpponentBelief for ReplayTruthOpponent {
    fn resolve(
        &self,
        context: &OpponentContext<'_>,
    ) -> Result<mechcore_document::r#match::SideState, Failure> {
        transition::deployed(
            context.economy,
            context.opponent_opening,
            context.opponent_actions,
            context.opponent_side.red(),
            context.offers,
        )
        .map_err(|error| {
            Failure::refused(format!(
                "opponent.replay_truth cannot replay {} actions: {error}",
                context.opponent_side.name()
            ))
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WarmStartConfig {
    #[serde(default = "default_true")]
    pub(crate) unpin_all_movable: bool,
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
        seams: &mut KernelSeams<'_>,
        beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        let effective: Vec<Action> = seams
            .own_actions
            .iter()
            .filter(|action| !matches!(action, Action::Concede))
            .cloned()
            .collect();
        let mut next = Vec::with_capacity(beam.len());
        for parent in beam {
            let base_state = seams.arena.get(parent).state.clone();
            let base_prior = seams.arena.get(parent).prior;
            let mut history = seams.arena.get(parent).history.clone();
            let deployed = transition::deployed(
                seams.economy,
                &base_state,
                &effective,
                seams.side.red(),
                seams.offers(),
            )
            .map_err(|error| {
                Failure::refused(format!(
                    "warm_start.own_actions cannot replay {} actions: {error}",
                    seams.side.name()
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
            let child = seams.arena.fork(
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
pub(crate) struct PredictPlacementConfig {
    #[serde(default = "default_predictor")]
    pub(crate) predictor: String,
    #[serde(default)]
    pub(crate) offsets: Option<Vec<i32>>,
    #[serde(default = "default_max_branches")]
    pub(crate) max_branches: usize,
}

fn default_predictor() -> String {
    "shift_stub".to_owned()
}

const fn default_max_branches() -> usize {
    64
}

/// Sequentially predicts placements for every `unpinned` formation using a
/// [`PlacementPredictor`], filtering out colliding or out-of-region poses.
pub(crate) struct PredictUnpinnedPlacement {
    predictor: Box<dyn PlacementPredictor>,
    max_branches: usize,
}

impl PredictUnpinnedPlacement {
    pub(crate) fn from_value(value: &serde_json::Value) -> Result<Self, Failure> {
        let config: PredictPlacementConfig = parse_params("placement.predict_unpinned", value)?;
        if config.max_branches == 0 {
            return Err(Failure::refused(
                "placement.predict_unpinned requires max_branches >= 1",
            ));
        }
        let predictor = predictor::build(&config.predictor, config.offsets)?;
        Ok(Self {
            predictor,
            max_branches: config.max_branches,
        })
    }

    fn expand_unit(
        &self,
        seams: &mut KernelSeams<'_>,
        parent: BranchId,
        unit_index: i32,
    ) -> Vec<BranchId> {
        let (state, unpinned, parent_prior, history) = {
            let branch = seams.arena.get(parent);
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
            let child = seams.arena.fork(
                parent,
                state,
                remaining,
                parent_prior,
                IntentRecord {
                    operator: self.name().to_owned(),
                    summary: format!("skip missing formation #{unit_index}"),
                },
                history,
            );
            return vec![child];
        };
        let Some(current_pos) = unit.position else {
            return Vec::new();
        };
        let current_rot = unit.rotated == Some(true);
        let type_name = unit.type_name.clone();
        let query = PlacementQuery {
            round: seams.round,
            side: seams.side,
            state: &state,
            opponent_opening: seams.opponent_opening(),
            unit_index,
            unpinned: &unpinned,
        };
        let proposals = self.predictor.predict(&query);
        let mut children = Vec::new();
        for proposal in proposals {
            if let Some(child) = self.apply_proposal(
                seams,
                parent,
                &state,
                &unpinned,
                parent_prior,
                &history,
                unit_index,
                &type_name,
                current_pos,
                current_rot,
                &proposal,
            ) {
                children.push(child);
            }
        }
        if children.is_empty() {
            let mut next_unpinned = unpinned;
            next_unpinned.remove(&unit_index);
            let child = seams.arena.fork(
                parent,
                state,
                next_unpinned,
                parent_prior,
                IntentRecord {
                    operator: self.name().to_owned(),
                    summary: format!(
                        "unit #{unit_index} ({type_name}): stay at ({}, {}) (proposals blocked)",
                        current_pos.x, current_pos.y
                    ),
                },
                history,
            );
            children.push(child);
        }
        children
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_proposal(
        &self,
        seams: &mut KernelSeams<'_>,
        parent: BranchId,
        state: &mechcore_document::r#match::SideState,
        unpinned: &BTreeSet<i32>,
        parent_prior: f64,
        history: &[Action],
        unit_index: i32,
        type_name: &str,
        current_pos: mechcore_document::Position,
        current_rot: bool,
        proposal: &predictor::PlacementProposal,
    ) -> Option<BranchId> {
        if seams.round == 1 && Region::of(proposal.position).is_flank() {
            return None;
        }
        let changed = proposal.position != current_pos || proposal.rotated != current_rot;
        let mut next_unpinned = unpinned.clone();
        next_unpinned.remove(&unit_index);
        let next_prior = parent_prior + proposal.prior.max(1e-9).ln();
        if !changed {
            return Some(seams.arena.fork(
                parent,
                state.clone(),
                next_unpinned,
                next_prior,
                IntentRecord {
                    operator: self.name().to_owned(),
                    summary: format!(
                        "unit #{unit_index} ({type_name}): {} at ({}, {})",
                        proposal.label, proposal.position.x, proposal.position.y
                    ),
                },
                history.to_vec(),
            ));
        }
        let action = Action::MoveUnit {
            index: unit_index,
            position: proposal.position,
            rotated: proposal.rotated,
        };
        transition::check_place(state, &action).ok()?;
        let mut placement = landing::placement(seams.side.red());
        let next_state = transition::step_placing(
            seams.economy,
            state,
            &action,
            seams.offers(),
            &mut placement,
        )
        .ok()?;
        let mut next_history = history.to_vec();
        next_history.push(action);
        Some(seams.arena.fork(
            parent,
            next_state,
            next_unpinned,
            next_prior,
            IntentRecord {
                operator: self.name().to_owned(),
                summary: format!(
                    "unit #{unit_index} ({type_name}): {} -> ({}, {})",
                    proposal.label, proposal.position.x, proposal.position.y
                ),
            },
            next_history,
        ))
    }
}

impl Operator for PredictUnpinnedPlacement {
    fn name(&self) -> &'static str {
        "placement.predict_unpinned"
    }

    fn apply(
        &self,
        seams: &mut KernelSeams<'_>,
        beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        let mut current = beam;
        loop {
            if current
                .iter()
                .all(|&id| seams.arena.get(id).unpinned.is_empty())
            {
                return Ok(current);
            }
            let mut next_beam = Vec::new();
            for parent in current {
                let Some(&unit_index) = seams.arena.get(parent).unpinned.iter().next() else {
                    next_beam.push(parent);
                    continue;
                };
                next_beam.extend(self.expand_unit(seams, parent, unit_index));
            }
            next_beam = dedup_beam(seams, next_beam)?;
            if next_beam.len() > self.max_branches {
                next_beam.sort_by(|left, right| {
                    let a = seams.arena.get(*left);
                    let b = seams.arena.get(*right);
                    b.prior.total_cmp(&a.prior).then_with(|| a.id.cmp(&b.id))
                });
                next_beam.truncate(self.max_branches);
            }
            current = next_beam;
        }
    }
}

/// Deduplicates branches with identical `(SideState, unpinned)` keys, keeping
/// the branch with the highest accumulated prior.
pub(crate) struct DedupOperator;

impl Operator for DedupOperator {
    fn name(&self) -> &'static str {
        "flow.dedup"
    }

    fn apply(
        &self,
        seams: &mut KernelSeams<'_>,
        beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        dedup_beam(seams, beam)
    }
}

fn dedup_beam(seams: &KernelSeams<'_>, beam: Vec<BranchId>) -> Result<Vec<BranchId>, Failure> {
    let mut best: BTreeMap<([u8; 32], BTreeSet<i32>), BranchId> = BTreeMap::new();
    let mut order = Vec::new();
    for id in beam {
        let branch = seams.arena.get(id);
        let key = (state_digest(&branch.state)?, branch.unpinned.clone());
        match best.entry(key) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(id);
                order.push((state_digest(&branch.state)?, branch.unpinned.clone()));
            }
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                let existing = seams.arena.get(*slot.get());
                if branch.prior > existing.prior {
                    slot.insert(id);
                }
            }
        }
    }
    Ok(order.into_iter().map(|key| best[&key]).collect())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvalTopKConfig {
    #[serde(default = "default_top_k")]
    pub(crate) top_k: usize,
    #[serde(default = "default_objective")]
    pub(crate) objective: String,
}

const fn default_top_k() -> usize {
    3
}

fn default_objective() -> String {
    "core_delta".to_owned()
}

/// Evaluates every candidate branch against the sealed opponent belief and
/// retains the top-k branches maximizing `core_delta`.
pub(crate) struct EvalTopKOperator {
    top_k: usize,
}

impl EvalTopKOperator {
    pub(crate) fn from_value(value: &serde_json::Value) -> Result<Self, Failure> {
        let config: EvalTopKConfig = parse_params("eval.top_k", value)?;
        if config.top_k == 0 {
            return Err(Failure::refused("eval.top_k requires top_k >= 1"));
        }
        if config.objective != "core_delta" {
            return Err(Failure::refused(format!(
                "unsupported eval.top_k objective {:?}; expected \"core_delta\"",
                config.objective
            )));
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
        seams: &mut KernelSeams<'_>,
        mut beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure> {
        seams.evaluate_batch(&beam)?;
        beam.sort_by(|left, right| {
            let a = seams.arena.get(*left);
            let b = seams.arena.get(*right);
            let va = a.valuation.expect("evaluated");
            let vb = b.valuation.expect("evaluated");
            vb.core_delta
                .cmp(&va.core_delta)
                .then_with(|| vb.opponent_core_damage.cmp(&va.opponent_core_damage))
                .then_with(|| va.ticks.cmp(&vb.ticks))
                .then_with(|| a.id.cmp(&b.id))
        });
        if beam.len() > self.top_k {
            for &pruned_id in beam.iter().skip(self.top_k).take(5) {
                let branch = seams.arena.get(pruned_id);
                let val = branch.valuation.expect("evaluated");
                seams.pruned_top.push(PrunedCandidate {
                    node_id: branch.id.0,
                    score: val.core_delta,
                    own_core_damage: val.own_core_damage,
                    opponent_core_damage: val.opponent_core_damage,
                    intent: branch.intent.summary.clone(),
                });
            }
            beam.truncate(self.top_k);
        }
        Ok(beam)
    }
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
