//! A match round's deployment, replayed before its fight.
//!
//! Each side's recorded decisions play in their order on the side's own
//! track, both tracks starting together, ahead of the fight's first tick.
//! Each decision plays for two seconds. What a decision did to the formations is
//! read by stepping the side's state through it, the way the fight's layout was
//! projected, and comparing the formations before and after: one appeared,
//! one left, one moved or one rose a level. The page animates those changes.

use std::collections::BTreeMap;

use mechcore_document::{
    Region,
    compile::unit_footprint,
    economy::Economy,
    r#match::{Action, SideState, StateUnit, TurnActions},
};
use serde::Serialize;

/// Seconds before the first decision, per decision, and after the last.
const INTRO: f64 = 0.4;
const ACTION_SECONDS: f64 = 2.0;
const OUTRO: f64 = 0.6;

/// One round's deployment lead-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LeadIn {
    /// Frames before the fight's first tick: the page plays `1 - ticks`
    /// through `0`, then tick 1.
    pub ticks: u32,
    /// Available supply on `[blue, red]` as the deployment opened.
    pub supply: [i32; 2],
    /// Unit technologies already active on each side as the deployment opened,
    /// keyed by unit kind.
    pub techs: [BTreeMap<String, Vec<i32>>; 2],
    /// Every unit formation either side held at some moment of the deployment.
    pub formations: Vec<LeadInFormation>,
    /// Blue's decisions and red's, each on its own clock.
    pub tracks: [Vec<LeadInEntry>; 2],
}

/// A formation as the deployment knew it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LeadInFormation {
    pub team: u32,
    /// The formation's index in its side's state.
    pub index: i32,
    pub kind: String,
    /// The recording's formation ID, when the formation reached the fight.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formation: Option<u64>,
    /// Where it stood as the deployment opened; absent when a decision brought it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opening: Option<Stance>,
}

/// Where a formation stands and its level, in world centimetres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Stance {
    pub x: i64,
    pub z: i64,
    /// Occupied extents along the world's x and z axes.
    pub width: i64,
    pub depth: i64,
    /// The way its region faces, in tenths of a degree: toward the enemy in
    /// the main region, toward the map's centre line in a flank.
    pub facing: i64,
    pub level: i32,
}

/// One decision on a side's track.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LeadInEntry {
    /// The frame it starts on, counted from the lead-in's first.
    pub start: u32,
    pub ticks: u32,
    /// The decision's document type, as `buy_unit`.
    #[serde(rename = "type")]
    pub action: String,
    /// The document name of what it takes, buys or researches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The unit type a decision about a type names, as a technology's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// A researched technology's ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tech: Option<i32>,
    /// Available supply after it.
    pub supply: i32,
    /// Supply it spent; negative when it gave supply.
    pub spent: i32,
    /// What it did to the formations.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<Change>,
}

/// A formation's stance after a decision; none when it left the side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Change {
    /// Position in [`LeadIn::formations`].
    pub formation: usize,
    pub to: Option<Stance>,
}

