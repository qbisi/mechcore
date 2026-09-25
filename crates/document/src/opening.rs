//! How a match's seed decides the four openings each side is dealt.
//!
//! The opening is the one part of a battle that no action records and no
//! snapshot carries: a replay stores the combination taken and not the three
//! refused. It is not lost, though. The deal is drawn from the match's
//! reinforcement stream, which is seeded from `BattleInfo.SystemSeed`, so the
//! four combinations are a function of a number the document already holds.
//!
//! `docs/rules/opening.md` pins the deal parameters and their arithmetic to
//! the build. Initialization advances the reinforcement stream explicitly;
//! map constructions use a separate stream seeded with the same match seed.

use crate::battle::{Action, OpeningOffer, Turn, TurnActions};
use crate::economy::{Economy, OpeningKind};
use crate::layout::{Position, StaticPlacement};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// How many combinations a side is dealt.
///
/// The build's `AdvanceTeamSetting.chooseCount` is 4.
/// `CalculateChooseCount` caps it by the available teams and specialists per
/// player; neither cap binds for the standard 1v1 pools.
pub const CHOOSE_COUNT: usize = 4;

/// `RandAdvance`'s `diffUnitLimit` under this mode's match setting.
///
/// It is a floor rather than a cap: a candidate team is refused unless every
/// price tier it touches can still reach this many different units by the time
/// the deal is over. The build passes `MatchSetting.advanceSameUnitMaximum`
/// (3 for standard versus maps) into this parameter. Despite that field's
/// name, `RandAdvance` compares a minimum count of distinct units per tier.
pub const DIFFERENT_UNIT_FLOOR: i32 = 3;

/// The match's reinforcement random stream.
///
/// `GRRandom` delegates to `RanState`, which is Lua 5.4's generator: xoshiro256\*\*
/// for the values and Lua's own rejection loop for a range. Both are reproduced
/// here rather than approximated, because the replay records the generator's
/// 256-bit state and a near miss would not land on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stream {
    state: [u64; 4],
    draws: u32,
}

impl Stream {
    /// The stream as it stands, which a replay records once per round.
    ///
    /// # Errors
    ///
    /// Refuses the all-zero state, which cannot be reached by this generator.
    pub fn from_state(state: [u64; 4]) -> Result<Self, String> {
        if state == [0; 4] {
            return Err("opening random state must not be all zero".into());
        }
        Ok(Self { state, draws: 0 })
    }

    /// The stream a match seed starts, as `math_randomseed(seed, 0)` builds it.
    ///
    /// Lua fills the state with the seed, a constant, the second seed and zero,
    /// then throws away sixteen values to spread it. Every tracked match seed is
    /// positive, so which way a negative one widens is not covered.
    #[must_use]
    pub fn seeded(seed: i32) -> Self {
        let mut stream = Self {
            state: [i64::from(seed).cast_unsigned(), 0xff, 0, 0],
            draws: 0,
        };
        for _ in 0..16 {
            stream.value();
        }
        stream.draws = 0;
        stream
    }

    /// Where the stream stands, for a caller comparing against a recorded one.
    #[must_use]
    pub const fn state(&self) -> [u64; 4] {
        self.state
    }

    pub(crate) const fn draws(&self) -> u32 {
        self.draws
    }

    /// Steps the stream without reading it.
    pub fn skip(&mut self, values: u32) {
        for _ in 0..values {
            self.value();
        }
    }

    /// `nextrand`: one xoshiro256\*\* value.
    fn value(&mut self) -> u64 {
        self.draws += 1;
        let [zero, one, two, three] = self.state;
        let two = two ^ zero;
        let three = three ^ one;
        let result = one.wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        self.state = [
            zero ^ three,
            one ^ two,
            two ^ (one << 17),
            three.rotate_left(45),
        ];
        result
    }

    /// `math_random(low, up)`: uniform on the inclusive range.
    ///
    /// The mask is folded five times rather than six, so this build's copy is
    /// the 32-bit one. Nothing here draws a range that would notice.
    fn range(&mut self, low: u64, up: u64) -> u64 {
        let mut value = self.value();
        let span = up.wrapping_sub(low);
        if (span + 1) & span == 0 {
            return low.wrapping_add(value & span);
        }
        let mut limit = span;
        for shift in [1, 2, 4, 8, 16] {
            limit |= limit >> shift;
        }
        value &= limit;
        while value > span {
            value = self.value() & limit;
        }
        low.wrapping_add(value)
    }

