//! Fills a match document from one recorded match.
//!
//! Most fields are copied. Four are rebuilt, because the snapshot the game
//! writes precedes the round's own reset: client `supply` gains the round's income
//! (server snapshots already include it),
//! the two shop counters are restored to the round's allowance, the energy
//! tower list is the set activated during the round and so empty at its
//! start, and the equipment list drops what the formations carry.
//! `docs/spec/document/match.md` says what the conversion refuses.

use crate::catalog::{construction_type_from_id, contraption_type_from_id, unit_type_from_id};
use crate::economy::{Economy, OpeningKind, RoundSupply};
use crate::layout::{ContraptionPlacement, Experience, Position, StaticPlacement, UnitPlacement};
use crate::r#match::{
    Action, EquipmentItem, Match, MatchSide, NextIndex, Offers, Opening, OpeningOffer, PanelSkill,
    SideState, SkillTarget, State, StateUnit, Turn, TurnActions,
};
use crate::opening::{self, Stream};
use crate::record::{self, ActionRecord, PlayerData, PlayerRoundRecord};
use crate::{GrbrStanding, retained_from_grbr_round};
use std::collections::BTreeMap;

/// The version a replay's header names: the last component of the game
/// version `GAME_VERSION` pins, which is how a GRBR carries it (`2324`).
fn replay_version() -> &'static str {
    let build = crate::economy::game_build();
    build.rsplit('.').next().unwrap_or(build)
}
/// Energy tower skill `1`, the only one carrying a next-round supply change.
const RAPID_SUPPLY_SKILL: i32 = 1;
/// Officers a research centre blueprint grants; `blueprints` owns them instead.
const CHAIN_OFFICERS: [i32; 4] = [20300, 20301, 20310, 20311];
/// Round 0 is the opening, which has no state and so is no turn.
const OPENING_ROUNDS: usize = 1;

/// Which half of the map a side plays on, and so how its positions are read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seat {
    Blue,
    Red,
}

impl Seat {
    const fn name(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Red => "red",
        }
    }

    /// Red's recorded coordinates are the blue frame turned half a turn.
    fn position(self, record: &record::PositionRecord) -> Position {
        match self {
            Self::Blue => Position {
                x: record.x,
                y: record.y,
            },
            Self::Red => Position {
                x: -record.x,
                y: -record.y,
            },
        }
    }
}

/// Converts a client-recorded or server-downloaded GRBR into a match document.
///
/// # Errors
///
/// Returns an error when the replay is not a standard 1v1 of this version,
/// when its rounds are not the contiguous sequence both sides
/// and the match share, or when it contains an object this build's catalogues
/// or this format cannot name.
pub fn match_from_grbr(grbr: &[u8]) -> Result<Match, String> {
    let record = record::read(grbr)?;
    readable(&record)?;
    let [blue, red] = <[record::PlayerRecord; 2]>::try_from(record.players.entries)
        .map_err(|players| format!("replay has {} sides, and a match has two", players.len()))?;

    let match_rounds: Vec<i32> = record
        .match_rounds
        .entries
        .iter()
        .map(|entry| entry.round)
        .collect();
    if match_rounds.first() != Some(&0)
        || match_rounds.get(1).is_some_and(|round| *round < 1)
        || match_rounds[match_rounds.len().min(1)..]
            .windows(2)
            .any(|pair| pair[0].checked_add(1) != Some(pair[1]))
    {
        return Err(format!(
            "match rounds {match_rounds:?} are not round zero followed by a contiguous captured suffix"
        ));
    }
    for (seat, player) in [("blue", &blue), ("red", &red)] {
        let rounds: Vec<i32> = player
            .rounds
            .entries
            .iter()
            .map(|entry| entry.round)
            .collect();
        if rounds != match_rounds {
            return Err(format!(
                "{seat} records rounds {rounds:?} while the match records {match_rounds:?}"
            ));
        }
    }

    if match_rounds.len() < 2 {
        return Err(format!(
            "replay holds {} round, which is the opening alone, and a match is deployment rounds",
            match_rounds.len()
        ));
    }

    let economy = Economy::embedded()?;
    let has_opening = match_rounds[1] == 1
        && [&blue, &red]
            .iter()
            .any(|player| !net_actions(&player.rounds.entries[0].actions.entries).is_empty());
    let predicted = opening::predict(economy, record.info.system_seed, record.info.map_id)?;
    let pool = predicted.initialization.unit_round_pool;
    // The opening is round 0, and it has no state: every side enters it holding
    // nothing. Read its decision only when the captured prefix is complete;
    // otherwise the turns start at the first available deployment.
    let mut turns = Vec::with_capacity(match_rounds.len() - 1);
    for (position, round) in match_rounds
        .iter()
        .copied()
        .enumerate()
        .skip(OPENING_ROUNDS)
    {
        // A round that deals offers also offers the decline, and what the
        // decline pays depends on the pool the seed selects.
        let offers = record.match_rounds.entries[position]
            .reinforce_items
            .arrays
            .first()
            .map(|array| {
                Ok::<_, String>(Offers {
                    dealt: array.values.clone(),
                    refund: crate::reinforcement::decline_supply(economy, pool, round)?,
                })
            })
            .transpose()?;
        turns.push(turn(grbr, economy, [&blue, &red], position, round, offers)?);
    }

    // The four combinations each side was dealt are recorded nowhere, and are
    // not lost: the seed determines them. A client opening snapshot is already
    // initialized; a server opening snapshot is still the raw seeded stream.
    let dealt = if has_opening && !blue.rounds.entries[0].data.special_supply {
        opening_offers(economy, &record.match_rounds.entries[0])?
    } else {
        predicted.deal.clone()
    };

    settle_concession(&mut turns)?;

    let mut converted = Match {
        has_opening,
        game_build: crate::economy::game_build().to_owned(),
        map_id: record.info.map_id,
        seed: record.info.system_seed,
        deploy_time: Some(record.info.deploy_time),
        blue: match_side(economy, &blue, Seat::Blue, dealt.blue, has_opening)?,
        red: match_side(economy, &red, Seat::Red, dealt.red, has_opening)?,
        turns,
    };
    if has_opening {
        crate::reinforcement::fill_checkpoints(economy, &mut converted)?;
    } else {
        converted.blue.constructions = predicted.constructions.blue;
        converted.red.constructions = predicted.constructions.red;
        crate::reinforcement::fill_recorded_checkpoints(
            economy,
            &mut converted,
            &record.match_rounds.entries,
        )?;
    }
    Ok(converted)
}

/// Refuses a replay this converter does not read, by the property it fails.
///
/// Each of these is a premise the rest of the conversion rests on, and naming
/// which one failed is what tells a caller whether the file is the wrong build,
/// the wrong provenance or the wrong kind of match.
fn readable(record: &record::BattleRecord) -> Result<(), String> {
    if record.version != replay_version() {
        return Err(format!(
            "replay is build {}, and this converter reads build {}",
            record.version,
            replay_version()
        ));
    }
    if record.info.match_mode != "VS_1_1" {
        return Err(format!(
            "replay is match mode {}, and this format describes VS_1_1",
            record.info.match_mode
        ));
    }
    if let Some(match_type) = &record.info.match_type {
        return Err(format!(
            "replay is a {match_type} match, not a played one; \
             its snapshots were installed by Training Ground commands"
        ));
    }
    if !record.info.game_rules.values.is_empty() {
        return Err(format!(
            "replay carries game rules {:?}, which this format has not been measured against",
            record.info.game_rules.values
        ));
    }
    Ok(())
}

