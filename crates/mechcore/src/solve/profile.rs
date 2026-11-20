//! Profile manifest loading and plugin assembly for `solve`.

use std::path::Path;

use serde::Deserialize;

use crate::cli::Failure;

use super::{
    model::{Operator, OpponentBelief},
    operators::{
        DedupOperator, EvalTopKOperator, PredictUnpinnedPlacement, ReplayTruthOpponent,
        WarmStartOwnActions,
    },
};

pub(crate) const SCHEMA: &str = "mechcore.profile/v1";

/// Declarative solver profile manifest (`--profile <profile.yaml>`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    pub(crate) schema: String,
    pub(crate) name: String,
    pub(crate) opponent: PluginSpec,
    pub(crate) pipeline: Vec<PluginSpec>,
}

/// Reference to one plugin and its parameter map inside a profile.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginSpec {
    #[serde(rename = "use")]
    pub(crate) plugin: String,
    #[serde(default)]
    pub(crate) with: serde_json::Value,
}

/// Instantiated opponent belief and pipeline operators built from a [`Profile`].
pub(crate) struct BuiltProfile {
    pub(crate) opponent: Box<dyn OpponentBelief>,
    pub(crate) operators: Vec<Box<dyn Operator>>,
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

    /// Instantiates the opponent belief plugin and every pipeline operator.
    ///
    /// # Errors
    /// Returns a refusal for unknown plugin names or invalid parameters.
    pub(crate) fn build(&self) -> Result<BuiltProfile, Failure> {
        let opponent: Box<dyn OpponentBelief> = match self.opponent.plugin.as_str() {
            "opponent.replay_truth" => {
                if !self.opponent.with.is_null()
                    && self
                        .opponent
                        .with
                        .as_object()
                        .is_none_or(|map| !map.is_empty())
                {
                    return Err(Failure::refused(
                        "opponent.replay_truth takes no parameters",
                    ));
                }
                Box::new(ReplayTruthOpponent)
            }
            other => {
                return Err(Failure::refused(format!(
                    "unknown opponent plugin {other:?}; expected opponent.replay_truth"
                )));
            }
        };

        let mut operators: Vec<Box<dyn Operator>> = Vec::with_capacity(self.pipeline.len());
        for stage in &self.pipeline {
            let operator: Box<dyn Operator> = match stage.plugin.as_str() {
                "warm_start.own_actions" => Box::new(WarmStartOwnActions::from_value(&stage.with)?),
                "placement.predict_unpinned" => {
                    Box::new(PredictUnpinnedPlacement::from_value(&stage.with)?)
                }
                "flow.dedup" => {
                    if !stage.with.is_null()
                        && stage.with.as_object().is_none_or(|map| !map.is_empty())
                    {
                        return Err(Failure::refused("flow.dedup takes no parameters"));
                    }
                    Box::new(DedupOperator)
                }
                "eval.top_k" => Box::new(EvalTopKOperator::from_value(&stage.with)?),
                other => {
                    return Err(Failure::refused(format!(
                        "unknown pipeline operator {other:?}"
                    )));
                }
            };
            operators.push(operator);
        }
        Ok(BuiltProfile {
            opponent,
            operators,
        })
    }
}