    /// `ReinforcePool.ServerRand(bottom, top)`: uniform on `bottom..top`.
    pub(crate) fn pick(&mut self, bottom: usize, top: usize) -> usize {
        let low = bottom as u64 + 1;
        let up = top as u64;
        usize::try_from(self.range(low, up)).unwrap_or(bottom) - 1
    }
}

/// The four combinations each side was dealt.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Deal {
    pub blue: Vec<OpeningOffer>,
    pub red: Vec<OpeningOffer>,
}

/// The two pools an opening is dealt from, each sorted as `RandAdvance` sorts.
struct Pools {
    teams: Vec<i32>,
    specialists: Vec<i32>,
    /// Each team's units grouped by what one costs, which is the tier the
    /// different-unit floor counts within.
    tiers: BTreeMap<i32, BTreeMap<i32, BTreeSet<i32>>>,
}

impl Pools {
    fn read(economy: &Economy) -> Result<Self, String> {
        let mut teams = Vec::new();
        let mut specialists = Vec::new();
        let mut tiers = BTreeMap::new();
        for (id, team) in economy.advance_teams() {
            match team.kind {
                OpeningKind::Officer => specialists.push(id),
                OpeningKind::Units => {
                    let mut grouped: BTreeMap<i32, BTreeSet<i32>> = BTreeMap::new();
                    for unit in &team.units {
                        let price = economy.unit(*unit).ok_or_else(|| {
                            format!("advance team {id} fields unit {unit}, which has no price")
                        })?;
                        grouped.entry(price.supply).or_default().insert(*unit);
                    }
                    teams.push(id);
                    tiers.insert(id, grouped);
                }
            }
        }
        teams.sort_unstable();
        specialists.sort_unstable();
        Ok(Self {
            teams,
            specialists,
            tiers,
        })
    }
}

/// The deal the stream is standing at the start of.
///
/// Blue is dealt first and red draws from what is left, which is why no
/// specialist is offered to both sides. The stream is left where the deal ended,
/// so a caller can compare it against the state the next round recorded.
///
/// # Errors
///
/// Returns an error when a pool runs dry or a team fields a unit with no price.
pub fn deal(economy: &Economy, stream: &mut Stream) -> Result<Deal, String> {
    deal_from_pools(&Pools::read(economy)?, stream)
}

fn deal_from_pools(pools: &Pools, stream: &mut Stream) -> Result<Deal, String> {
    let mut teams = pools.teams.clone();
    let mut specialists = pools.specialists.clone();
    let mut sides = Vec::with_capacity(2);
    for _ in 0..2 {
        let offers = rand_advance(pools, stream, &mut teams, &mut specialists)?;
        sides.push(offers);
    }
    let [blue, red]: [Vec<OpeningOffer>; 2] = sides
        .try_into()
        .map_err(|_| "an opening deals two sides".to_string())?;
    Ok(Deal { blue, red })
}

/// One `ReinforcePool.RandAdvance` call: the combinations one side is dealt.
///
/// Both pools are sorted on entry and drawn by a partial Fisher-Yates, so the
/// `i`th pick is the `i`th offer. The team half is refused and redrawn until it
/// passes the different-unit floor, and a refused team leaves the window for the
/// rest of the call rather than only for this pick.
fn rand_advance(
    pools: &Pools,
    stream: &mut Stream,
    teams: &mut Vec<i32>,
    specialists: &mut Vec<i32>,
) -> Result<Vec<OpeningOffer>, String> {
    teams.sort_unstable();
    specialists.sort_unstable();
    let mut held: BTreeMap<i32, BTreeSet<i32>> = BTreeMap::new();
    let mut window = teams.len();
    let mut offers = Vec::with_capacity(CHOOSE_COUNT);
    for taken in 0..CHOOSE_COUNT {
        let mut accepted = None;
        while accepted.is_none() {
            if window <= taken {
                return Err(format!(
                    "the advance team pool ran out with {taken} of {CHOOSE_COUNT} dealt"
                ));
            }
            let at = stream.pick(taken, window);
            let candidate = *teams
                .get(at)
                .ok_or_else(|| format!("advance team pool has no entry {at}"))?;
            if passes(pools, &held, candidate, taken)? {
                teams.swap(taken, at);
                accepted = Some(candidate);
            } else {
                window -= 1;
                teams.swap(at, window);
            }
        }
        let team = accepted.ok_or_else(|| "no advance team was accepted".to_string())?;
        for (tier, units) in tiers_of(pools, team)? {
            held.entry(*tier).or_default().extend(units);
        }
        if specialists.len() <= taken {
            return Err(format!(
                "the specialist pool ran out with {taken} of {CHOOSE_COUNT} dealt"
            ));
        }
        let at = stream.pick(taken, specialists.len());
        specialists.swap(taken, at);
        offers.push(OpeningOffer {
            team,
            specialist: specialists[taken],
        });
    }
    teams.retain(|team| !offers.iter().any(|offer| offer.team == *team));
    specialists.retain(|officer| !offers.iter().any(|offer| offer.specialist == *officer));
    Ok(offers)
}