/// One deployment round: the position each side opens it with, and the
/// decisions each takes from there.
fn turn(
    grbr: &[u8],
    economy: &Economy,
    [blue, red]: [&record::PlayerRecord; 2],
    position: usize,
    round: i32,
    offers: Option<Offers>,
) -> Result<Turn, String> {
    let opened = [
        side_state(grbr, economy, blue, position, Seat::Blue)?,
        side_state(grbr, economy, red, position, Seat::Red)?,
    ];
    let [blue_state, red_state] = opened;
    let taken = |player: &record::PlayerRecord, seat, opened| {
        actions(
            economy,
            &player.rounds.entries[position],
            seat,
            opened,
            offers.as_ref(),
        )
        .map_err(|error| format!("round {round}: {error}"))
    };
    let actions = TurnActions {
        blue: taken(blue, Seat::Blue, &blue_state)?,
        red: taken(red, Seat::Red, &red_state)?,
    };
    Ok(Turn {
        round,
        state: State {
            random_state: [0; 4],
            reinforce_pool: crate::r#match::PoolDelta::default(),
            reinforce_offers: offers,
            blue: blue_state,
            red: red_state,
        },
        actions,
    })
}

/// A replay converted into the document `convert --to match` writes, and that
/// document read back the way `verify` reads it.
pub struct Converted {
    pub r#match: Match,
    pub yaml: String,
    pub stated: opening::Stated,
}

/// Converts a replay into its match document, refusing one the document
/// would not faithfully carry.
///
/// Two things are checked here because only the replay can check them. The
/// written document has to read back as exactly the match it was written
/// from, every state field and action operand included, or the corpus would
/// hold something other than what was converted. And the seeded random stream
/// has to land on every state the replay recorded: the match seed on the
/// opening round's, and each round's reinforcement deal on the states that
/// round and the next one recorded. The document carries post-deal checkpoints,
/// so the native pre-deal boundaries are checked while the replay is available.
///
/// A deal the rules cannot reproduce is refused: conversion cannot write its
/// post-deal checkpoint.
///
/// # Errors
///
/// Returns what [`match_from_grbr`] refuses, a document that does not read
/// back as its match, and a stream that misses a recorded state.
pub fn document(economy: &Economy, grbr: &[u8]) -> Result<Converted, String> {
    let r#match = match_from_grbr(grbr)?;
    let yaml = crate::r#match::canonical_yaml(&r#match)?;
    let stated = opening::stated(yaml.as_bytes())?
        .ok_or("the converted document does not read back as a match")?;
    if stated.turns != r#match.turns {
        return Err(
            "the converted document does not read back as the match it was \
                    written from"
                .into(),
        );
    }
    if stated.has_opening {
        deal_lands_on_the_record(economy, &record::read(grbr)?, &stated)?;
    }
    Ok(Converted {
        r#match,
        yaml,
        stated,
    })
}

/// Refuses a replay whose recorded random states the seeded stream misses,
/// or whose recorded pool the match's deal does not make.
///
/// The generator is Lua's and a state is 256 bits, so a near miss does not
/// land: equal states are the whole of the claim that the stream is modelled.
fn deal_lands_on_the_record(
    economy: &Economy,
    native: &record::BattleRecord,
    stated: &opening::Stated,
) -> Result<(), String> {
    let recorded = |round: i32| {
        usize::try_from(round)
            .ok()
            .and_then(|at| native.match_rounds.entries.get(at))
            .map(|entry| entry.random_state.states.values.as_slice())
    };
    // A server's round-zero scene snapshot precedes pool initialization;
    // the client recorder takes its snapshot after that initialization.
    let seeded = if native.players.entries.iter().all(|player| {
        player
            .rounds
            .entries
            .first()
            .is_some_and(|round| round.data.special_supply)
    }) {
        Stream::seeded(native.info.system_seed).state()
    } else {
        opening::initialize(native.info.system_seed)?.stream.state()
    };
    if recorded(0) != Some(seeded.as_slice()) {
        return Err(format!(
            "seed {} does not reach the random state the opening round recorded",
            native.info.system_seed
        ));
    }
    let Ok(deal) = opening::verify(economy, stated)
        .and_then(|found| crate::reinforcement::verify(economy, stated, &found))
    else {
        return Ok(());
    };
    for round in &deal.rounds {
        if recorded(round.round) != Some(round.before_state.as_slice()) {
            return Err(format!(
                "round {}'s deal starts from a random state the round did not record",
                round.round
            ));
        }
        if let Some(next) = recorded(round.round + 1)
            && next != round.after_state.as_slice()
        {
            return Err(format!(
                "round {}'s deal ends on a random state round {} did not record",
                round.round,
                round.round + 1
            ));
        }
    }
    pool_lands_on_the_recorded_log(&native.match_rounds.entries, &deal.pools)
}

/// Refuses a replay whose rounds open on another pool than the match's deal
/// leaves: each round's log, and the rounds it excludes the level-4 commander
/// skills from, must be the deal's. A round's choices may follow the players'
/// selection order, which a match does not state. Exclusion dictionary entries
/// may be enumerated in any order; their keys and card sets must still agree.
fn pool_lands_on_the_recorded_log(
    recorded_rounds: &[record::MatchRound],
    pools: &[crate::reinforcement::Pool],
) -> Result<(), String> {
    let level_four_skills = crate::reinforcement::level_four_skills()?;
    let unordered = |log: &[(i32, i32)], choices: &[std::ops::Range<usize>]| {
        let mut log = log.to_vec();
        for choice in choices {
            if let Some(entries) = log.get_mut(choice.clone()) {
                entries.sort_unstable();
            }
        }
        log
    };
    for entry in recorded_rounds {
        let pool = usize::try_from(entry.round - 1)
            .ok()
            .and_then(|at| pools.get(at));
        // Round 0 opens on the pool the seed initializes, and a round past
        // the deal's has nothing to be compared with.
        let (log, choices, excluded) = match (pool, entry.round) {
            (Some(pool), _) => (
                pool.log.as_slice(),
                pool.choices.as_slice(),
                pool.excluded.as_slice(),
            ),
            (None, 0) => (&[][..], &[][..], &[][..]),
            (None, _) => continue,
        };
        let recorded: Vec<(i32, i32)> = entry
            .pool_operations
            .entries
            .iter()
            .map(|operation| (operation.operation, operation.id))
            .collect();
        if unordered(&recorded, choices) != unordered(log, choices) {
            return Err(format!(
                "round {} opens on a reinforcement pool log the deal does not make",
                entry.round
            ));
        }
        let mut recorded: Vec<i32> = entry
            .excluded_rounds
            .entries
            .iter()
            .map(|excluded| excluded.round)
            .collect();
        recorded.sort_unstable();
        if recorded != excluded
            || entry.excluded_rounds.entries.iter().any(|excluded| {
                !crate::reinforcement::same_excluded_cards(&excluded.ids.values, &level_four_skills)
            })
        {
            return Err(format!(
                "round {} excludes other reinforcements than the deal does",
                entry.round
            ));
        }
    }
    Ok(())
}

/// Reconstructs both sides' offers from the opening round's random state.
/// A missing or malformed state cannot supply the offers that `choose` names.
fn opening_offers(economy: &Economy, round: &record::MatchRound) -> Result<opening::Deal, String> {
    let state = <[u64; 4]>::try_from(round.random_state.states.values.as_slice())
        .map_err(|_| "opening requires a random state of four words".to_string())?;
    let mut stream = opening::Stream::from_state(state)?;
    opening::deal(economy, &mut stream)
}

