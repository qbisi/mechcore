//! Built-in capability services (`opponent`, `space`, `evaluator`).

use mechcore_document::{Position, mask::Mask, r#match::SideState, project, transition};
use serde::Deserialize;

use crate::{cli::Failure, turn::Side};

use super::model::{
    Constraints, EvaluatorService, OpponentContext, OpponentHypothesis, OpponentService, Problem,
    SpaceService, Valuation,
};

/// Opponent belief service that replays the opponent's recorded match actions
/// as a single weight-`1.0` hypothesis.
pub(crate) struct ReplayTruthOpponent;

impl OpponentService for ReplayTruthOpponent {
    fn resolve(&self, context: &OpponentContext<'_>) -> Result<Vec<OpponentHypothesis>, Failure> {
        let deployed = transition::deployed(
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
        })?;
        Ok(vec![OpponentHypothesis {
            label: "replay_truth".to_owned(),
            weight: 1.0,
            state: deployed,
        }])
    }
}

/// Strict-collision spatial service backed by `mechcore_document::mask::move_unit`.
pub(crate) struct StrictGridSpace;

impl SpaceService for StrictGridSpace {
    fn move_mask(
        &self,
        round: i32,
        red: bool,
        state: &SideState,
        unit_index: i32,
    ) -> Result<Mask, Failure> {
        mechcore_document::mask::move_unit(state, round, red, unit_index).map_err(Failure::refused)
    }

    fn can_place_move(
        &self,
        mask: &Mask,
        position: Position,
        rotated: bool,
        constraints: &Constraints,
    ) -> bool {
        constraints.admits(position) && mask.contains(position, rotated)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactSimConfig {
    #[serde(default = "default_objective")]
    objective: String,
    #[serde(default)]
    risk_lambda: f64,
}

fn default_objective() -> String {
    "core_delta".to_owned()
}

/// Core damage and duration metrics from simulating one own-vs-opponent fight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FightMetrics {
    own_core_damage: i32,
    opponent_core_damage: i32,
    ticks: u32,
}

impl FightMetrics {
    const fn core_delta(self) -> i32 {
        self.opponent_core_damage - self.own_core_damage
    }
}

/// Projects and simulates `own_state` against `opponent_state` for `problem`.
fn simulate_pair(
    problem: &Problem<'_>,
    own_state: &SideState,
    opponent_state: &SideState,
) -> Result<FightMetrics, Failure> {
    let mut combined = problem.opening.clone();
    match problem.side {
        Side::Blue => {
            combined.blue = own_state.clone();
            combined.red = opponent_state.clone();
        }
        Side::Red => {
            combined.blue = opponent_state.clone();
            combined.red = own_state.clone();
        }
    }
    let layout = project::project(
        problem.opening,
        &combined,
        problem.round,
        problem.map_id,
        problem.seed,
    )
    .map_err(Failure::refused)?;
    let outcome = mechcore_simulation::simulate_layout_outcome(layout, None)
        .map_err(|error| Failure::unsimulated(&error))?;
    let (own_core_damage, opponent_core_damage) = match problem.side {
        Side::Blue => (outcome.blue_core_damage, outcome.red_core_damage),
        Side::Red => (outcome.red_core_damage, outcome.blue_core_damage),
    };
    Ok(FightMetrics {
        own_core_damage,
        opponent_core_damage,
        ticks: outcome.ticks,
    })
}

/// Simulates `(own_state, opponent_state)` fight pairs across worker threads
/// using work-stealing, returning metrics in input order.
fn simulate_pairs_parallel(
    problem: &Problem<'_>,
    pairs: &[(&SideState, &SideState)],
) -> Result<Vec<FightMetrics>, Failure> {
    if pairs.is_empty() {
        return Ok(Vec::new());
    }
    if pairs.len() == 1 {
        let (own_state, opp_state) = pairs[0];
        return Ok(vec![simulate_pair(problem, own_state, opp_state)?]);
    }
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(pairs.len());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let aborted = std::sync::atomic::AtomicBool::new(false);
    let ordered = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let next = &next;
            let aborted = &aborted;
            handles.push(scope.spawn(move || {
                let mut local = Vec::new();
                while !aborted.load(std::sync::atomic::Ordering::Relaxed) {
                    let idx = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&(own_state, opp_state)) = pairs.get(idx) else {
                        break;
                    };
                    match simulate_pair(problem, own_state, opp_state) {
                        Ok(metrics) => local.push((idx, metrics)),
                        Err(error) => {
                            aborted.store(true, std::sync::atomic::Ordering::Relaxed);
                            return Err(error);
                        }
                    }
                }
                Ok::<_, Failure>(local)
            }));
        }
        let mut ordered = vec![None; pairs.len()];
        for handle in handles {
            let part = handle
                .join()
                .map_err(|_| Failure::failed("simulation worker thread panicked"))??;
            for (idx, metrics) in part {
                ordered[idx] = Some(metrics);
            }
        }
        Ok::<_, Failure>(ordered)
    })?;
    Ok(ordered
        .into_iter()
        .map(|entry| entry.expect("every scheduled fight produced metrics"))
        .collect())
}

