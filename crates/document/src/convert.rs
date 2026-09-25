//! Fills a battle document from one recorded match.
//!
//! Most fields are copied. Four are rebuilt, because the snapshot the game
//! writes precedes the round's own reset: `supply` gains the round's income,
//! the two shop counters are restored to the round's allowance, the energy
//! tower list is the set activated during the round and so empty at its
//! start, and the equipment list drops what the formations carry.
//! `docs/spec/document/battle.md` says what the conversion refuses.

use crate::battle::{
    Action, Battle, BattleSide, DECLINED_OFFER, EquipmentItem, NextIndex, Opening, OpeningOffer,
    PanelSkill, ShopState, SideState, SkillTarget, State, StateUnit, Turn, TurnActions,
};
use crate::catalog::{construction_type_from_id, contraption_type_from_id, unit_type_from_id};
use crate::economy::{Economy, OpeningKind, RoundSupply};
use crate::layout::{
    ContraptionPlacement, Experience, Position, Region, StaticPlacement, UnitPlacement,
};
use crate::opening::{self, Stream};
use crate::record::{self, ActionRecord, PlayerData, PlayerRoundRecord};
use crate::retained_from_grbr_round;
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

/// Converts one locally recorded GRBR replay into a battle document.
///
/// # Errors
///
/// Returns an error when the replay is not a standard 1v1 of this version recorded
/// by this machine, when its rounds are not the contiguous sequence both sides
/// and the match share, or when it contains an object this build's catalogues
/// or this format cannot name.
pub fn battle_from_grbr(grbr: &[u8]) -> Result<Battle, String> {
    let record = record::read(grbr)?;
    readable(&record)?;
    let [blue, red] = <[record::PlayerRecord; 2]>::try_from(record.players.entries)
        .map_err(|players| format!("replay has {} sides, and a battle has two", players.len()))?;

    let match_rounds: Vec<i32> = record
        .match_rounds
        .entries
        .iter()
        .map(|entry| entry.round)
        .collect();
    let expected: Vec<i32> = (0..i32::try_from(match_rounds.len()).unwrap_or(i32::MAX)).collect();
    if match_rounds != expected {
        return Err(format!(
            "match rounds {match_rounds:?} are not the contiguous sequence from zero"
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
            "replay holds {} round, which is the opening alone, and a battle is deployment rounds",
            match_rounds.len()
        ));
    }

    let economy = Economy::embedded()?;
    // What a decline pays depends on the unit reinforcement pool the seed
    // selects. A match whose opening this build cannot deal still converts, as
    // long as it declines nothing.
    let pool = crate::opening::predict(&economy, record.info.system_seed, record.info.map_id)
        .map(|opening| opening.initialization.unit_round_pool)
        .ok();
    // The opening is round 0, and it has no state: every side enters it holding
    // nothing. Its one decision is read into each side's opening, and the turns
    // start at round 1.
    let mut turns = Vec::with_capacity(match_rounds.len() - 1);
    for (position, round) in match_rounds
        .iter()
        .copied()
        .enumerate()
        .skip(OPENING_ROUNDS)
    {
        let offers = record.match_rounds.entries[position]
            .reinforce_items
            .arrays
            .first()
            .map(|array| array.values.clone());
        let declined = pool
            .map(|pool| crate::reinforcement::decline_supply(&economy, pool, round))
            .transpose()?;
        turns.push(turn(
            grbr,
            &economy,
            [&blue, &red],
            position,
            round,
            offers,
            declined,
        )?);
    }

    // The four combinations each side was dealt are recorded nowhere, and are
    // not lost: they are drawn from the match's reinforcement stream, which the
    // opening round's snapshot carries. `crate::opening` rebuilds them.
    let dealt = opening_offers(&economy, &record.match_rounds.entries[0])?;

    check_concession(&turns)?;

    Ok(Battle {
        game_build: crate::economy::game_build().to_owned(),
        map_id: record.info.map_id,
        seed: record.info.system_seed,
        // A replay was played under the game's own clock, not under this
        // platform's rule for running out of one, so it states none.
        deploy_time: None,
        blue: battle_side(&economy, &blue, Seat::Blue, dealt.blue)?,
        red: battle_side(&economy, &red, Seat::Red, dealt.red)?,
        turns,
    })
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
    if record.seat < 0 {
        return Err(
            "replay was downloaded from the server, whose snapshots are reconstructions; \
             see replay/README.md"
                .into(),
        );
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
    offers: Option<Vec<i32>>,
    declined: Option<i32>,
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
            declined,
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
            reinforce_offers: offers,
            blue: blue_state,
            red: red_state,
        },
        actions,
    })
}