/// Lets a concession stand for its side's whole round.
///
/// `PAD_GiveUp` is the one recorded action that overrides `IsExitMatchAction`,
/// and the override returns true unconditionally: it leaves the match. A
/// replay may still record decisions of the conceding side around it in the
/// same round, which no fight follows; the concession replaces them all, and
/// the round is not fought.
///
/// # Errors
///
/// Returns an error when a round follows a concession, or more than one side
/// concedes.
fn settle_concession(turns: &mut [Turn]) -> Result<(), String> {
    let last = turns.len().saturating_sub(1);
    let mut conceded = 0;
    for (at, turn) in turns.iter_mut().enumerate() {
        for (side, actions) in [
            ("blue", &mut turn.actions.blue),
            ("red", &mut turn.actions.red),
        ] {
            if !actions.contains(&Action::Concede) {
                continue;
            }
            conceded += 1;
            if at != last {
                return Err(format!(
                    "round {} {side} concedes, and the replay continues",
                    turn.round
                ));
            }
            *actions = vec![Action::Concede];
        }
    }
    if conceded > 1 {
        return Err(format!(
            "replay records {conceded} concessions, and the first one ends the match"
        ));
    }
    Ok(())
}

fn match_side(
    economy: &Economy,
    player: &record::PlayerRecord,
    seat: Seat,
    dealt: Vec<OpeningOffer>,
    has_opening: bool,
) -> Result<MatchSide, String> {
    let mut loadout = BTreeMap::new();
    for row in &player.data.unit_datas.entries {
        // The record lists every unit the account owns a loadout for, and a
        // standard 1v1 match fields only those this build's catalogue names.
        if unit_type_from_id(row.id).is_none() {
            continue;
        }
        let mut techs: Vec<i32> = row.techs.entries.iter().map(|tech| tech.data).collect();
        techs.sort_unstable();
        loadout.insert(row.id, techs);
    }
    Ok(MatchSide {
        name: player.name.clone().unwrap_or_else(|| match seat {
            Seat::Blue => "blue".to_owned(),
            Seat::Red => "red".to_owned(),
        }),
        opening: if has_opening {
            opening_taken(economy, player, seat, dealt)?
        } else {
            Opening {
                choose: None,
                offers: dealt,
            }
        },
        // The map deals the layout before the first round and nothing adds to
        // it, so the first round's list is the one the side started with.
        constructions: constructions(&player.rounds.entries[OPENING_ROUNDS].data, seat)?,
        tech_loadout: loadout,
        seed: Some(player.seed),
    })
}

/// The opening a side took, which the record logs as round 0's one decision.
///
/// # Errors
///
/// Returns an error when round 0 stands for anything other than one opening
/// choice, which is the only decision that round can hold.
fn opening_taken(
    economy: &Economy,
    player: &record::PlayerRecord,
    seat: Seat,
    dealt: Vec<OpeningOffer>,
) -> Result<Opening, String> {
    let round = &player.rounds.entries[0];
    let taken = net_actions(&round.actions.entries);
    let [action] = taken.as_slice() else {
        return Err(format!(
            "{} takes {} decisions in the opening, and the opening is one",
            seat.name(),
            taken.len()
        ));
    };
    if action.kind != "PAD_ChooseAdvanceTeam" {
        return Err(format!(
            "{} opens with {}, and an opening is a team choice",
            seat.name(),
            action.kind
        ));
    }
    let offer = action
        .index
        .ok_or("PAD_ChooseAdvanceTeam has no Index".to_string())?;
    let team = action
        .id
        .ok_or("PAD_ChooseAdvanceTeam has no ID".to_string())?;
    let specialist = opening_specialist(economy, player)?;
    let taken = usize::try_from(offer)
        .ok()
        .and_then(|at| dealt.get(at))
        .ok_or_else(|| {
            format!(
                "{} took opening {offer}, and the deal holds {}",
                seat.name(),
                dealt.len()
            )
        })?;
    // Server operations retain the selected offer index but omit its team ID.
    let team = if team == -1 && round.data.special_supply {
        taken.team
    } else {
        team
    };
    if taken.team != team || taken.specialist != specialist {
        return Err(format!(
            "{} took team {team} with specialist {specialist:?} at offer {offer}, \
             and the seed deals {taken:?} there",
            seat.name()
        ));
    }
    Ok(Opening {
        choose: Some(offer),
        offers: dealt,
    })
}

fn side_state(
    grbr: &[u8],
    economy: &Economy,
    player: &record::PlayerRecord,
    position: usize,
    seat: Seat,
) -> Result<SideState, String> {
    let entry = &player.rounds.entries[position];
    let data = &entry.data;
    let round = entry.round;
    shared_income(economy, player, seat)?;

    let formations = formations(data, seat)?;
    let constructions = constructions(data, seat)?;
    let contraptions = contraptions(data, seat)?;

    // The snapshot is taken before the round opens, so its cooldowns are the
    // previous round's, and the slots the previous round spent are marked
    // for the opening to restart. The actions say which were spent, and the
    // snapshot marks the same ones active.
    let previous = previous_round(player, position);
    let spent: Vec<i32> = previous
        .map(|previous| {
            net_actions(&previous.actions.entries)
                .iter()
                .filter(|action| action.kind == "PAD_ReleaseCommanderSkill")
                .filter_map(|action| action.skill_index)
                .collect()
        })
        .unwrap_or_default();
    let mut battle_skills: Vec<PanelSkill> = data
        .commander_skills
        .entries
        .iter()
        .map(|skill| {
            let previous_known = previous.is_some();
            let used = if previous_known {
                spent.contains(&skill.index)
            } else {
                skill.active
            };
            if previous_known && skill.active != used {
                return Err(format!(
                    "round {round} {} marks slot {} {}, and the round before {} it",
                    seat.name(),
                    skill.index,
                    if skill.active { "active" } else { "inactive" },
                    if used { "spent" } else { "did not spend" },
                ));
            }
            Ok(PanelSkill {
                index: skill.index,
                id: skill.id,
                cooldown: skill.cooling_round,
                used,
                standing: Vec::new(),
                release: None,
            })
        })
        .collect::<Result<_, String>>()?;
    battle_skills.sort_by_key(|skill| skill.index);

    stand_on_panel(grbr, round, seat, &mut battle_skills)?;

    let mut officers: Vec<i32> = data
        .officers
        .values
        .iter()
        .copied()
        .filter(|officer| !CHAIN_OFFICERS.contains(officer))
        .collect();
    officers.sort_unstable();

    let mut units: Vec<i32> = data
        .active_technologies
        .entries
        .iter()
        .flat_map(|row| row.techs.entries.iter().map(|tech| tech.data))
        .collect();
    units.sort_unstable();

    let mut unlocked_units = data.shop.unlocked_units.values.clone();
    unlocked_units.sort_unstable();

    let snapshot = SideState {
        random_state: [0; 4],
        reactor_core: data.reactor_core,
        // The snapshot precedes the round's income, which the opening pays.
        supply: data.supply,
        unlocked_units,
        // The allowances are the opening's to set.
        shop: crate::r#match::Allowances::default(),
        blueprints: active_blueprints(economy, data),
        // What the previous round activated and still owes for, which the
        // opening charges against the income and then lapses.
        energy_tower_skills: if energy_tower_debt(player, position, seat)? {
            vec![RAPID_SUPPLY_SKILL]
        } else {
            Vec::new()
        },
        tower_strengthen_levels: data.tower_strengthen_levels.values.clone(),
        equipment: unfitted_equipment(data),
        battle_skills,
        next_index: NextIndex {
            unit: data.unit_index,
            contraption: data.contraption_index,
        },
        officers,
        techs: units,
        units: formations,
        constructions,
        contraptions,
    };

    // The snapshot is taken before the round opens: before its resets, and
    // before its deliveries, which the game makes before either side decides
    // anything. Both belong to the position the round opens with, so the
    // opening is made here, and each delivery lands where the board puts it.
    let mut placement = crate::landing::placement(seat == Seat::Red);
    let stream = player_stream(economy, player, position, seat)?;
    let mut opened =
        crate::transition::open_round(economy, &snapshot, round, &mut placement, Some(stream))
            .map_err(|reason| format!("round {round} {} delivery: {reason:?}", seat.name()))?;
    if data.special_supply {
        // PlayerSnapshotController.ApplySnapshot locks Player.AddRoundSupply:
        // NetworkMessageData.Convert already added roundSupply to supply.
        opened.supply = data.supply;
    }
    Ok(opened)
}