/// Exact simulator evaluator aggregating scores across the opponent belief set.
///
/// Objective: `(1 - risk_lambda) * E[core_delta] + risk_lambda * min(core_delta)`.
pub(crate) struct ExactSimEvaluator {
    risk_lambda: f64,
}

impl ExactSimEvaluator {
    pub(crate) fn from_value(value: &serde_json::Value) -> Result<Self, Failure> {
        let normalized = if value.is_null() {
            serde_json::json!({})
        } else {
            value.clone()
        };
        let config: ExactSimConfig = serde_json::from_value(normalized).map_err(|error| {
            Failure::refused(format!("invalid parameters for eval.sim_exact: {error}"))
        })?;
        if config.objective != "core_delta" {
            return Err(Failure::refused(format!(
                "unsupported eval.sim_exact objective {:?}; expected \"core_delta\"",
                config.objective
            )));
        }
        if !(0.0..=1.0).contains(&config.risk_lambda) {
            return Err(Failure::refused(
                "eval.sim_exact risk_lambda must be within [0.0, 1.0]",
            ));
        }
        Ok(Self {
            risk_lambda: config.risk_lambda,
        })
    }

    fn score_metrics(
        &self,
        side: Side,
        opponents: &[OpponentHypothesis],
        metrics: &[FightMetrics],
    ) -> Result<Valuation, Failure> {
        let mut weight_sum = 0.0_f64;
        let mut weighted_delta = 0.0_f64;
        let mut weighted_own_dmg = 0.0_f64;
        let mut weighted_opp_dmg = 0.0_f64;
        let mut min_core_delta = i32::MAX;
        let mut max_ticks = 0_u32;

        for (hypothesis, &metric) in opponents.iter().zip(metrics) {
            let delta = metric.core_delta();
            min_core_delta = min_core_delta.min(delta);
            weight_sum += hypothesis.weight;
            weighted_delta += hypothesis.weight * f64::from(delta);
            weighted_own_dmg += hypothesis.weight * f64::from(metric.own_core_damage);
            weighted_opp_dmg += hypothesis.weight * f64::from(metric.opponent_core_damage);
            max_ticks = max_ticks.max(metric.ticks);
        }

        if weight_sum <= 0.0 {
            return Err(Failure::refused(
                "opponent belief weights must sum to a positive value",
            ));
        }
        let expected_delta = weighted_delta / weight_sum;
        let own_core_damage = weighted_own_dmg / weight_sum;
        let opponent_core_damage = weighted_opp_dmg / weight_sum;
        let score = (1.0 - self.risk_lambda) * expected_delta
            + self.risk_lambda * f64::from(min_core_delta);
        let winner = if expected_delta > 0.0 {
            side.name()
        } else if expected_delta < 0.0 {
            side.other().name()
        } else {
            "draw"
        };
        Ok(Valuation {
            score,
            min_core_delta,
            own_core_damage,
            opponent_core_damage,
            ticks: max_ticks,
            winner,
        })
    }
}

impl EvaluatorService for ExactSimEvaluator {
    fn evaluate_state(
        &self,
        problem: &Problem<'_>,
        opponents: &[OpponentHypothesis],
        own_state: &SideState,
    ) -> Result<Valuation, Failure> {
        Ok(self
            .evaluate_batch(problem, opponents, &[own_state])?
            .into_iter()
            .next()
            .expect("single-state batch produces one valuation"))
    }

    fn evaluate_batch(
        &self,
        problem: &Problem<'_>,
        opponents: &[OpponentHypothesis],
        own_states: &[&SideState],
    ) -> Result<Vec<Valuation>, Failure> {
        if opponents.is_empty() {
            return Err(Failure::refused(
                "opponent belief service returned no hypotheses",
            ));
        }
        if own_states.is_empty() {
            return Ok(Vec::new());
        }
        let mut pairs = Vec::with_capacity(own_states.len() * opponents.len());
        for &own_state in own_states {
            for hypothesis in opponents {
                pairs.push((own_state, &hypothesis.state));
            }
        }
        let metrics = simulate_pairs_parallel(problem, &pairs)?;
        metrics
            .chunks_exact(opponents.len())
            .map(|chunk| self.score_metrics(problem.side, opponents, chunk))
            .collect()
    }
}