/// A replay converted into the document `replay convert` writes, and that
/// document read back the way `doc verify` reads it.
pub struct Converted {
    pub battle: Battle,
    pub yaml: String,
    pub stated: opening::Stated,
}

/// Converts a replay into its battle document, refusing one the document
/// would not faithfully carry.
///
/// Two things are checked here because only the replay can check them. The
/// written document has to read back as exactly the battle it was written
/// from, every state field and action operand included, or the corpus would
/// hold something other than what was converted. And the seeded random stream
/// has to land on every state the replay recorded: the match seed on the
/// opening round's, and each round's reinforcement deal on the states that
/// round and the next one recorded. Those states are not in the document, so
/// once it is written nothing else can compare them.
///
/// A deal the rules cannot reproduce at all is not refused here: `doc verify`
/// reports it against the document, and the conversion report carries it.
///
/// # Errors
///
/// Returns what [`battle_from_grbr`] refuses, a document that does not read
/// back as its battle, and a stream that misses a recorded state.
pub fn document(economy: &Economy, grbr: &[u8]) -> Result<Converted, String> {
    let battle = battle_from_grbr(grbr)?;
    let yaml = crate::battle::canonical_yaml(&battle)?;
    let stated = opening::stated(yaml.as_bytes())?
        .ok_or("the converted document does not read back as a battle")?;
    if stated.turns != battle.turns {
        return Err(
            "the converted document does not read back as the battle it was \
                    written from"
                .into(),
        );
    }
    stream_lands_on_the_recorded_states(economy, &record::read(grbr)?, &stated)?;
    Ok(Converted {
        battle,
        yaml,
        stated,
    })
}