/// The server retains every completed research, while the native manager
/// replaces a chain's first stage with its successor.
fn active_blueprints(economy: &Economy, data: &PlayerData) -> Vec<i32> {
    let mut blueprints = data.blueprints.values.clone();
    blueprints.retain(|id| {
        !economy
            .blueprint_successor(*id)
            .is_some_and(|next| data.blueprints.values.contains(&next))
    });
    blueprints.sort_unstable();
    blueprints
}

/// Puts what earlier releases left standing, as the round's snapshot lists
/// it, on the panel slot whose skill released each, in normal-form order.
fn stand_on_panel(
    grbr: &[u8],
    round: i32,
    seat: Seat,
    panel: &mut [PanelSkill],
) -> Result<(), String> {
    let retained = retained_from_grbr_round(grbr, u32::try_from(round).unwrap_or(0))?;
    let retained = match seat {
        Seat::Blue => retained.blue,
        Seat::Red => retained.red,
    };
    for GrbrStanding { index, standing } in retained {
        let slot = panel
            .iter_mut()
            .find(|slot| slot.index == index)
            .ok_or_else(|| {
                format!(
                    "round {round} {} lists a standing object under panel slot {index}, which \
                     it does not hold",
                    seat.name()
                )
            })?;
        standing.require_skill(slot.id)?;
        slot.standing.push(standing);
    }
    for slot in panel {
        slot.standing
            .sort_by_cached_key(crate::layout::Standing::sort_key);
    }
    Ok(())
}

/// The side's own stream as round `position` opens, which is the snapshot's.
///
/// It is also where the seed the header states, advanced once for every
/// hand-out an earlier round drew, puts it; a snapshot anywhere else would be
/// a stream something else draws from, and is refused.
fn player_stream(
    economy: &Economy,
    player: &record::PlayerRecord,
    position: usize,
    seat: Seat,
) -> Result<Stream, String> {
    let entry = &player.rounds.entries[position];
    let recorded =
        <[u64; 4]>::try_from(entry.data.random_state.states.values.as_slice()).map_err(|_| {
            format!(
                "round {} {} records no player stream",
                entry.round,
                seat.name()
            )
        })?;
    if player
        .rounds
        .entries
        .get(1)
        .is_some_and(|first| first.round != 1)
        || player.rounds.entries[0].actions.entries.is_empty()
    {
        if let Some(previous) = previous_round(player, position) {
            let state = previous
                .data
                .random_state
                .states
                .values
                .as_slice()
                .try_into()
                .map_err(|_| "previous round records no player stream".to_owned())?;
            let mut stream = Stream::from_state(state)?;
            stream.skip(crate::transition::player_draws(
                economy,
                &previous.data.officers.values,
                previous.round,
            ));
            if stream.state() != recorded {
                return Err(format!(
                    "round {} {} player stream disagrees with the preceding recorded round",
                    entry.round,
                    seat.name()
                ));
            }
        }
        return Stream::from_state(recorded);
    }
    let draws: u32 = player.rounds.entries[..position]
        .iter()
        .map(|earlier| {
            crate::transition::player_draws(economy, &earlier.data.officers.values, earlier.round)
        })
        .sum();
    let mut stream = Stream::seeded(player.seed);
    stream.skip(draws);
    if stream.state() != recorded {
        return Err(format!(
            "round {} {} player stream is not where seed {} and {draws} earlier hand-outs put it",
            entry.round,
            seat.name(),
            player.seed
        ));
    }
    Ok(stream)
}

/// Refuses a side whose map pays a round income other than the one every
/// versus map shares. The opening pays the shared schedule, and the record
/// carries the map's own row, so a match on a map that pays differently would
/// otherwise be given an income its map never paid.
fn shared_income(
    economy: &Economy,
    player: &record::PlayerRecord,
    seat: Seat,
) -> Result<(), String> {
    let shared: RoundSupply = economy.round_supply();
    let recorded = (
        player.data.first_round_supply,
        player.data.round_supply_increase,
        player.data.max_round_supply,
    );
    if recorded == (shared.first, shared.increase, shared.max) {
        return Ok(());
    }
    Err(format!(
        "{} pays a round income of {recorded:?} (first, increase, max), and \
         every versus map pays {:?}",
        seat.name(),
        (shared.first, shared.increase, shared.max)
    ))
}

/// The unit roster, as the layout formations a projection would keep.
fn formations(data: &PlayerData, seat: Seat) -> Result<Vec<StateUnit>, String> {
    let mut board = SideState {
        constructions: constructions(data, seat)?,
        contraptions: contraptions(data, seat)?,
        ..SideState::default()
    };
    for unit in &data.units.entries {
        let (type_name, _) = unit_type_from_id(unit.id).ok_or_else(|| {
            format!(
                "unit ID {} has no layout type in build {}",
                unit.id,
                replay_version()
            )
        })?;
        // NetworkMessageData.Convert(CardInfo) writes this sentinel when the
        // server leaves a new unit unplaced. ApplyUnitSnapshot restores it
        // through MAD_AddUnit, whose normal landing rule chooses its position.
        let position = if (unit.position.x == i32::MIN && unit.position.y == i32::MIN)
            || (unit.position.x == 0 && unit.position.y == 0)
        {
            crate::landing::landing(&board, type_name, seat == Seat::Red)
                .ok_or_else(|| format!("no landing position for recorded unit {}", unit.index))?
        } else {
            if unit.position.x == i32::MIN || unit.position.y == i32::MIN {
                return Err(format!(
                    "recorded unit {} has an incomplete position",
                    unit.index
                ));
            }
            seat.position(&unit.position)
        };
        board.units.push(StateUnit {
            value: Some(unit.sell_supply),
            redeployed: false,
            unit: UnitPlacement {
                record_position: None,
                record_rotated: None,
                type_name: type_name.to_owned(),
                index: unit.index,
                position,
                // The record counts paid upgrades from zero; a layout displays the
                // level from one.
                level: Some(unit.level + 1).filter(|level| *level != 1),
                exp: Experience::of(unit.exp, type_name, unit.level + 1)?,
                rotated: Some(unit.rotated).filter(|rotated| *rotated),
                equipment: unit.equipments.entries.iter().map(|item| item.id).collect(),
                // No recorded field states it; see docs/spec/document/match.md.
                travelling: None,
                source: crate::layout::UnitSource::Legacy,
            },
        });
    }
    board.units.sort_by_key(|entry| entry.unit.index);
    Ok(board.units)
}

