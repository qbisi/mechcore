//! Core state, branch arena, capability seams, and plugin traits for `solve`.

use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
};

use mechcore_document::{
    economy::Economy,
    r#match::{Action, Offers, SideState, State},
    normalize, project,
};
use serde::Serialize;

use crate::{cli::Failure, outcome, turn::Side};

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

/// Deterministic fight evaluation metrics from the solver side's perspective.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Valuation {
    /// Own reactor core change minus opponent reactor core change:
    /// `(-own_core_damage) - (-opponent_core_damage) = opponent_core_damage - own_core_damage`.
    pub(crate) core_delta: i32,
    pub(crate) own_core_damage: i32,
    pub(crate) opponent_core_damage: i32,
    pub(crate) ticks: u32,
    pub(crate) winner: &'static str,
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
    /// when a warm-started human replay contains non-rational economic orderings
    /// that pure endpoint normalization rejects.
    pub(crate) history: Vec<Action>,
}

/// One step along the winning lineage back to the round opening.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct LineageStep {
    pub(crate) node_id: usize,
    pub(crate) operator: String,
    pub(crate) summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) score: Option<i32>,
}

/// Per-stage funnel statistics recorded for observability.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct StageTrace {
    pub(crate) index: usize,
    pub(crate) operator: String,
    pub(crate) in_beam: usize,
    pub(crate) out_beam: usize,
    pub(crate) simulations: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) best_score: Option<i32>,
}

/// A high-scoring alternative pruned by an evaluation stage.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct PrunedCandidate {
    pub(crate) node_id: usize,
    pub(crate) score: i32,
    pub(crate) own_core_damage: i32,
    pub(crate) opponent_core_damage: i32,
    pub(crate) intent: String,
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
                score: branch.valuation.map(|valuation| valuation.core_delta),
            });
            cursor = branch.parent;
        }
        steps.reverse();
        steps
    }
}

/// Read-only context handed to an [`OpponentBelief`] plugin before the pipeline runs.
pub(crate) struct OpponentContext<'a> {
    pub(crate) economy: &'a Economy,
    pub(crate) opponent_side: Side,
    pub(crate) opponent_opening: &'a SideState,
    pub(crate) opponent_actions: &'a [Action],
    pub(crate) offers: Option<&'a Offers>,
}

/// Plugin resolving the sealed opponent state used by fight evaluation.
pub(crate) trait OpponentBelief: Send + Sync {
    /// Resolves the opponent's deployed state for evaluation.
    ///
    /// # Errors
    /// Returns a refusal when the opponent belief cannot be materialized.
    fn resolve(&self, context: &OpponentContext<'_>) -> Result<SideState, Failure>;
}

/// Pipeline stage plugin transforming a beam of candidate branches into a new beam.
pub(crate) trait Operator: Send + Sync {
    fn name(&self) -> &'static str;

