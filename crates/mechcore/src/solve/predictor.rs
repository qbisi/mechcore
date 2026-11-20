//! Neural-network-ready placement predictor seam and deterministic demo stubs.

use std::collections::BTreeSet;

use mechcore_document::{Position, r#match::SideState};

use crate::{cli::Failure, turn::Side};

/// Public observation passed to a [`PlacementPredictor`] when placing one
/// `unpinned` formation.
///
/// The opponent's unseen current-round actions are strictly excluded; only
/// `opponent_opening` is visible.
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
}

/// One candidate placement predicted for `PlacementQuery::unit_index`.
#[derive(Clone, Debug)]
pub(crate) struct PlacementProposal {
    pub(crate) position: Position,
    pub(crate) rotated: bool,
    pub(crate) prior: f64,
    pub(crate) label: String,
}

/// Seam implemented by neural placement networks or heuristic placeholder models.
pub(crate) trait PlacementPredictor: Send + Sync {
    #[allow(dead_code)]
    fn name(&self) -> &'static str;

    /// Proposes candidate placements for `query.unit_index`, ordered by preference.
    fn predict(&self, query: &PlacementQuery<'_>) -> Vec<PlacementProposal>;
}

/// Placeholder predictor that keeps every `unpinned` formation in place.
pub(crate) struct StayPredictor;

impl PlacementPredictor for StayPredictor {
    fn name(&self) -> &'static str {
        "stay"
    }

    fn predict(&self, query: &PlacementQuery<'_>) -> Vec<PlacementProposal> {
        let Some(unit) = query
            .state
            .active_units()
            .find(|entry| entry.index == query.unit_index)
        else {
            return Vec::new();
        };
        let Some(position) = unit.position else {
            return Vec::new();
        };
        vec![PlacementProposal {
            position,
            rotated: unit.rotated == Some(true),
            prior: 1.0,
            label: "stay".to_owned(),
        }]
    }
}

/// Placeholder predictor proposing horizontal 10m grid shifts (`offsets`).
pub(crate) struct ShiftStubPredictor {
    offsets: Vec<i32>,
}

impl ShiftStubPredictor {
    pub(crate) fn new(offsets: Vec<i32>) -> Result<Self, Failure> {
        if offsets.is_empty() {
            return Err(Failure::refused(
                "shift_stub predictor requires at least one offset",
            ));
        }
        for &dx in &offsets {
            if dx % 10 != 0 {
                return Err(Failure::refused(format!(
                    "shift_stub offset {dx} must be a multiple of the 10m deployment grid"
                )));
            }
        }
        Ok(Self { offsets })
    }
}

impl PlacementPredictor for ShiftStubPredictor {
    fn name(&self) -> &'static str {
        "shift_stub"
    }

    fn predict(&self, query: &PlacementQuery<'_>) -> Vec<PlacementProposal> {
        let Some(unit) = query
            .state
            .active_units()
            .find(|entry| entry.index == query.unit_index)
        else {
            return Vec::new();
        };
        let Some(position) = unit.position else {
            return Vec::new();
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
        proposals
    }
}

/// Builds a [`PlacementPredictor`] from profile configuration.
///
/// # Errors
/// Returns a refusal for unknown predictor names or invalid parameters.
pub(crate) fn build(
    name: &str,
    offsets: Option<Vec<i32>>,
) -> Result<Box<dyn PlacementPredictor>, Failure> {
    match name {
        "stay" => {
            if offsets.is_some() {
                return Err(Failure::refused(
                    "stay predictor does not accept an offsets parameter",
                ));
            }
            Ok(Box::new(StayPredictor))
        }
        "shift_stub" => Ok(Box::new(ShiftStubPredictor::new(
            offsets.unwrap_or_else(|| vec![0, -10, 10]),
        )?)),
        other => Err(Failure::refused(format!(
            "unknown placement predictor {other:?}; expected stay or shift_stub"
        ))),
    }
}