/// The snapshot portion of a synthetic layout replay. Its restoration commands
/// are read separately; this shares the native-to-document mapping with matches.
pub(crate) fn layout_snapshot(data: &PlayerData, red: bool) -> Result<crate::Side, String> {
    let seat = if red { Seat::Red } else { Seat::Blue };
    Ok(crate::Side {
        officers: data
            .officers
            .values
            .iter()
            .copied()
            .filter(|id| !CHAIN_OFFICERS.contains(id))
            .collect(),
        techs: data
            .active_technologies
            .entries
            .iter()
            .flat_map(|row| row.techs.entries.iter().map(|tech| tech.data))
            .collect(),
        blueprints: data.blueprints.values.clone(),
        energy_tower_skills: Vec::new(),
        tower_strengthen_levels: data.tower_strengthen_levels.values.clone(),
        units: formations(data, seat)?
            .into_iter()
            .map(|entry| entry.unit)
            .collect(),
        recovered: Vec::new(),
        constructions: constructions(data, seat)?,
        contraptions: contraptions(data, seat)?,
        battle_skills: Vec::new(),
    })
}

fn constructions(data: &PlayerData, seat: Seat) -> Result<Vec<StaticPlacement>, String> {
    let mut constructions = Vec::with_capacity(data.constructions.entries.len());
    for construction in &data.constructions.entries {
        let (type_name, _) = construction_type_from_id(construction.id).ok_or_else(|| {
            format!(
                "construction ID {} has no layout type in build {}",
                construction.id,
                replay_version()
            )
        })?;
        constructions.push(StaticPlacement {
            type_name: type_name.to_owned(),
            index: construction.index,
            position: seat.position(&construction.position),
        });
    }
    constructions.sort_by_key(|construction| construction.index);
    Ok(constructions)
}

fn contraptions(data: &PlayerData, seat: Seat) -> Result<Vec<ContraptionPlacement>, String> {
    let mut contraptions = Vec::with_capacity(data.contraptions.entries.len());
    for contraption in &data.contraptions.entries {
        let type_name = contraption_type_from_id(contraption.id).ok_or_else(|| {
            format!(
                "contraption ID {} has no layout type in build {}",
                contraption.id,
                replay_version()
            )
        })?;
        contraptions.push(ContraptionPlacement {
            type_name: type_name.to_owned(),
            index: contraption.index,
            position: seat.position(&contraption.position),
        });
    }
    contraptions.sort_by_key(|contraption| contraption.index);
    Ok(contraptions)
}

/// The recorded inventory holds fitted items too, and a state stores the rest.
fn unfitted_equipment(data: &PlayerData) -> Vec<EquipmentItem> {
    let mut fitted: Vec<i32> = data
        .units
        .entries
        .iter()
        .flat_map(|unit| unit.equipments.entries.iter().map(|item| item.id))
        .collect();
    let mut unfitted = Vec::new();
    for item in &data.equipment.entries {
        if let Some(position) = fitted.iter().position(|id| *id == item.id) {
            fitted.swap_remove(position);
            continue;
        }
        unfitted.push(EquipmentItem {
            id: item.id,
            durability: Some(item.durability).filter(|durability| *durability != -1),
        });
    }
    unfitted.sort_unstable();
    unfitted
}

/// Whether last round's Rapid Supply is deducted from this round's income.
///
/// Two readings of one fact have to agree, and this is where they meet. The
/// debt follows from the previous round's own decisions, which is what the
/// ledger prices. The game also snapshots it: the activation flag survives into
/// the round after the one that set it, so a round's recorded
/// `energyTowerSkills` names the previous round's activation rather than its
/// own, and only skill `1` has a deferred half to be snapshotted at all.
///
/// # Errors
///
/// Returns an error when the two disagree, which means either the collapse of
/// the previous round's actions or the reading of the recorded list is wrong.
/// Guessing which would put a supply figure into the document that no reading
/// supports.
fn energy_tower_debt(
    player: &record::PlayerRecord,
    position: usize,
    seat: Seat,
) -> Result<bool, String> {
    let entry = &player.rounds.entries[position];
    let recorded = entry.data.energy_tower_skills.values.as_slice();
    let previous = previous_round(player, position);
    if previous.is_none() {
        return match recorded {
            [] => Ok(false),
            [RAPID_SUPPLY_SKILL] => Ok(true),
            _ => Err(format!(
                "{} round {} has unsupported energy tower debt {recorded:?}",
                seat.name(),
                entry.round
            )),
        };
    }
    let owed = previous.is_some_and(|previous| {
        net_actions(&previous.actions.entries).iter().any(|action| {
            action.kind == "PAD_ActiveEnergyTowerSkill"
                && action.skill_id == Some(RAPID_SUPPLY_SKILL)
        })
    });
    // NetworkMessageData.Convert does not populate energyTowerSkills. The
    // recorded actions still identify the debt, and special_supply already
    // includes its effect on the coming income.
    if entry.data.special_supply && recorded.is_empty() {
        return Ok(owed);
    }
    let expected: &[i32] = if owed { &[RAPID_SUPPLY_SKILL] } else { &[] };
    if recorded == expected {
        return Ok(owed);
    }
    Err(format!(
        "{} round {} records energy tower skills {recorded:?}, and the round before it \
         decided {expected:?}; see docs/spec/document/match.md",
        seat.name(),
        entry.round
    ))
}

/// A cached placeholder is not evidence of the preceding round's decisions.
fn previous_round(player: &record::PlayerRecord, position: usize) -> Option<&PlayerRoundRecord> {
    let entry = player.rounds.entries.get(position)?;
    player
        .rounds
        .entries
        .get(position.checked_sub(1)?)
        .filter(|previous| {
            previous.round.checked_add(1) == Some(entry.round)
                && (previous.round > 0 || !previous.actions.entries.is_empty())
        })
}

/// Collapses a recorded action list onto the decisions that took effect.
///
/// Every recorded action is one entry on the undo stack, and `Undo` pops the
/// newest entry whether or not it still stands for a decision. That is what
/// distinguishes this from popping the newest surviving decision: a cancelled
/// release stays on the stack as an entry, and so does the cancel itself, so
/// the two of them absorb two undos between them.
///
/// One player-round settles it. A side chose a card, bought two units, moved
/// one of them twice, released a skill, cancelled it, released it again,
/// unlocked a unit, and then pressed undo seven times. Its next snapshot keeps
/// the card and one of the two units, which is what stepping back over seven
/// recorded entries leaves and is two entries further than stepping back over
/// seven surviving decisions.
///
/// `Redo` pushes the newest undone entry back, and any other action clears what
/// could be redone. `docs/spec/document/action.md` states the rule.
fn net_actions(recorded: &[ActionRecord]) -> Vec<&ActionRecord> {
    /// One recorded action on the undo stack: whether it still stands for a
    /// decision, and for a cancel, where on the stack its release sits.
    struct Entry<'a> {
        action: &'a ActionRecord,
        stands: bool,
        cancels: Option<usize>,
    }
    let mut taken: Vec<Entry> = Vec::with_capacity(recorded.len());
    let mut undone: Vec<Entry> = Vec::new();
    for action in recorded {
        match action.kind.as_str() {
            // Undoing a cancel stands its release again, and redoing it spends
            // the release once more: the release sits below the cancel, so it
            // is on the stack whenever the cancel is.
            "PAD_Undo" => {
                if let Some(last) = taken.pop() {
                    if let Some(release) = last.cancels {
                        taken[release].stands = true;
                    }
                    undone.push(last);
                }
            }
            "PAD_Redo" => {
                if let Some(last) = undone.pop() {
                    if let Some(release) = last.cancels {
                        taken[release].stands = false;
                    }
                    taken.push(last);
                }
            }
            "PAD_CancelReleaseCommanderSkill" => {
                undone.clear();
                let release = taken.iter().rposition(|entry| {
                    entry.stands
                        && entry.action.kind == "PAD_ReleaseCommanderSkill"
                        && entry.action.skill_index == action.skill_index
                });
                if let Some(release) = release {
                    taken[release].stands = false;
                }
                taken.push(Entry {
                    action,
                    stands: false,
                    cancels: release,
                });
            }
            "PAD_FinishDeploy" => undone.clear(),
            _ => {
                undone.clear();
                taken.push(Entry {
                    action,
                    stands: true,
                    cancels: None,
                });
            }
        }
    }
    taken
        .into_iter()
        .filter_map(|entry| entry.stands.then_some(entry.action))
        .collect()
}