    /// Applies this operator to `beam` using the host's [`KernelSeams`].
    ///
    /// # Errors
    /// Returns a refusal or failure when the stage cannot be executed.
    fn apply(
        &self,
        seams: &mut KernelSeams<'_>,
        beam: Vec<BranchId>,
    ) -> Result<Vec<BranchId>, Failure>;
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

/// Host capability seams exposed to pipeline operators.
///
/// The opponent's deployed state is held privately inside this struct so
/// proposal/placement operators can only inspect `opponent_opening()` and
/// receive fight scores through `evaluate_batch()`.
pub(crate) struct KernelSeams<'a> {
    pub(crate) economy: &'a Economy,
    pub(crate) round: i32,
    pub(crate) side: Side,
    pub(crate) map_id: i32,
    pub(crate) seed: i32,
    pub(crate) opening: &'a State,
    pub(crate) own_actions: &'a [Action],
    opponent_deployed: SideState,
    pub(crate) arena: Arena,
    sim_cache: BTreeMap<[u8; 32], Valuation>,
    pub(crate) simulations: usize,
    pub(crate) pruned_top: Vec<PrunedCandidate>,
}

impl<'a> KernelSeams<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        economy: &'a Economy,
        round: i32,
        side: Side,
        map_id: i32,
        seed: i32,
        opening: &'a State,
        own_actions: &'a [Action],
        opponent_deployed: SideState,
    ) -> Self {
        Self {
            economy,
            round,
            side,
            map_id,
            seed,
            opening,
            own_actions,
            opponent_deployed,
            arena: Arena::default(),
            sim_cache: BTreeMap::new(),
            simulations: 0,
            pruned_top: Vec::new(),
        }
    }

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

    /// Evaluates one candidate state against the sealed opponent belief.
    ///
    /// # Errors
    /// Returns a refusal if the state cannot be projected or simulated.
    pub(crate) fn evaluate_state(&self, own_state: &SideState) -> Result<Valuation, Failure> {
        let mut combined = self.opening.clone();
        match self.side {
            Side::Blue => {
                combined.blue = own_state.clone();
                combined.red = self.opponent_deployed.clone();
            }
            Side::Red => {
                combined.blue = self.opponent_deployed.clone();
                combined.red = own_state.clone();
            }
        }
        let layout = project::project(self.opening, &combined, self.round, self.map_id, self.seed)
            .map_err(Failure::refused)?;
        let yaml = mechcore_document::canonical_yaml(layout).map_err(Failure::refused)?;
        let simulated = mechcore_simulation::simulate_document(
            yaml.as_bytes(),
            mechcore_simulation::Record::Memory,
            None,
        )
        .map_err(|error| Failure::unsimulated(&error))?;
        let recording = simulated
            .recording
            .ok_or_else(|| Failure::failed("simulator did not keep in-memory recording"))?;
        let fight = outcome::read(&recording)?.fight()?;
        let (own_core_damage, opponent_core_damage) = match self.side {
            Side::Blue => (fight.blue.core_damage, fight.red.core_damage),
            Side::Red => (fight.red.core_damage, fight.blue.core_damage),
        };
        let core_delta = opponent_core_damage - own_core_damage;
        let winner = match fight.blue.core_damage.cmp(&fight.red.core_damage) {
            std::cmp::Ordering::Less => "blue",
            std::cmp::Ordering::Greater => "red",
            std::cmp::Ordering::Equal => "draw",
        };
        let ticks = fight
            .trajectory
            .as_ref()
            .map_or(0, |trajectory| trajectory.ticks);
        Ok(Valuation {
            core_delta,
            own_core_damage,
            opponent_core_damage,
            ticks,
            winner,
        })
    }

    /// Evaluates one state and caches its valuation so identical search
    /// branches reuse the fight result without re-simulating.
    ///
    /// # Errors
    /// Returns a refusal if the state cannot be hashed, projected, or simulated.
    pub(crate) fn evaluate_cached(&mut self, own_state: &SideState) -> Result<Valuation, Failure> {
        let digest = state_digest(own_state)?;
        if let Some(&valuation) = self.sim_cache.get(&digest) {
            return Ok(valuation);
        }
        let valuation = self.evaluate_state(own_state)?;
        self.sim_cache.insert(digest, valuation);
        self.simulations += 1;
        Ok(valuation)
    }

    /// Evaluates every branch in `ids`, deduplicating identical states and running
    /// cache misses in parallel across available CPU cores.
    ///
    /// # Errors
    /// Returns the first projection or simulation failure encountered.
    pub(crate) fn evaluate_batch(&mut self, ids: &[BranchId]) -> Result<usize, Failure> {
        let mut digests = Vec::with_capacity(ids.len());
        let mut unique_misses: Vec<([u8; 32], SideState)> = Vec::new();
        let mut seen_misses = BTreeSet::new();

        for &id in ids {
            let state = &self.arena.get(id).state;
            let digest = state_digest(state)?;
            digests.push((id, digest));
            if !self.sim_cache.contains_key(&digest) && seen_misses.insert(digest) {
                unique_misses.push((digest, state.clone()));
            }
        }

        let executed = unique_misses.len();
        if !unique_misses.is_empty() {
            let workers = std::thread::available_parallelism()
                .map_or(1, NonZeroUsize::get)
                .min(unique_misses.len());
            let chunk_size = unique_misses.len().div_ceil(workers);
            let evaluated = std::thread::scope(|scope| {
                let mut handles = Vec::new();
                for chunk in unique_misses.chunks(chunk_size) {
                    let seams = &*self;
                    handles.push(scope.spawn(move || {
                        let mut results = Vec::with_capacity(chunk.len());
                        for (digest, state) in chunk {
                            let valuation = seams.evaluate_state(state)?;
                            results.push((*digest, valuation));
                        }
                        Ok::<_, Failure>(results)
                    }));
                }
                let mut merged = Vec::with_capacity(unique_misses.len());
                for handle in handles {
                    let part = handle
                        .join()
                        .map_err(|_| Failure::failed("simulation worker thread panicked"))??;
                    merged.extend(part);
                }
                Ok::<_, Failure>(merged)
            })?;
            for (digest, valuation) in evaluated {
                self.sim_cache.insert(digest, valuation);
            }
            self.simulations += executed;
        }

        for (id, digest) in digests {
            let valuation = self.sim_cache[&digest];
            self.arena.get_mut(id).valuation = Some(valuation);
        }
        Ok(executed)
    }

    /// Compiles a branch's closing state into a legal action list from round opening.
    ///
    /// Prefers pure two-endpoint normalization (`normalize::deployment`), falling
    /// back to compacting `branch.history` if a warm-started human replay carried
    /// non-rational economic orderings.
    ///
    /// # Errors
    /// Returns a refusal if neither normalization nor history compaction succeeds.
    pub(crate) fn compile_actions(&self, branch: &Branch) -> Result<(Vec<Action>, bool), Failure> {
        match normalize::deployment(
            self.economy,
            self.own_opening(),
            &branch.state,
            self.offers(),
            self.side.red(),
        ) {
            Ok(actions) => Ok((actions, true)),
            Err(error) if !branch.history.is_empty() => normalize::historical_deployment(
                self.economy,
                self.own_opening(),
                &branch.state,
                self.side.red(),
                self.offers(),
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
