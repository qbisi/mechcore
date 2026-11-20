//! Batch-first placement proposal seam and deterministic demo proposers.

use std::collections::BTreeSet;

use mechcore_document::{Position, mask::Mask, r#match::SideState};
use serde::Deserialize;

use crate::{cli::Failure, turn::Side};

use super::model::PlacementProposer;

/// Public observation passed to a [`PlacementProposer`] when placing one
/// formation.
///
/// The opponent's unseen current-round actions are strictly excluded; only
/// `opponent_opening` and the exact geometric `legal_mask` are supplied.
pub(crate) struct PlacementQuery<'a> {
    #[allow(dead_code)]
    pub(crate) round: i32,
    #[allow(dead_code)]
    pub(crate) side: Side,
    pub(crate) state: &'a SideState,
    #[allow(dead_code)]
    pub(crate) opponent_opening: &'a SideState,
    pub(crate) unit_index: i32,
    #[allow(dead_code)]
    pub(crate) unpinned: &'a BTreeSet<i32>,
    #[allow(dead_code)]
    pub(crate) legal_mask: &'a Mask,
}

/// One candidate placement proposed for `PlacementQuery::unit_index`.
#[derive(Clone, Debug)]
pub(crate) struct PlacementProposal {
    pub(crate) position: Position,
    pub(crate) rotated: bool,
    pub(crate) prior: f64,
    pub(crate) label: String,
}

/// Placeholder proposer that keeps every queried formation in place.
pub(crate) struct StayProposer;

impl PlacementProposer for StayProposer {
    fn name(&self) -> &'static str {
        "proposer.stay"
    }

    fn propose_batch(
        &self,
        queries: &[PlacementQuery<'_>],
    ) -> Result<Vec<Vec<PlacementProposal>>, Failure> {
        let mut out = Vec::with_capacity(queries.len());
        for query in queries {
            let items = query
                .state
                .active_units()
                .find(|entry| entry.index == query.unit_index)
                .and_then(|unit| {
                    unit.position.map(|position| {
                        vec![PlacementProposal {
                            position,
                            rotated: unit.rotated == Some(true),
                            prior: 1.0,
                            label: "stay".to_owned(),
                        }]
                    })
                })
                .unwrap_or_default();
            out.push(items);
        }
        Ok(out)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShiftStubConfig {
    #[serde(default = "default_offsets")]
    offsets: Vec<i32>,
}

fn default_offsets() -> Vec<i32> {
    vec![0, -10, 10]
}

/// Placeholder proposer generating horizontal 10m grid shifts (`offsets`).
pub(crate) struct ShiftStubProposer {
    offsets: Vec<i32>,
}

impl ShiftStubProposer {
    pub(crate) fn from_value(value: &serde_json::Value) -> Result<Self, Failure> {
        let normalized = if value.is_null() {
            serde_json::json!({})
        } else {
            value.clone()
        };
        let config: ShiftStubConfig = serde_json::from_value(normalized).map_err(|error| {
            Failure::refused(format!(
                "invalid parameters for proposer.shift_stub: {error}"
            ))
        })?;
        if config.offsets.is_empty() {
            return Err(Failure::refused(
                "proposer.shift_stub requires at least one offset",
            ));
        }
        for &dx in &config.offsets {
            if dx % 10 != 0 {
                return Err(Failure::refused(format!(
                    "proposer.shift_stub offset {dx} must be a multiple of the 10m deployment grid"
                )));
            }
        }
        Ok(Self {
            offsets: config.offsets,
        })
    }
}

impl PlacementProposer for ShiftStubProposer {
    fn name(&self) -> &'static str {
        "proposer.shift_stub"
    }

    fn propose_batch(
        &self,
        queries: &[PlacementQuery<'_>],
    ) -> Result<Vec<Vec<PlacementProposal>>, Failure> {
        let mut out = Vec::with_capacity(queries.len());
        for query in queries {
            let Some(unit) = query
                .state
                .active_units()
                .find(|entry| entry.index == query.unit_index)
            else {
                out.push(Vec::new());
                continue;
            };
            let Some(position) = unit.position else {
                out.push(Vec::new());
                continue;
            };
            let rotated = unit.rotated == Some(true);
            let mut proposals = Vec::with_capacity(self.offsets.len());
            for &dx in &self.offsets {
                let Some(x) = position.x.checked_add(dx) else {
                    continue;
                };
                let (prior, label) = match dx.cmp(&0) {
                    std::cmp::Ordering::Equal => (1.0, "stay".to_owned()),
                    std::cmp::Ordering::Less => (0.8, format!("shift_left({dx})")),
                    std::cmp::Ordering::Greater => (0.8, format!("shift_right(+{dx})")),
                };
                proposals.push(PlacementProposal {
                    position: Position { x, y: position.y },
                    rotated,
                    prior,
                    label,
                });
            }
            out.push(proposals);
        }
        Ok(out)
    }
}