/// Refuses a replay whose recorded random states the seeded stream misses.
///
/// The generator is Lua's and a state is 256 bits, so a near miss does not
/// land: equal states are the whole of the claim that the stream is modelled.
fn stream_lands_on_the_recorded_states(
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
    let seeded = opening::initialize(native.info.system_seed)?.stream.state();
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

/// Refuses a replay whose concession does not end it.
///
/// `PAD_GiveUp` is the one recorded action that overrides `IsExitMatchAction`,
/// and the override returns true unconditionally: it leaves the match. So a
/// battle holds at most one, as the last decision its side takes in the last
/// round.
///
/// # Errors
///
/// Returns an error when a side decides after conceding, a round follows a
/// concession, or more than one side concedes.
fn check_concession(turns: &[Turn]) -> Result<(), String> {
    let mut conceded = 0;
    for (at, turn) in turns.iter().enumerate() {
        for (side, actions) in [("blue", &turn.actions.blue), ("red", &turn.actions.red)] {
            let count = actions
                .iter()
                .filter(|action| matches!(action, Action::Concede))
                .count();
            if count == 0 {
                continue;
            }
            conceded += count;
            if actions.last() != Some(&Action::Concede) {
                return Err(format!(
                    "round {} {side} decides after conceding",
                    turn.round
                ));
            }
            if at + 1 != turns.len() {
                return Err(format!(
                    "round {} {side} concedes, and the replay continues",
                    turn.round
                ));
            }
        }
    }
    if conceded > 1 {
        return Err(format!(
            "replay records {conceded} concessions, and the first one ends the match"
        ));
    }
    Ok(())
}

fn battle_side(
    economy: &Economy,
    player: &record::PlayerRecord,
    seat: Seat,
    dealt: Vec<OpeningOffer>,
) -> Result<BattleSide, String> {
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
    Ok(BattleSide {
        opening: opening_taken(economy, player, seat, dealt)?,
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
    // here for the opening to restart. The actions say which were spent.
    let spent: Vec<i32> = position
        .checked_sub(1)
        .map(|previous| {
            net_actions(&player.rounds.entries[previous].actions.entries)
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
        .map(|skill| PanelSkill {
            index: skill.index,
            id: skill.id,
            cooldown: skill.cooling_round,
            used: spent.contains(&skill.index),
            release: None,
        })
        .collect();
    battle_skills.sort_by_key(|skill| skill.index);

    let retained = retained_from_grbr_round(grbr, u32::try_from(round).unwrap_or(0))?;
    let mut retained = match seat {
        Seat::Blue => retained.blue,
        Seat::Red => retained.red,
    };
    retained
        .airdrop_shields
        .sort_unstable_by_key(|position| (position.x, position.y));

    let mut blueprints = data.blueprints.values.clone();
    blueprints.sort_unstable();

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
        reactor_core: data.reactor_core,
        // The snapshot precedes the round's income, which the opening pays.
        supply: data.supply,
        // The allowances are the opening's to set.
        shop: ShopState {
            unlocked_units,
            buys_remaining: 0,
            unlocks_remaining: 0,
        },
        blueprints,
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
        airdrop_shields: retained.airdrop_shields,
        terrains: retained.terrains,
    };

    // The snapshot is taken before the round opens: before its resets, and
    // before its deliveries, which the game makes before either side decides
    // anything. Both belong to the position the round opens with, so the
    // opening is made here, and each delivery lands where the board puts it.
    let mut placement = crate::landing::placement(seat == Seat::Red);
    let stream = player_stream(economy, player, position, seat)?;
    crate::transition::open_round(economy, &snapshot, round, &mut placement, Some(stream))
        .map_err(|reason| format!("round {round} {} delivery: {reason:?}", seat.name()))
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
    let mut formations = Vec::with_capacity(data.units.entries.len());
    for unit in &data.units.entries {
        let (type_name, _) = unit_type_from_id(unit.id).ok_or_else(|| {
            format!(
                "unit ID {} has no layout type in build {}",
                unit.id,
                replay_version()
            )
        })?;
        formations.push(StateUnit {
            value: Some(unit.sell_supply),
            // Settled by the opening, which knows the round.
            movable: false,
            unit: UnitPlacement {
                type_name: type_name.to_owned(),
                index: unit.index,
                position: seat.position(&unit.position),
                // The record counts paid upgrades from zero; a layout displays the
                // level from one.
                level: Some(unit.level + 1).filter(|level| *level != 1),
                exp: Experience::of(unit.exp, type_name, unit.level + 1)?,
                rotated: Some(unit.rotated).filter(|rotated| *rotated),
                equipment: unit.equipments.entries.iter().map(|item| item.id).collect(),
                // No recorded field states it; see docs/spec/document/battle.md.
                travelling: None,
            },
        });
    }
    formations.sort_by_key(|entry| entry.unit.index);
    Ok(formations)
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
    let owed = position.checked_sub(1).is_some_and(|previous| {
        net_actions(&player.rounds.entries[previous].actions.entries)
            .iter()
            .any(|action| {
                action.kind == "PAD_ActiveEnergyTowerSkill"
                    && action.skill_id == Some(RAPID_SUPPLY_SKILL)
            })
    });
    let expected: &[i32] = if owed { &[RAPID_SUPPLY_SKILL] } else { &[] };
    let recorded = entry.data.energy_tower_skills.values.as_slice();
    if recorded == expected {
        return Ok(owed);
    }
    Err(format!(
        "{} round {} records energy tower skills {recorded:?}, and the round before it \
         decided {expected:?}; see docs/spec/document/battle.md",
        seat.name(),
        entry.round
    ))
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
    /// An entry that no longer stands for a decision but still absorbs an undo.
    const SPENT: bool = false;
    let mut taken: Vec<(&ActionRecord, bool)> = Vec::with_capacity(recorded.len());
    let mut undone: Vec<(&ActionRecord, bool)> = Vec::new();
    for action in recorded {
        match action.kind.as_str() {
            "PAD_Undo" => {
                if let Some(last) = taken.pop() {
                    undone.push(last);
                }
            }
            "PAD_Redo" => {
                if let Some(last) = undone.pop() {
                    taken.push(last);
                }
            }
            "PAD_CancelReleaseCommanderSkill" => {
                undone.clear();
                if let Some(entry) = taken.iter_mut().rev().find(|(candidate, stands)| {
                    *stands
                        && candidate.kind == "PAD_ReleaseCommanderSkill"
                        && candidate.skill_index == action.skill_index
                }) {
                    entry.1 = SPENT;
                }
                taken.push((action, SPENT));
            }
            "PAD_FinishDeploy" => undone.clear(),
            _ => {
                undone.clear();
                taken.push((action, true));
            }
        }
    }
    taken
        .into_iter()
        .filter_map(|(action, stands)| stands.then_some(action))
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
/// slot holds and the index a purchase creates. The moves are then collapsed
/// by [`collapse_moves`], and the collapsed round is stepped again: it has to
/// reach exactly the position the recorded one does, or the replay is refused.
fn actions(
    economy: &Economy,
    round: &PlayerRoundRecord,
    seat: Seat,
    opened: &SideState,
    declined: Option<i32>,
) -> Result<Vec<Action>, String> {
    let red = seat == Seat::Red;
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
    let mut taken = Vec::new();
    for (at, recorded) in recorded_actions(round, seat)?.into_iter().enumerate() {
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
        let placed = match &action {
            Action::BuyUnit { .. } => Placed::Created(position.next_index.unit),
            Action::MoveUnit { index, .. } => position
                .units
                .iter()
                .find(|entry| entry.unit.index == *index)
                .map_or(Placed::Elsewhere, |entry| {
                    Placed::Moved(Region::of(entry.unit.position))
                }),
            _ => Placed::Elsewhere,
        };
        position = step(&position, &action, at)?;
        taken.push((action, placed));
    }
    let collapsed = collapse_moves(taken);
    let mut replayed = opened.clone();
    for (at, action) in collapsed.iter().enumerate() {
        replayed = step(&replayed, action, at)?;
    }
    if replayed != position {
        return Err(format!(
            "{}'s collapsed moves end the round somewhere its recorded ones do not",
            seat.name()
        ));
    }
    Ok(collapsed)
}

/// What a stepped decision did to a formation's place on the board.
enum Placed {
    /// A purchase, and the index it handed out.
    Created(i32),
    /// A move, and the region the formation was in before it.
    Moved(Region),
    Elsewhere,
}

/// How a formation's moves so far collapse, and where the kept one is written.
#[derive(Clone, Copy)]
enum Run {
    /// A purchase this round: every later move is folded into it.
    Bought(usize),
    /// A formation that began the round's moves in the main half: only the
    /// last move is kept.
    FromMain(usize),
    /// A formation that began them on a flank: the last move within each
    /// stretch of moves ending in one region is kept.
    FromFlank(usize, Region),
}

/// Keeps what a formation's moves amount to, and nothing of how they got there.
///
/// A move settles `travelling` by the region it arrives in, and a round's
/// opening holds no travelling formation. So a formation that begins its moves
/// in the main half, including one bought or handed out this round, travels
/// exactly when its last move ends on a flank, whatever route it took: its
/// last move is all it needs. A purchase is what creates its formation, so the
/// purchase itself carries where the moves end, and step marks it travelling
/// when that is a flank. A formation that begins on a flank is different: going
/// to the main half and back travels, staying does not. For it, only a run of
/// moves ending in the same region collapses to its last.
fn collapse_moves(taken: Vec<(Action, Placed)>) -> Vec<Action> {
    let mut kept: Vec<Option<Action>> = Vec::with_capacity(taken.len());
    let mut runs: BTreeMap<i32, Run> = BTreeMap::new();
    for (action, placed) in taken {
        match (&action, placed) {
            (Action::BuyUnit { .. }, Placed::Created(index)) => {
                runs.insert(index, Run::Bought(kept.len()));
            }
            (
                Action::MoveUnit {
                    index,
                    position,
                    rotated,
                },
                Placed::Moved(from),
            ) => {
                let arrives = Region::of(*position);
                match runs.get(index).copied() {
                    Some(Run::Bought(at)) => {
                        if let Some(Action::BuyUnit {
                            position: bought,
                            rotated: turned,
                            ..
                        }) = &mut kept[at]
                        {
                            *bought = *position;
                            *turned = *rotated;
                        }
                        continue;
                    }
                    Some(Run::FromMain(at)) => {
                        kept[at] = None;
                        runs.insert(*index, Run::FromMain(kept.len()));
                    }
                    Some(Run::FromFlank(at, region)) if region == arrives => {
                        kept[at] = None;
                        runs.insert(*index, Run::FromFlank(kept.len(), arrives));
                    }
                    None if from == Region::Main => {
                        runs.insert(*index, Run::FromMain(kept.len()));
                    }
                    // A flank run that changes region, or a first move from a
                    // flank, starts a run of its own.
                    Some(Run::FromFlank(..)) | None => {
                        runs.insert(*index, Run::FromFlank(kept.len(), arrives));
                    }
                }
            }
            _ => {}
        }
        kept.push(Some(action));
    }
    kept.into_iter().flatten().collect()
}

fn recorded_actions(round: &PlayerRoundRecord, seat: Seat) -> Result<Vec<Recorded>, String> {
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
                // Declining is the same decision at the declined offer, and
                // the game records its `ID` as zero rather than omitting it.
                let offer = field("Index", action.index)?;
                Action::ChooseReinforceItem {
                    offer,
                    id: if offer == DECLINED_OFFER {
                        None
                    } else {
                        Some(field("ID", action.id)?)
                    },
                }
            }
            "PAD_BuyUnit" => Action::BuyUnit {
                unit: field("UID", action.unit_id)?,
                position: position(&action.buy_position)?,
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
    use crate::battle::Action;

    /// A formation that begins its moves in the main half keeps only its
    /// last, a purchase takes every move of its own formation, and one that
    /// begins on a flank keeps the last move of each region it passes through.
    #[test]
    fn a_formation_keeps_what_its_moves_amount_to() {
        use super::{Placed, collapse_moves};
        use crate::layout::Region;
        let moved = |index, x, y| Action::MoveUnit {
            index,
            position: Position { x, y },
            rotated: false,
        };
        let bought = |x, y| Action::BuyUnit {
            unit: 10,
            position: Position { x, y },
            rotated: false,
        };
        // From the main half: out to a flank and back, and only the last stays.
        assert_eq!(
            collapse_moves(vec![
                (moved(0, 0, -160), Placed::Moved(Region::Main)),
                (moved(1, 50, -160), Placed::Moved(Region::Main)),
                (moved(0, 330, 100), Placed::Moved(Region::Main)),
                (moved(0, 100, -100), Placed::Moved(Region::RightFlank)),
            ]),
            [moved(1, 50, -160), moved(0, 100, -100)]
        );
        // A purchase takes its formation's moves, a flank included.
        assert_eq!(
            collapse_moves(vec![
                (bought(0, -160), Placed::Created(5)),
                (moved(5, 100, -100), Placed::Moved(Region::Main)),
                (moved(5, -330, 100), Placed::Moved(Region::Main)),
            ]),
            [bought(-330, 100)]
        );
        // From a flank: the main half twice, then the flank again. Going out
        // and back travels where staying would not, so both regions keep one.
        assert_eq!(
            collapse_moves(vec![
                (moved(7, 0, -160), Placed::Moved(Region::LeftFlank)),
                (moved(7, 100, -100), Placed::Moved(Region::Main)),
                (moved(7, -330, 200), Placed::Moved(Region::Main)),
            ]),
            [moved(7, 100, -100), moved(7, -330, 200)]
        );
    }
}