/// The opening specialist a side ends round 0 holding.
///
/// The record logs the team half of the opening and not the specialist half,
/// so the specialist is read back from the officer list of the round the
/// opening produced. Exactly one officer of a side is an opening specialist,
/// in every player-round of the local set.
fn opening_specialist(economy: &Economy, player: &record::PlayerRecord) -> Result<i32, String> {
    let next = player
        .rounds
        .entries
        .get(OPENING_ROUNDS)
        .ok_or("opening has no following deployment round")?;
    let specialists: Vec<i32> = next
        .data
        .officers
        .values
        .iter()
        .copied()
        .filter(|officer| {
            economy
                .advance_team(*officer)
                .is_some_and(|team| team.kind == OpeningKind::Officer)
        })
        .collect();
    match specialists.as_slice() {
        [specialist] => Ok(*specialist),
        _ => Err(format!(
            "opening must grant exactly one specialist, found {specialists:?}"
        )),
    }
}

/// A recorded decision, before the round is stepped.
///
/// A release records the panel slot, and the skill the slot holds is whatever
/// the round has put there by then, so stepping the round is what names it.
enum Recorded {
    Taken(Action),
    Released { index: i32, target: SkillTarget },
}

/// A side's decisions in one round, from the position the round opened with.
///
/// Each decision is stepped as it is read, which names the skill a release's
/// slot holds and the index a purchase creates. Only the resulting position
/// reaches the normalizer, which reconstructs the standard sequence from the
/// two endpoint positions and checks its result.
fn actions(
    economy: &Economy,
    round: &PlayerRoundRecord,
    seat: Seat,
    opened: &SideState,
    offers: Option<&Offers>,
) -> Result<Vec<Action>, String> {
    let red = seat == Seat::Red;
    let declined = offers.map(|offers| offers.refund);
    let step = |position: &SideState, action: &Action, at: usize| {
        crate::transition::step_placing(
            economy,
            position,
            action,
            declined,
            &mut crate::landing::placement(red),
        )
        .map_err(|reason| {
            format!(
                "{} decision {at} ({action:?}) cannot be stepped: {reason:?}",
                seat.name()
            )
        })
    };
    let mut position = opened.clone();
    let mut dependencies = crate::normalize::UpgradeDependencies::default();
    let mut conceded = false;
    let mut decided = false;
    for (at, recorded) in recorded_actions(round, seat, offers)?
        .into_iter()
        .enumerate()
    {
        let action = match recorded {
            Recorded::Taken(action) => action,
            Recorded::Released { index, target } => {
                let id = position
                    .battle_skills
                    .iter()
                    .find(|skill| skill.index == index)
                    .map(|skill| skill.id)
                    .ok_or_else(|| {
                        format!(
                            "{} releases panel slot {index}, which it does not hold",
                            seat.name()
                        )
                    })?;
                Action::ReleaseCommanderSkill { index, id, target }
            }
        };
        conceded |= matches!(action, Action::Concede);
        decided |= !matches!(action, Action::Concede);
        position = step(&position, &action, at)?;
        dependencies.record(&position, &action);
    }
    if !decided {
        return Ok(if conceded {
            vec![Action::Concede]
        } else {
            Vec::new()
        });
    }
    let mut normalized =
        crate::normalize::deployment(economy, opened, &position, offers, red, &dependencies)
            .map_err(|error| format!("{} {error}", seat.name()))?;
    if conceded {
        normalized.push(Action::Concede);
    }
    Ok(normalized)
}

/// The `Index` the game records a decline at, which is no position in the
/// round's offers.
const RECORDED_DECLINE: i32 = -1;

/// Server picks name a card but omit its offer index; zero alone is decline.
fn reinforcement_choice(
    action: &ActionRecord,
    offers: Option<&Offers>,
    special_supply: bool,
) -> Result<Action, String> {
    let mut index = action.index.ok_or("PAD_ChooseReinforceItem has no Index")?;
    if index == RECORDED_DECLINE && special_supply {
        let id = action.id.ok_or("PAD_ChooseReinforceItem has no ID")?;
        if id != 0 {
            index = offers
                .and_then(|offers| offers.dealt.iter().position(|item| *item == id))
                .and_then(|at| i32::try_from(at).ok())
                .ok_or_else(|| format!("server choice {id} is not among the round's offers"))?;
        }
    }
    if index == RECORDED_DECLINE {
        Ok(Action::ChooseReinforceItem {
            index: offers
                .map(Offers::decline_index)
                .ok_or("a decline in a round that deals no offers")?,
            id: None,
        })
    } else {
        Ok(Action::ChooseReinforceItem {
            index,
            id: Some(action.id.ok_or("PAD_ChooseReinforceItem has no ID")?),
        })
    }
}

fn recorded_actions(
    round: &PlayerRoundRecord,
    seat: Seat,
    offers: Option<&Offers>,
) -> Result<Vec<Recorded>, String> {
    let mut converted = Vec::new();
    for action in net_actions(&round.actions.entries) {
        let field = |name: &'static str, value: Option<i32>| {
            value.ok_or_else(|| format!("{} has no {name}", action.kind))
        };
        let position = |value: &Option<record::PositionRecord>| {
            value
                .as_ref()
                .map(|position| seat.position(position))
                .ok_or_else(|| format!("{} has no position", action.kind))
        };
        converted.push(Recorded::Taken(match action.kind.as_str() {
            "PAD_ChooseReinforceItem" => {
                reinforcement_choice(action, offers, round.data.special_supply)?
            }
            "PAD_BuyUnit" => Action::BuyUnit {
                unit: field("UID", action.unit_id)?,
                level: None,
                position: Some(position(&action.buy_position)?),
                rotated: false,
            },
            "PAD_UpgradeUnit" => Action::UpgradeUnit {
                index: field("UIDX", action.unit_index_allocated)?,
            },
            "PAD_UnlockUnit" => Action::UnlockUnit {
                unit: field("UID", action.unit_id)?,
            },
            "PAD_UpgradeTechnology" => Action::UpgradeTechnology {
                unit: field("UID", action.unit_id)?,
                tech: field("TechID", action.tech_id)?,
            },
            "PAD_ActiveBlueprint" => Action::ActiveBlueprint {
                id: field("ID", action.id)?,
            },
            "PAD_ActiveEnergyTowerSkill" => Action::ActiveEnergyTowerSkill {
                skill: field("SkillID", action.skill_id)?,
            },
            "PAD_StrengthenTower" => Action::StrengthenTower {
                tower: field("Index", action.index)?,
            },
            "PAD_UseEquipment" => Action::UseEquipment {
                equipment: field("EquipmentID", action.equipment_id)?,
                index: field("UnitIndex", action.unit_index)?,
            },
            "PAD_ReleaseCommanderSkill" => {
                converted.push(Recorded::Released {
                    index: field("SkillIndex", action.skill_index)?,
                    target: skill_target(action, seat)?,
                });
                continue;
            }
            "PAD_ReleaseContraption" => Action::ReleaseContraption {
                contraption: field("ContraptionID", action.contraption_id)?,
                position: position(&action.release_position)?,
                extra_position: action
                    .extra_position
                    .as_ref()
                    .filter(|extra| extra.x != 0 || extra.y != 0)
                    .map(|extra| seat.position(extra)),
            },
            "PAD_MoveUnit" => {
                // One recorded move can carry several units. Order is all a turn
                // keeps, and the collapse has already run, so the batch flattens.
                let moves = action
                    .moves
                    .as_ref()
                    .ok_or_else(|| "PAD_MoveUnit has no moveUnitDatas".to_owned())?;
                for moved in &moves.entries {
                    converted.push(Recorded::Taken(Action::MoveUnit {
                        index: moved.unit_index,
                        position: seat.position(&moved.position),
                        rotated: moved.rotated,
                    }));
                }
                continue;
            }
            "PAD_GiveUp" => Action::Concede,
            other => return Err(format!("action {other} has no turn representation")),
        }));
    }
    Ok(converted)
}