fn tiers_of(pools: &Pools, team: i32) -> Result<&BTreeMap<i32, BTreeSet<i32>>, String> {
    pools
        .tiers
        .get(&team)
        .ok_or_else(|| format!("advance team {team} is not in the units pool"))
}

/// Whether a candidate team can still be dealt.
///
/// For every price tier the candidate touches, the units already held there,
/// the ones it would add, and one for each pick still to come have to reach the
/// floor. A tier that cannot get there sends the candidate back.
fn passes(
    pools: &Pools,
    held: &BTreeMap<i32, BTreeSet<i32>>,
    candidate: i32,
    taken: usize,
) -> Result<bool, String> {
    // The native instruction is `not eax`, not arithmetic negation:
    // num + !taken is num - taken - 1, excluding the current pick.
    let remaining = i32::try_from(CHOOSE_COUNT - taken - 1).unwrap_or(0);
    for (tier, units) in tiers_of(pools, candidate)? {
        let known = held.get(tier);
        let standing = known.map_or(0, BTreeSet::len);
        let fresh = units
            .iter()
            .filter(|unit| !known.is_some_and(|known| known.contains(*unit)))
            .count();
        let reach = i32::try_from(standing + fresh).unwrap_or(i32::MAX) + remaining;
        if reach < DIFFERENT_UNIT_FLOOR {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The shipped inputs to initialization, extracted by `scripts/extract_opening.py`.
#[derive(Deserialize)]
struct Setup {
    officer_groups: BTreeMap<i32, Vec<i32>>,
    unit_round_pools: Vec<i32>,
    maps: BTreeMap<i32, MapSetup>,
    constructions: BTreeMap<i32, Vec<Construction>>,
}

#[derive(Deserialize)]
struct MapSetup {
    groups: Vec<i32>,
    centers: [Position; 2],
    reactor_cores: Vec<i32>,
}

#[derive(Deserialize)]
struct Construction {
    unit: i32,
    position: Position,
}

impl Setup {
    fn embedded() -> Result<Self, String> {
        serde_yaml::from_str(include_str!("../../../config/opening.yaml"))
            .map_err(|error| format!("cannot read opening initialization: {error}"))
    }

    fn initialize(&self, seed: i32) -> Initialization {
        let mut stream = Stream::seeded(seed);
        let mut officers = BTreeMap::new();
        // RandomTypeGroup sorts positive type IDs, then each group's members.
        // A singleton is retained without calling ServerRand.
        for (&kind, members) in &self.officer_groups {
            let at = if members.len() == 1 {
                0
            } else {
                stream.pick(0, members.len())
            };
            officers.insert(kind, members[at]);
        }
        let officer_draws = stream.draws;
        let unit_round_pool = self.unit_round_pools[stream.pick(0, self.unit_round_pools.len())];
        Initialization {
            stream,
            officers,
            officer_draws,
            unit_round_pool,
        }
    }
}

/// The reinforcement stream after pool setup and before the opening deal.
/// Raw draw counts exclude seeding's sixteen warm-up values and include retries.
#[derive(Debug, Serialize)]
pub struct Initialization {
    #[serde(skip)]
    pub stream: Stream,
    pub officers: BTreeMap<i32, i32>,
    pub officer_draws: u32,
    pub unit_round_pool: i32,
}

/// Advances the stream through officer selection and unit reinforcement setup.
///
/// # Errors
/// Returns an error if the embedded initialization inputs cannot be read.
pub fn initialize(seed: i32) -> Result<Initialization, String> {
    Ok(Setup::embedded()?.initialize(seed))
}

/// Both initial construction lists in the battle document's side-local frame.
#[derive(Debug, Serialize)]
pub struct Constructions {
    pub blue: Vec<StaticPlacement>,
    pub red: Vec<StaticPlacement>,
}

/// A seed-only prediction plus the exact stream boundaries for auditing it.
#[derive(Debug, Serialize)]
pub struct Prediction {
    pub map_id: i32,
    pub seed: i32,
    pub initialization: Initialization,
    pub opening_offset: u32,
    pub opening_state: [u64; 4],
    pub after_opening_offset: u32,
    pub after_opening_state: [u64; 4],
    pub deal: Deal,
    pub construction_group: i32,
    pub reversed: [bool; 2],
    pub construction_draws: u32,
    pub constructions: Constructions,
}

/// The maps this build has an opening initialization for, in ID order.
///
/// A match dealt on any of them is one [`predict`] can deal, so this is what a
/// caller that was given no map draws from.
///
/// # Errors
/// Returns an error when the embedded initialization cannot be read.
pub fn maps() -> Result<Vec<i32>, String> {
    Ok(Setup::embedded()?.maps.into_keys().collect())
}

/// The reactor core a seat starts a match on `map_id` with, before its
/// opening moves it.
///
/// This is `MatchSetting.GetReactorCore`, which reads the map's
/// `reactorCores` by seat and gives every seat the first entry when the list
/// holds fewer than two. `seat` is 0 for blue and 1 for red.
///
/// # Errors
/// Refuses a map without a supported opening, or a seat the list cannot name.
pub fn reactor_core(map_id: i32, seat: usize) -> Result<i32, String> {
    let setup = Setup::embedded()?;
    let cores = &setup
        .maps
        .get(&map_id)
        .ok_or_else(|| format!("map {map_id} has no supported opening initialization"))?
        .reactor_cores;
    let at = if cores.len() < 2 { 0 } else { seat };
    cores
        .get(at)
        .copied()
        .ok_or_else(|| format!("map {map_id} states no reactor core for seat {seat}"))
}

/// Predicts standard 1v1 offers and initial defensive constructions from a seed.
/// Player choices are inputs to play, so this predicts their options only.
///
/// # Errors
/// Refuses unsupported maps or negative seeds, unreadable configuration, and
/// pools that cannot deal a complete opening.
pub fn predict(economy: &Economy, seed: i32, map_id: i32) -> Result<Prediction, String> {
    if seed < 0 {
        return Err("negative match seeds are outside the verified opening scope".into());
    }
    let setup = Setup::embedded()?;
    let map = setup
        .maps
        .get(&map_id)
        .ok_or_else(|| format!("map {map_id} has no supported opening initialization"))?;
    let initialization = setup.initialize(seed);
    let mut stream = initialization.stream;
    let opening_offset = stream.draws;
    let opening_state = stream.state();
    let deal = deal(economy, &mut stream)?;
    // MapSystem copies GRRandom.seed; it does not draw from Match.random.
    let mut map_stream = Stream::seeded(seed);
    let construction_group = map.groups[map_stream.pick(0, map.groups.len())];
    // GRRandom.NextBool is Next(1000) < 500, including range rejection.
    let reversed = [
        map_stream.pick(0, 1000) < 500,
        map_stream.pick(0, 1000) < 500,
    ];
    let group = setup
        .constructions
        .get(&construction_group)
        .ok_or_else(|| format!("construction group {construction_group} is missing"))?;
    let build_side = |side: usize| -> Result<Vec<StaticPlacement>, String> {
        group
            .iter()
            .enumerate()
            .map(|(index, unit)| {
                let (type_name, _) = crate::catalog::construction_type_from_id(unit.unit)
                    .ok_or_else(|| format!("unknown construction {}", unit.unit))?;
                Ok(StaticPlacement {
                    type_name: type_name.into(),
                    index: i32::try_from(index).map_err(|error| error.to_string())?,
                    position: Position {
                        x: map.centers[side].x
                            + if reversed[side] {
                                -unit.position.x
                            } else {
                                unit.position.x
                            },
                        y: map.centers[side].y + unit.position.y,
                    },
                })
            })
            .collect()
    };
    Ok(Prediction {
        map_id,
        seed,
        initialization,
        opening_offset,
        opening_state,
        after_opening_offset: stream.draws,
        after_opening_state: stream.state(),
        deal,
        construction_group,
        reversed,
        construction_draws: map_stream.draws,
        constructions: Constructions {
            blue: build_side(0)?,
            red: build_side(1)?,
        },
    })
}

/// What a battle document says about its two openings and its rounds.
///
/// States and decisions use the complete document types. A seed check may
/// read only part of them, but malformed payloads must never pass unnoticed.
#[derive(Debug)]
pub struct Stated {
    pub map_id: i32,
    pub seed: i32,
    /// The header's deployment clock, which only a match states.
    pub deploy_time: Option<i32>,
    pub blue: StatedSide,
    pub red: StatedSide,
    pub turns: Vec<Turn>,
    /// Round zero's decisions, each side's opening, as written.
    pub opening: TurnActions,
    /// Whether the last round's decisions are written with no position after
    /// them, which a converted battle does because no replay records the last
    /// fight's result.
    pub ends_on_actions: bool,
}

#[derive(Debug, Deserialize)]
pub struct StatedSide {
    /// The opening this side took, absent while it has not taken one.
    #[serde(skip)]
    pub opening: Option<StatedOpening>,
    pub offers: Vec<OpeningOffer>,
    pub constructions: Vec<StaticPlacement>,
    #[serde(with = "crate::names::loadout")]
    pub tech_loadout: BTreeMap<i32, Vec<i32>>,
}

/// The opening decision one side states in round zero.
#[derive(Debug, Default)]
pub struct StatedOpening {
    /// The zero-based `offer` the decision names.
    pub choose: i32,
    /// The team and specialist the decision says that offer holds.
    pub taken: OpeningOffer,
}

impl StatedSide {
    /// Checks the complete deal after verification validates its choice.
    fn agrees(&self, dealt: &[OpeningOffer]) -> bool {
        self.offers == dealt
    }
}

#[derive(Deserialize)]
struct StatedHeader {
    /// A battle that states no build is this binary's, as a layout is.
    #[serde(default = "crate::economy::this_build")]
    game_build: String,
    map_id: i32,
    seed: i32,
    #[serde(default)]
    deploy_time: Option<i32>,
    blue: StatedSide,
    red: StatedSide,
}

impl StatedOpening {
    /// Reads one side's round-zero decisions, which are its opening or, while
    /// the side has not taken one, nothing.
    fn opening(choices: &[Action], side: &str) -> Result<Option<StatedOpening>, String> {
        let [choice] = choices else {
            if choices.is_empty() {
                return Ok(None);
            }
            return Err(format!(
                "{side} takes {} decisions in round 0, and the opening is one",
                choices.len()
            ));
        };
        let Action::ChooseAdvanceTeam {
            offer,
            id,
            specialist,
        } = choice
        else {
            return Err(format!("{side} opening is not choose_advance_team"));
        };
        Ok(Some(Self {
            choose: *offer,
            taken: OpeningOffer {
                team: *id,
                specialist: *specialist,
            },
        }))
    }
}

/// Reads the openings and rounds out of a battle stream, or nothing when the
/// file is not one.
///
/// # Errors
///
/// Returns an error when the document names itself a battle and then breaks
/// the stream's grammar, or has a malformed state field or action operand.
pub fn stated(bytes: &[u8]) -> Result<Option<Stated>, String> {
    let Some(stream) = crate::battle::segments(bytes)? else {
        return Ok(None);
    };
    let header: StatedHeader = serde_yaml::from_value(stream.header)
        .map_err(|error| format!("battle header is not readable: {error}"))?;
    crate::economy::require_this_build(&header.game_build)?;
    let opening = stream
        .opening
        .ok_or("battle states no round 0 opening decisions")?;
    let opening: TurnActions = crate::battle::payload(opening)
        .map_err(|error| format!("round 0 action segment is not readable: {error}"))?;
    let mut blue = header.blue;
    let mut red = header.red;
    blue.opening = StatedOpening::opening(&opening.blue, "blue")?;
    red.opening = StatedOpening::opening(&opening.red, "red")?;
    let ends_on_actions = stream
        .rounds
        .last()
        .is_some_and(|round| round.actions.is_some());
    let turns = stream
        .rounds
        .into_iter()
        .map(|round| {
            let state = crate::battle::payload(round.state)
                .map_err(|error| format!("round {} state is not readable: {error}", round.round))?;
            let actions = match round.actions {
                Some(actions) => crate::battle::payload(actions).map_err(|error| {
                    format!("round {} actions are not readable: {error}", round.round)
                })?,
                None => TurnActions::default(),
            };
            Ok(Turn {
                round: round.round,
                state,
                actions,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(Some(Stated {
        map_id: header.map_id,
        seed: header.seed,
        deploy_time: header.deploy_time,
        blue,
        red,
        turns,
        opening,
        ends_on_actions,
    }))
}

/// Checks both offer arrays and construction lists against their seed and map.
/// The chosen index must be in range; a seed cannot determine a player's choice.
///
/// # Errors
/// Returns an error for a mismatched deal, layout, invalid choice or unsupported
/// initialization scope.
pub fn verify(economy: &Economy, stated: &Stated) -> Result<Prediction, String> {
    for (name, side) in [("blue", &stated.blue), ("red", &stated.red)] {
        if side.offers.len() != CHOOSE_COUNT {
            return Err(format!("{name} opening requires {CHOOSE_COUNT} offers"));
        }
        // A match in progress may have one opening and not the other, and a
        // side that has taken none has nothing here to disagree with. What
        // the seed deals it is checked below all the same.
        let Some(opening) = &side.opening else {
            continue;
        };
        let Some(offered) = usize::try_from(opening.choose)
            .ok()
            .filter(|at| *at < CHOOSE_COUNT)
            .map(|at| side.offers[at])
        else {
            return Err(format!(
                "{name} opening offer {} is outside 0..{CHOOSE_COUNT}",
                opening.choose
            ));
        };
        if opening.taken != offered {
            return Err(format!(
                "{name} opening offer {} holds team {} and specialist {}, \
                 and the decision names {:?}",
                opening.choose, offered.team, offered.specialist, opening.taken
            ));
        }
    }
    let found = predict(economy, stated.seed, stated.map_id)?;
    for (name, side, offers, constructions) in [
        (
            "blue",
            &stated.blue,
            &found.deal.blue,
            &found.constructions.blue,
        ),
        (
            "red",
            &stated.red,
            &found.deal.red,
            &found.constructions.red,
        ),
    ] {
        if !side.agrees(offers) {
            return Err(format!(
                "seed {} does not deal the stated {name} opening",
                stated.seed
            ));
        }
        if side.constructions != *constructions {
            return Err(format!(
                "seed {} does not produce the stated {name} constructions on map {}",
                stated.seed, stated.map_id
            ));
        }
    }
    Ok(found)
}

#[cfg(all(test, feature = "convert"))]
mod tests {
    use super::{Stream, deal, stated};
    use crate::economy::Economy;

    /// Nothing but a battle takes this path.
    #[test]
    fn a_layout_is_not_a_battle() {
        let layout = std::fs::read("../../layouts/tuff-replay-round-7.yaml").unwrap();
        assert!(stated(&layout).unwrap().is_none());
        assert!(stated(b"not a document at all").unwrap().is_none());
    }

    #[test]
    fn an_all_zero_state_is_refused() {
        assert!(Stream::from_state([0; 4]).is_err());
        let seeded = Stream::seeded(1);
        assert_eq!(Stream::from_state(seeded.state()).unwrap(), seeded);
    }

    /// The stream is the one thing here with no tolerance at all.
    #[test]
    fn the_stream_is_lua_54_seeded_the_way_lua_seeds_it() {
        let mut stream = Stream::seeded(1);
        let first = stream.state();
        stream.skip(1);
        assert_ne!(first, stream.state());
        let mut again = Stream::seeded(1);
        again.skip(1);
        assert_eq!(stream.state(), again.state());
        assert_ne!(Stream::seeded(1).state(), Stream::seeded(2).state());
    }

    /// Dealing twice from one position gives one answer.
    #[test]
    fn a_deal_is_a_function_of_the_position_it_starts_from() {
        let economy = Economy::embedded().unwrap();
        let mut one = Stream::seeded(31_103_914);
        one.skip(29);
        let mut two = Stream::seeded(31_103_914);
        two.skip(29);
        assert_eq!(
            deal(&economy, &mut one).unwrap(),
            deal(&economy, &mut two).unwrap()
        );
        assert_eq!(one.state(), two.state());
    }
}
