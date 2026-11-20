//! Built-in capability services (`opponent`, `space`, `evaluator`).

use mechcore_document::{Position, mask::Mask, r#match::SideState, project, transition};
use serde::Deserialize;

use crate::{cli::Failure, outcome, turn::Side};

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
}

impl EvaluatorService for ExactSimEvaluator {
    fn evaluate_state(
        &self,
        problem: &Problem<'_>,
        opponents: &[OpponentHypothesis],
        own_state: &SideState,
    ) -> Result<Valuation, Failure> {
        if opponents.is_empty() {
            return Err(Failure::refused(
                "opponent belief service returned no hypotheses",
            ));
        }
        let mut weight_sum = 0.0_f64;
        let mut weighted_delta = 0.0_f64;
        let mut weighted_own_dmg = 0.0_f64;
        let mut weighted_opp_dmg = 0.0_f64;
        let mut min_core_delta = i32::MAX;
        let mut max_ticks = 0_u32;

        for hypothesis in opponents {
            let mut combined = problem.opening.clone();
            match problem.side {
                Side::Blue => {
                    combined.blue = own_state.clone();
                    combined.red = hypothesis.state.clone();
                }
                Side::Red => {
                    combined.blue = hypothesis.state.clone();
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
            let (own_dmg, opp_dmg) = match problem.side {
                Side::Blue => (fight.blue.core_damage, fight.red.core_damage),
                Side::Red => (fight.red.core_damage, fight.blue.core_damage),
            };
            let delta = opp_dmg - own_dmg;
            min_core_delta = min_core_delta.min(delta);
            weight_sum += hypothesis.weight;
            weighted_delta += hypothesis.weight * f64::from(delta);
            weighted_own_dmg += hypothesis.weight * f64::from(own_dmg);
            weighted_opp_dmg += hypothesis.weight * f64::from(opp_dmg);
            let ticks = fight
                .trajectory
                .as_ref()
                .map_or(0, |trajectory| trajectory.ticks);
            max_ticks = max_ticks.max(ticks);
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
            problem.side.name()
        } else if expected_delta < 0.0 {
            problem.side.other().name()
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
