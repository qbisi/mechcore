//! Built-in Cordis-style typed hook plugins (`guards`, `filters`, `observers`).

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use mechcore_document::Position;
use serde::Deserialize;

use crate::cli::Failure;

use super::{
    model::{
        Constraints, ProposalFilter, PrunedCandidate, SearchEvent, SearchObserver, SearchStats,
        StageTrace, StopGuard,
    },
    predictor::{PlacementProposal, PlacementQuery},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MaxSimulationsConfig {
    limit: usize,
}

/// `bail` guard halting loops once total simulations reach `limit`.
pub(crate) struct MaxSimulationsGuard {
    limit: usize,
}

impl MaxSimulationsGuard {
    pub(crate) fn from_value(value: &serde_json::Value) -> Result<Self, Failure> {
        let config: MaxSimulationsConfig =
            serde_json::from_value(value.clone()).map_err(|error| {
                Failure::refused(format!(
                    "invalid parameters for guard.max_simulations: {error}"
                ))
            })?;
        if config.limit == 0 {
            return Err(Failure::refused(
                "guard.max_simulations requires limit >= 1",
            ));
        }
        Ok(Self {
            limit: config.limit,
        })
    }
}

impl StopGuard for MaxSimulationsGuard {
    fn simulation_limit(&self) -> Option<usize> {
        Some(self.limit)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StagnationConfig {
    #[serde(default = "default_patience")]
    patience_sweeps: usize,
}

const fn default_patience() -> usize {
    1
}

/// `bail` guard halting `repeat` loops after `patience_sweeps` sweeps without
/// strict score improvement.
pub(crate) struct StagnationGuard {
    patience_sweeps: usize,
}

impl StagnationGuard {
    pub(crate) fn from_value(value: &serde_json::Value) -> Result<Self, Failure> {
        let normalized = if value.is_null() {
            serde_json::json!({})
        } else {
            value.clone()
        };
        let config: StagnationConfig = serde_json::from_value(normalized).map_err(|error| {
            Failure::refused(format!("invalid parameters for guard.stagnation: {error}"))
        })?;
        if config.patience_sweeps == 0 {
            return Err(Failure::refused(
                "guard.stagnation requires patience_sweeps >= 1",
            ));
        }
        Ok(Self {
            patience_sweeps: config.patience_sweeps,
        })
    }
}

impl StopGuard for StagnationGuard {
    fn check(&self, stats: &SearchStats) -> Option<String> {
        (stats.stagnant_sweeps >= self.patience_sweeps).then(|| {
            format!(
                "no improvement for {} consecutive sweep(s)",
                stats.stagnant_sweeps
            )
        })
    }
}

/// `waterfall` filter deduplicating proposals that name the same `(position, rotated)`,
/// keeping the proposal with the highest prior.
pub(crate) struct DedupPosesFilter;

impl ProposalFilter for DedupPosesFilter {
    fn filter(
        &self,
        _query: &PlacementQuery<'_>,
        _constraints: &Constraints,
        proposals: Vec<PlacementProposal>,
    ) -> Vec<PlacementProposal> {
        let mut best: BTreeMap<(i32, i32, bool), PlacementProposal> = BTreeMap::new();
        let mut order = Vec::new();
        for proposal in proposals {
            let key = (proposal.position.x, proposal.position.y, proposal.rotated);
            match best.entry(key) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(proposal);
                    order.push(key);
                }
                std::collections::btree_map::Entry::Occupied(mut slot) => {
                    if proposal.prior > slot.get().prior {
                        slot.insert(proposal);
                    }
                }
            }
        }
        order
            .into_iter()
            .filter_map(|key| best.remove(&key))
            .collect()
    }
}

/// `waterfall` filter dropping proposals outside active scope [`Constraints`].
pub(crate) struct ScopeBoundsFilter;

impl ProposalFilter for ScopeBoundsFilter {
    fn filter(
        &self,
        _query: &PlacementQuery<'_>,
        constraints: &Constraints,
        proposals: Vec<PlacementProposal>,
    ) -> Vec<PlacementProposal> {
        proposals
            .into_iter()
            .filter(|proposal| {
                constraints.admits(Position {
                    x: proposal.position.x,
                    y: proposal.position.y,
                })
            })
            .collect()
    }
}

/// Shared trace buffer populated by [`TraceObserver`].
#[derive(Clone, Debug, Default)]
pub(crate) struct TraceBuffer {
    pub(crate) stages: Vec<StageTrace>,
    pub(crate) pruned_top: Vec<PrunedCandidate>,
    pub(crate) stop_reason: Option<String>,
}

/// `emit` observer recording stage funnel traces and pruned near-misses.
pub(crate) struct TraceObserver {
    buffer: Arc<Mutex<TraceBuffer>>,
}

impl TraceObserver {
    #[must_use]
    pub(crate) const fn new(buffer: Arc<Mutex<TraceBuffer>>) -> Self {
        Self { buffer }
    }
}

impl SearchObserver for TraceObserver {
    fn on_event(&mut self, event: &SearchEvent<'_>) {
        let Ok(mut guard) = self.buffer.lock() else {
            return;
        };
        match event {
            SearchEvent::StageFinished(stage) => guard.stages.push(stage.clone()),
            SearchEvent::CandidatesPruned(pruned) => guard.pruned_top.extend_from_slice(pruned),
            SearchEvent::Stopped(reason) => guard.stop_reason = Some((*reason).to_owned()),
        }
    }
}
