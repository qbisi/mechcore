//! Which formations a side may move during deployment.
//!
//! `docs/rules/mobility.md` states the rule and its evidence. A formation may
//! move in the round it arrives, and in no later round unless something frees
//! it: the Deployment Module, a Jump Drive technology, or a Redeploy release.
//! A state carries the answer as `movable`, because the round a formation
//! arrived in is history no other field records.

use crate::layout::Formation;

/// Equipment `13040001` 部署模块, Deployment Module: the formation wearing it
/// moves freely in every round.
pub const DEPLOYMENT_MODULE: i32 = 13_040_001;

/// Commander skills that free one formation to move for the rest of the round:
/// `1000001` 再部署, Redeploy.
pub const REDEPLOY_SKILLS: [i32; 1] = [1_000_001];

/// The 高速引擎 Jump Drive technologies, each with the unit type it frees to
/// move in every round.
pub const JUMP_DRIVES: [(i32, &str); 3] = [(1606, "wasp"), (1611, "overlord"), (1616, "phoenix")];

/// Whether a formation moves freely in every round, whatever round it arrived
/// in, given the side's researched technologies.
#[must_use]
pub fn free(formation: &Formation, techs: &[i32]) -> bool {
    formation.equipment == Some(DEPLOYMENT_MODULE)
        || JUMP_DRIVES
            .iter()
            .any(|(tech, unit)| *unit == formation.type_name && techs.contains(tech))
}