/// Replays a round's decisions from the state it opened with.
///
/// `formation_ids` names each `(team, index)` formation that reached the
/// fight by its recording formation ID. Answers `None` when neither side
/// decided anything a lead-in shows.
///
/// # Errors
///
/// Refuses a decision the state does not admit.
pub fn lead_in(
    economy: &Economy,
    opening: [&SideState; 2],
    actions: &TurnActions,
    declined: Option<i32>,
    formation_ids: &BTreeMap<(u32, i32), u64>,
    ticks_per_second: u32,
) -> Result<Option<LeadIn>, String> {
    let tick = |seconds: f64| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ticks = (seconds * f64::from(ticks_per_second)).round() as u32;
        ticks.max(1)
    };
    let mut formations = Vec::new();
    let mut known = BTreeMap::new();
    for (team, side) in (0u32..).zip(opening) {
        for entry in &side.units {
            known.insert((team, entry.unit.index), formations.len());
            formations.push(LeadInFormation {
                team,
                index: entry.unit.index,
                kind: entry.unit.type_name.clone(),
                formation: formation_ids.get(&(team, entry.unit.index)).copied(),
                opening: stance(team, entry),
            });
        }
    }
    let mut tracks = [Vec::new(), Vec::new()];
    for (team, (side, decisions)) in
        (0u32..).zip(opening.into_iter().zip([&actions.blue, &actions.red]))
    {
        tracks[team as usize] = build_track(
            economy,
            team,
            side,
            decisions,
            declined,
            formation_ids,
            &mut formations,
            &mut known,
            &tick,
        )?;
    }
    let Some(last) = tracks
        .iter()
        .filter_map(|track| track.last().map(|entry| entry.start + entry.ticks))
        .max()
    else {
        return Ok(None);
    };
    let techs = std::array::from_fn(|team| {
        let mut by_unit: BTreeMap<String, Vec<i32>> = BTreeMap::new();
        for &id in &opening[team].techs {
            if let Some((unit_kind, _)) = mechcore_document::names::technology_owner(id)
                .and_then(mechcore_document::unit_type_from_id)
            {
                let ids = by_unit.entry(unit_kind.to_string()).or_default();
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        by_unit
    });
    Ok(Some(LeadIn {
        ticks: last + tick(OUTRO),
        supply: [opening[0].supply, opening[1].supply],
        techs,
        formations,
        tracks,
    }))
}

#[allow(clippy::too_many_arguments)]
fn build_track(
    economy: &Economy,
    team: u32,
    opening: &SideState,
    decisions: &[Action],
    declined: Option<i32>,
    formation_ids: &BTreeMap<(u32, i32), u64>,
    formations: &mut Vec<LeadInFormation>,
    known: &mut BTreeMap<(u32, i32), usize>,
    tick: &impl Fn(f64) -> u32,
) -> Result<Vec<LeadInEntry>, String> {
    let mut track = Vec::new();
    let mut state = opening.clone();
    let mut clock = tick(INTRO);
    for action in decisions {
        if matches!(action, Action::ChooseAdvanceTeam { .. } | Action::Concede) {
            continue;
        }
        let next = mechcore_document::transition::deployed(
            economy,
            &state,
            std::slice::from_ref(action),
            team == 1,
            declined,
        )
        .map_err(|error| error.to_string())?;
        let mut changes = Vec::new();
        for before in &state.units {
            let after = next
                .units
                .iter()
                .find(|e| e.unit.index == before.unit.index);
            let (was, now) = (stance(team, before), after.and_then(|e| stance(team, e)));
            if was == now {
                continue;
            }
            changes.push(Change {
                formation: known[&(team, before.unit.index)],
                to: now,
            });
        }
        for entry in &next.units {
            if state.units.iter().all(|e| e.unit.index != entry.unit.index) {
                let formation = *known.entry((team, entry.unit.index)).or_insert_with(|| {
                    formations.push(LeadInFormation {
                        team,
                        index: entry.unit.index,
                        kind: entry.unit.type_name.clone(),
                        formation: formation_ids.get(&(team, entry.unit.index)).copied(),
                        opening: None,
                    });
                    formations.len() - 1
                });
                changes.push(Change {
                    formation,
                    to: stance(team, entry),
                });
            }
        }
        let ticks = tick(ACTION_SECONDS);
        let fields = serde_yaml::to_value(action).map_err(|error| error.to_string())?;
        let text = |key: &str| fields.get(key).and_then(|v| v.as_str()).map(str::to_owned);
        let (unit, tech) = match action {
            Action::UpgradeTechnology { tech, .. } => (text("unit"), Some(*tech)),
            _ => (None, None),
        };
        track.push(LeadInEntry {
            start: clock,
            ticks,
            action: text("type").unwrap_or_default(),
            name: text("name").or_else(|| text("tech")),
            unit,
            tech,
            supply: next.supply,
            spent: state.supply - next.supply,
            changes,
        });
        clock += ticks;
        state = next;
    }
    Ok(track)
}

/// A formation's stance in world centimetres; none for a type without a
/// footprint. Red's positions turn about the map's centre.
fn stance(team: u32, entry: &StateUnit) -> Option<Stance> {
    let unit = &entry.unit;
    let rotated = unit.rotated.unwrap_or(false);
    let (width, depth) = unit_footprint(&unit.type_name, unit.position, rotated)?;
    let turn = if team == 0 { 100 } else { -100 };
    let x = i64::from(unit.position.x) * turn;
    let z = i64::from(unit.position.y) * turn;
    let facing = match Region::of(unit.position) {
        Region::Main if team == 0 => 0,
        Region::Main => 1800,
        _ if x < 0 => 900,
        _ => 2700,
    };
    Some(Stance {
        x,
        z,
        width: width * 100,
        depth: depth * 100,
        facing,
        level: unit.level.unwrap_or(1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mechcore_document::{
        Position,
        r#match::{Allowances, NextIndex, ReinforceChoice},
    };

    fn unit(type_name: &str, index: i32, x: i32, y: i32) -> StateUnit {
        serde_yaml::from_str(&format!(
            "{{name: {type_name}, index: {index}, position: {{x: {x}, y: {y}}}, level: 1, source: joined, value: 100}}"
        ))
        .unwrap()
    }

    #[test]
    fn independent_tracks_scale_by_action_and_distance_without_cap() {
        let economy = Economy::embedded().unwrap();
        let shop = Allowances {
            buys_remaining: 5,
            unlocks_remaining: 3,
            contraptions_remaining: 3,
        };
        let blue = SideState {
            supply: 5000,
            unlocked_units: vec![9, 10], // fang, crawler
            shop: shop.clone(),
            next_index: NextIndex {
                unit: 1,
                contraption: 0,
            },
            units: vec![unit("crawler", 0, 0, -100)],
            ..SideState::default()
        };
        let red = SideState {
            supply: 4000,
            unlocked_units: vec![9, 10],
            shop,
            next_index: NextIndex {
                unit: 1,
                contraption: 0,
            },
            units: vec![unit("crawler", 0, 0, -100)],
            ..SideState::default()
        };
        let actions = TurnActions {
            blue: vec![
                Action::UnlockUnit { unit: 1 }, // 2.0s = 40 ticks
                Action::BuyUnit {
                    unit: 9,
                    position: Some(Position { x: -330, y: 100 }),
                    rotated: false,
                },
                Action::MoveUnit {
                    index: 0,
                    position: Position { x: 140, y: -100 },
                    rotated: false,
                },
                Action::UpgradeUnit { index: 0 },
                Action::UpgradeTechnology { unit: 9, tech: 209 },
            ],
            red: vec![Action::BuyUnit {
                unit: 9,
                position: Some(Position { x: 0, y: -120 }),
                rotated: false,
            }],
        };
        let ids = BTreeMap::from([((0, 0), 10), ((0, 1), 11), ((1, 0), 20), ((1, 1), 21)]);
        let lead = lead_in(economy, [&blue, &red], &actions, None, &ids, 20)
            .unwrap()
            .unwrap();
        assert_eq!(lead.supply, [5000, 4000]);
        // Intro = 8 ticks (0.4s). Blue: 5 * 40 = 200 ticks -> ends at 208.
        // Red: 40 ticks -> ends at 48. Outro = 12 ticks -> total 220 ticks.
        assert_eq!(lead.tracks[0].len(), 5);
        assert_eq!(lead.tracks[1].len(), 1);
        assert_eq!(
            lead.tracks[0]
                .iter()
                .map(|e| (e.action.as_str(), e.start, e.ticks))
                .collect::<Vec<_>>(),
            [
                ("unlock_unit", 8, 40),
                ("buy_unit", 48, 40),
                ("move_unit", 88, 40),
                ("upgrade_unit", 128, 40),
                ("upgrade_technology", 168, 40),
            ]
        );
        assert_eq!(
            (
                lead.tracks[1][0].action.as_str(),
                lead.tracks[1][0].start,
                lead.tracks[1][0].ticks
            ),
            ("buy_unit", 8, 40)
        );
        assert_eq!(lead.ticks, 8 + 200 + 12);

        // Verify flank orientation and red-side coordinate mirroring.
        let flank_buy = &lead.tracks[0][1].changes[0];
        let flank_stance = flank_buy.to.unwrap();
        assert_eq!((flank_stance.x, flank_stance.z), (-33_000, 10_000));
        assert_eq!(flank_stance.facing, 900);
        let (expected_w, expected_d) =
            unit_footprint("fang", Position { x: -330, y: 100 }, false).unwrap();
        assert_eq!(
            (flank_stance.width, flank_stance.depth),
            (expected_w * 100, expected_d * 100)
        );

        let red_buy = &lead.tracks[1][0].changes[0];
        let red_stance = red_buy.to.unwrap();
        assert_eq!((red_stance.x, red_stance.z), (0, 12_000));
        assert_eq!(red_stance.facing, 1800);
    }

    #[test]
    fn empty_or_non_lead_in_actions_return_none() {
        let economy = Economy::embedded().unwrap();
        let side = SideState::default();
        let none = lead_in(
            economy,
            [&side, &side],
            &TurnActions {
                blue: vec![Action::Concede],
                red: vec![],
            },
            None,
            &BTreeMap::new(),
            20,
        )
        .unwrap();
        assert_eq!(none, None);

        // ChooseReinforceItem produces a placeholder entry.
        let side_with_offer = SideState {
            supply: 200,
            ..SideState::default()
        };
        let with_reinforce = lead_in(
            economy,
            [&side_with_offer, &side],
            &TurnActions {
                blue: vec![Action::ChooseReinforceItem {
                    index: 0,
                    id: ReinforceChoice::Refund(100),
                }],
                red: vec![],
            },
            Some(100),
            &BTreeMap::new(),
            20,
        )
        .unwrap()
        .unwrap();
        assert_eq!(with_reinforce.tracks[0][0].ticks, 40);
        assert_eq!(with_reinforce.tracks[0][0].spent, -100);
    }

    #[test]
    fn opening_techs_group_by_unit_kind_without_duplicates() {
        let economy = Economy::embedded().unwrap();
        let owner = mechcore_document::names::technology_owner(209)
            .and_then(mechcore_document::unit_type_from_id)
            .unwrap()
            .0;
        let blue = SideState {
            supply: 200,
            techs: vec![209, 209],
            ..SideState::default()
        };
        let red = SideState::default();
        let lead = lead_in(
            economy,
            [&blue, &red],
            &TurnActions {
                blue: vec![Action::ChooseReinforceItem {
                    index: 0,
                    id: ReinforceChoice::Refund(100),
                }],
                red: vec![],
            },
            Some(100),
            &BTreeMap::new(),
            20,
        )
        .unwrap()
        .unwrap();
        assert_eq!(lead.techs[0].get(owner), Some(&vec![209]));
        assert!(lead.techs[1].is_empty());
    }
}