/// Resolves the exclusive target of a release.
///
/// The recorded shape is not exclusive: a pointing release also carries the
/// player's click point, which names no state. `docs/spec/document/state.md` states why the
/// resolved target is stored and the coordinate is dropped.
fn skill_target(action: &ActionRecord, seat: Seat) -> Result<SkillTarget, String> {
    if let Some(unit) = action.unit_index.filter(|index| *index >= 0) {
        return Ok(SkillTarget::Unit(unit));
    }
    if let Some(construction) = action.construction_index.filter(|index| *index >= 0) {
        return Ok(SkillTarget::Construction(construction));
    }
    let positions = action
        .positions
        .as_ref()
        .map(|positions| {
            positions
                .entries
                .iter()
                .map(|position| seat.position(position))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if positions.is_empty() {
        return Err("PAD_ReleaseCommanderSkill names neither an object nor an area".into());
    }
    Ok(SkillTarget::Area(positions))
}

/// Unit IDs a state may hold, for the catalogue coverage test.
#[cfg(test)]
mod tests {
    use crate::Position;
    use crate::r#match::Action;

    fn opening_side(
        economy: &crate::economy::Economy,
        name: &str,
        offers: Vec<crate::r#match::OpeningOffer>,
        constructions: Vec<crate::StaticPlacement>,
        seed: i32,
    ) -> (crate::r#match::MatchSide, crate::r#match::SideState) {
        use crate::r#match::{MatchSide, Opening};
        let taken = &offers[0];
        let state = crate::transition::predict(
            economy,
            0,
            &crate::transition::before_opening(4500, constructions.clone()),
            &[Action::ChooseAdvanceTeam {
                index: 0,
                id: taken.team,
                specialist: taken.specialist,
            }],
            name == "red",
            None,
            Some(crate::opening::Stream::seeded(seed)),
        )
        .unwrap();
        (
            MatchSide {
                name: name.into(),
                opening: Opening {
                    choose: Some(0),
                    offers,
                },
                constructions,
                seed: Some(seed),
                tech_loadout: std::collections::BTreeMap::default(),
            },
            state,
        )
    }

    #[test]
    fn a_server_opening_starts_before_pool_setup_and_keeps_its_chosen_offer() {
        use crate::r#match::{Match, State, Turn, TurnActions};
        let economy = crate::economy::Economy::embedded().unwrap();
        let predicted = crate::opening::predict(economy, 12345, 1012).unwrap();
        let (blue, blue_state) = opening_side(
            economy,
            "blue",
            predicted.deal.blue,
            predicted.constructions.blue,
            7,
        );
        let (red, red_state) = opening_side(
            economy,
            "red",
            predicted.deal.red,
            predicted.constructions.red,
            9,
        );
        let mut game = Match {
            has_opening: true,
            game_build: crate::game_build().into(),
            map_id: 1012,
            seed: 12345,
            deploy_time: Some(100),
            blue,
            red,
            turns: vec![Turn {
                round: 1,
                state: State {
                    blue: blue_state,
                    red: red_state,
                    ..Default::default()
                },
                actions: TurnActions::default(),
            }],
        };
        crate::reinforcement::fill_checkpoints(economy, &mut game).unwrap();
        let yaml = crate::r#match::canonical_yaml(&game).unwrap();
        let stated = crate::opening::stated(yaml.as_bytes()).unwrap().unwrap();
        let native =
            crate::match_replay::match_replay(economy, &stated, crate::game_build()).unwrap();
        let expected = super::document(economy, &native).unwrap().yaml;
        let words = |state: [u64; 4]| {
            state
                .map(|word| format!("<unsignedLong>{word}</unsignedLong>"))
                .concat()
        };
        let xml = crate::record::embedded_xml(&native).unwrap();
        let mut server = xml
            .replace(
                &words(predicted.initialization.stream.state()),
                &words(crate::opening::Stream::seeded(12345).state()),
            )
            .replace("<Seat>0</Seat>", "<Seat>-1</Seat>");
        // Only the opening snapshots select the RNG phase and absent team ID.
        server = server
            .split_inclusive("</PlayerRoundRecord>")
            .map(|part| {
                if part.contains("<round>0</round><playerData>") {
                    part.replace(
                        "<IsSpecialSupply>false</IsSpecialSupply>",
                        "<IsSpecialSupply>true</IsSpecialSupply>",
                    )
                } else {
                    part.to_owned()
                }
            })
            .collect();
        for side in [&game.blue, &game.red] {
            server = server.replace(
                &format!("<ID>{}</ID>", side.opening.offers[0].team),
                "<ID>-1</ID>",
            );
        }
        assert_eq!(
            super::document(economy, server.as_bytes()).unwrap().yaml,
            expected
        );
        let invalid = server.replace(
            &words(crate::opening::Stream::seeded(12345).state()),
            &words([1, 2, 3, 4]),
        );
        assert!(
            super::document(economy, invalid.as_bytes())
                .err()
                .unwrap()
                .contains("opening round recorded")
        );
    }

    fn snapshot_round(special: bool, actions: &str) -> String {
        let states = crate::opening::Stream::seeded(7)
            .state()
            .map(|value| format!("<unsignedLong>{value}</unsignedLong>"))
            .concat();
        format!(
            "<PlayerRoundRecord><round>5</round><playerData>\
             <IsSpecialSupply>{special}</IsSpecialSupply>\
             <reactorCore>4500</reactorCore><supply>1250</supply>\
             <unitIndex>0</unitIndex><contraptionIndex>0</contraptionIndex>\
             <bluepints><int>4</int><int>401</int></bluepints>\
             <randomStateData><randomStates>{states}</randomStates></randomStateData>\
             <shop><BuyCount>0</BuyCount><UnlockCount>0</UnlockCount></shop>\
             </playerData><actionRecords>{actions}</actionRecords></PlayerRoundRecord>"
        )
    }

    #[test]
    fn an_unanswered_round_does_not_invent_a_reinforcement_choice() {
        for (recorded, expected) in [
            ("", Vec::new()),
            (
                "<MatchActionData xsi:type=\"PAD_GiveUp\"><Time>0</Time><LocalTime>2</LocalTime></MatchActionData>",
                vec![Action::Concede],
            ),
        ] {
            let round = quick_xml::de::from_str(&snapshot_round(false, recorded)).unwrap();
            let found = super::actions(
                crate::economy::Economy::embedded().unwrap(),
                &round,
                super::Seat::Blue,
                &crate::r#match::SideState::default(),
                Some(&crate::r#match::Offers {
                    dealt: vec![20_003],
                    refund: 50,
                }),
            )
            .unwrap();
            assert_eq!(found, expected);
        }
    }

    #[test]
    fn server_supply_is_already_paid_and_completed_research_has_one_active_stage() {
        let economy = crate::economy::Economy::embedded().unwrap();
        for (special, supply) in [(true, 1250), (false, 2250)] {
            let round = snapshot_round(special, "");
            let player = format!(
                "<PlayerRecord><seed>7</seed><data>\
                 <maxRoundSupply>4000</maxRoundSupply><firstRoundSupply>200</firstRoundSupply>\
                 <roundSupplyIncreaseValue>200</roundSupplyIncreaseValue></data>\
                 <playerRoundRecords>{round}</playerRoundRecords></PlayerRecord>"
            );
            let parsed = quick_xml::de::from_str(&player).unwrap();
            let grbr = format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?><BattleRecord>\
                 <playerRecords>{player}{player}</playerRecords></BattleRecord>"
            );
            let state =
                super::side_state(grbr.as_bytes(), economy, &parsed, 0, super::Seat::Blue).unwrap();
            assert_eq!(state.supply, supply);
            assert_eq!(state.blueprints, [401]);
        }
    }

    #[test]
    fn unplaced_server_units_land_in_each_sides_frame() {
        for coordinate in [i32::MIN, 0] {
            let unit = format!(
                "<units><NewUnitData><id>2</id><Index>0</Index><Exp>0</Exp><Level>0</Level>\
                 <Position><x>{coordinate}</x><y>{coordinate}</y></Position>\
                 <IsRotate>false</IsRotate><SellSupply>100</SellSupply></NewUnitData></units>"
            );
            let xml = snapshot_round(true, "").replace("<unitIndex>0</unitIndex>", &unit);
            // The snapshot still needs the allocator, independently of its units.
            let xml = xml.replace("</units>", "</units><unitIndex>1</unitIndex>");
            let round: crate::record::PlayerRoundRecord = quick_xml::de::from_str(&xml).unwrap();
            for seat in [super::Seat::Blue, super::Seat::Red] {
                let units = super::formations(&round.data, seat).unwrap();
                assert_eq!(units[0].unit.position, Position { x: 0, y: -160 });
            }
        }
    }

    #[test]
    fn server_card_ids_resolve_missing_indices_without_turning_picks_into_declines() {
        let offers = crate::r#match::Offers {
            dealt: vec![31802, 1_200_001],
            refund: 50,
        };
        for (id, index, expected) in [
            (31802, -1, 0),
            (1_200_001, -1, 1),
            (0, -1, 2),
            (31802, 0, 0),
        ] {
            let xml = snapshot_round(
                true,
                &format!(
                    "<MatchActionData type=\"PAD_ChooseReinforceItem\"><ID>{id}</ID>\
                 <Index>{index}</Index></MatchActionData>"
                ),
            );
            let round = quick_xml::de::from_str(&xml).unwrap();
            let actions =
                super::recorded_actions(&round, super::Seat::Blue, Some(&offers)).unwrap();
            let [super::Recorded::Taken(Action::ChooseReinforceItem { index, id: chosen })] =
                actions.as_slice()
            else {
                panic!("not a card choice")
            };
            assert_eq!(*index, expected);
            assert_eq!(*chosen, (id != 0).then_some(id));
        }
        let xml = snapshot_round(
            true,
            "<MatchActionData type=\"PAD_ChooseReinforceItem\"><ID>999</ID><Index>-1</Index></MatchActionData>",
        );
        let round = quick_xml::de::from_str(&xml).unwrap();
        assert!(
            super::recorded_actions(&round, super::Seat::Blue, Some(&offers))
                .err()
                .unwrap()
                .contains("not among")
        );
    }

    /// Undoing a cancel stands the release it cancelled again. A side
    /// released Orbital Javelin, cancelled it, released it again and then
    /// undid past the cancel; its next snapshots show the javelin spent,
    /// which only the first release, standing again, accounts for.
    #[test]
    fn undoing_a_cancel_stands_its_release_again() {
        let entry = |kind: &str, index: &str| {
            format!(r#"<MatchActionData xsi:type="{kind}">{index}</MatchActionData>"#)
        };
        let skill = |at: i32| format!("<SkillIndex>{at}</SkillIndex>");
        let recorded = [
            entry("PAD_ChooseReinforceItem", "<ID>300007</ID><Index>3</Index>"),
            entry("PAD_ReleaseCommanderSkill", &skill(2)),
            entry("PAD_CancelReleaseCommanderSkill", &skill(2)),
            entry("PAD_ReleaseCommanderSkill", &skill(2)),
            entry("PAD_Undo", ""),
            entry("PAD_Undo", ""),
            entry("PAD_ReleaseCommanderSkill", &skill(0)),
        ]
        .concat();
        let records: crate::record::ActionRecords =
            quick_xml::de::from_str(&format!("<actionRecords>{recorded}</actionRecords>")).unwrap();
        let kept: Vec<_> = super::net_actions(&records.entries)
            .iter()
            .map(|action| (action.kind.as_str(), action.skill_index))
            .collect();
        assert_eq!(
            kept,
            [
                ("PAD_ChooseReinforceItem", None),
                ("PAD_ReleaseCommanderSkill", Some(2)),
                ("PAD_ReleaseCommanderSkill", Some(0)),
            ]
        );
    }

    /// A round's log is the deal's but for the order of its choices: the
    /// players' own order is kept, and a match does not state it. Anything
    /// else another pool would restore is refused.
    #[test]
    fn a_pool_log_may_order_only_its_choices_otherwise() {
        use crate::reinforcement::{Pool, level_four_skills};
        use std::fmt::Write as _;
        let skills = level_four_skills().unwrap();
        let rounds = |log: &[(i32, i32)], excluded: &[i32]| {
            let log = log.iter().fold(String::new(), |mut all, (operation, id)| {
                let _ = write!(
                    all,
                    "<ValueTupleOfInt32Int32><Item1>{operation}</Item1><Item2>{id}</Item2>\
                     </ValueTupleOfInt32Int32>"
                );
                all
            });
            let values = skills.iter().fold(String::new(), |mut all, id| {
                let _ = write!(all, "<Value>{id}</Value>");
                all
            });
            let excluded = excluded.iter().fold(String::new(), |mut all, round| {
                let _ = write!(
                    all,
                    "<DictItem><Key>{round}</Key><Values>{values}</Values></DictItem>"
                );
                all
            });
            let xml = format!(
                "<matchDatas><MatchSnapshotData><round>0</round></MatchSnapshotData>\
                 <MatchSnapshotData><round>1</round><poolOPs>{log}</poolOPs>\
                 <RoundExcludeReinforce>{excluded}</RoundExcludeReinforce></MatchSnapshotData>\
                 </matchDatas>"
            );
            quick_xml::de::from_str::<crate::record::MatchRounds>(&xml)
                .unwrap()
                .entries
        };
        // A refresh removes 30703 and adds 30701 in its place, then both
        // players choose.
        let pools = [Pool {
            log: vec![(0, 30703), (1, 30701), (0, 31801), (0, 32103)],
            choices: std::iter::once(2..4).collect(),
            excluded: vec![6],
        }];
        let check = |log: &[(i32, i32)], excluded: &[i32]| {
            super::pool_lands_on_the_recorded_log(&rounds(log, excluded), &pools)
        };
        assert!(check(&pools[0].log, &[6]).is_ok());
        assert!(check(&[(0, 30703), (1, 30701), (0, 32103), (0, 31801)], &[6]).is_ok());
        assert!(check(&[(1, 30701), (0, 30703), (0, 31801), (0, 32103)], &[6]).is_err());
        assert!(check(&[(0, 30703), (0, 31801), (0, 32103)], &[6]).is_err());
        assert!(check(&pools[0].log, &[]).is_err());
    }
}
