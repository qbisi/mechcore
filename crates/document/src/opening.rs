//! How a match's seed decides the four openings each side is dealt.
//!
//! The opening is the one part of a battle that no action records and no
//! snapshot carries: a replay stores the combination taken and not the three
//! refused. It is not lost, though. The deal is drawn from the match's
//! reinforcement stream, which is seeded from `BattleInfo.SystemSeed`, so the
//! four combinations are a function of a number the document already holds.
//!
//! `docs/rules/opening.md` pins the deal parameters and their arithmetic to
//! build 2259. Verification searches a bounded stream window; it does not
//! compute how many values the reinforcement pool consumes before the deal.

use crate::battle::OpeningOffer;
use crate::economy::{Economy, OpeningKind};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// How many combinations a side is dealt.
///
/// Build 2259's `AdvanceTeamSetting.chooseCount` is 4 (`level0`, path ID 146).
/// `CalculateChooseCount` caps it by the available teams and specialists per
/// player; neither cap binds for the standard 1v1 pools.
pub const CHOOSE_COUNT: usize = 4;

/// `RandAdvance`'s `diffUnitLimit` under this mode's match setting.
///
/// It is a floor rather than a cap: a candidate team is refused unless every
/// price tier it touches can still reach this many different units by the time
/// the deal is over. Build 2259 passes `MatchSetting.advanceSameUnitMaximum`
/// (3 for standard versus maps) into this parameter. Despite that field's
/// name, `RandAdvance` compares a minimum count of distinct units per tier.
pub const DIFFERENT_UNIT_FLOOR: i32 = 3;

/// How far past the seed the deal can start.
///
/// The reinforcement pool draws from the same stream before the opening does,
/// and how much it draws is not yet settled, so a check from the seed alone
/// searches this many positions for the one that produces the deal. Every
/// tracked replay starts between 24 and 33.
///
/// A deal agrees at a short contiguous run of positions rather than at one,
/// because a leading value its own rejection loop discards moves the start
/// without moving the deal. The run is one or two positions wide across the
/// tracked set, and the position the replay recorded is inside it every time.
pub const SEARCH_WINDOW: u32 = 96;

/// The match's reinforcement random stream.
///
/// `GRRandom` delegates to `RanState`, which is Lua 5.4's generator: xoshiro256\*\*
/// for the values and Lua's own rejection loop for a range. Both are reproduced
/// here rather than approximated, because the replay records the generator's
/// 256-bit state and a near miss would not land on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stream {
    state: [u64; 4],
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
        Ok(Self { state })
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
        };
        for _ in 0..16 {
            stream.value();
        }
        stream
    }

    /// Where the stream stands, for a caller comparing against a recorded one.
    #[must_use]
    pub const fn state(&self) -> [u64; 4] {
        self.state
    }

    /// Steps the stream without reading it.
    pub fn skip(&mut self, values: u32) {
        for _ in 0..values {
            self.value();
        }
    }

    /// `nextrand`: one xoshiro256\*\* value.
    fn value(&mut self) -> u64 {
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
    fn pick(&mut self, bottom: usize, top: usize) -> usize {
        let low = bottom as u64 + 1;
        let up = top as u64;
        usize::try_from(self.range(low, up)).unwrap_or(bottom) - 1
    }
}

/// The four combinations each side was dealt.
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// Where in the seed's own stream a deal starts, when it starts at all.
///
/// The pool draws before the opening does and this build has not been read far
/// enough to say how much, so the position is searched rather than computed.
/// `accepts` is the caller's test: what a document states about the deal.
///
/// # Errors
///
/// Returns an error when the pools cannot be read.
pub fn locate(
    economy: &Economy,
    seed: i32,
    accepts: &dyn Fn(&Deal) -> bool,
) -> Result<Vec<u32>, String> {
    let pools = Pools::read(economy)?;
    let mut found = Vec::new();
    for offset in 0..SEARCH_WINDOW {
        let mut stream = Stream::seeded(seed);
        stream.skip(offset);
        if let Ok(produced) = deal_from_pools(&pools, &mut stream)
            && accepts(&produced)
        {
            found.push(offset);
        }
    }
    Ok(found)
}

/// What a battle document says about its two openings.
///
/// Only the fields a seed check reads are declared, so the rest of the document
/// is skipped rather than parsed. That keeps the check independent of whether
/// the other four kinds can be read back yet.
#[derive(Debug, Deserialize)]
pub struct Stated {
    pub seed: i32,
    pub sides: StatedSides,
}

#[derive(Debug, Deserialize)]
pub struct StatedSides {
    pub blue: StatedSide,
    pub red: StatedSide,
}

#[derive(Debug, Deserialize)]
pub struct StatedSide {
    pub opening: StatedOpening,
}

/// The opening one side states: what it took, and what it chose between.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatedOpening {
    pub choose: i32,
    pub offers: Vec<OpeningOffer>,
}

impl StatedOpening {
    /// Checks the complete deal after verification validates its choice.
    fn agrees(&self, dealt: &[OpeningOffer]) -> bool {
        self.offers == dealt
    }
}

/// Reads the openings out of a battle document, or nothing when it is not one.
///
/// # Errors
///
/// Returns an error when the document names itself a battle and then does not
/// carry the two openings.
pub fn stated(bytes: &[u8]) -> Result<Option<Stated>, String> {
    #[derive(Deserialize)]
    struct Header {
        kind: crate::DocumentKind,
    }
    let header: Option<Header> = serde_yaml::from_slice(bytes).ok();
    if !header.is_some_and(|header| header.kind == crate::DocumentKind::Battle) {
        return Ok(None);
    }
    serde_yaml::from_slice(bytes)
        .map(Some)
        .map_err(|error| format!("battle document has no readable opening: {error}"))
}

/// What checking a battle's openings against its seed found.
#[derive(Debug)]
pub struct Verified {
    /// The first position of the run that agrees.
    pub offset: u32,
    /// Every position that agrees, which is one short run, see
    /// [`SEARCH_WINDOW`].
    pub matches: Vec<u32>,
    pub deal: Deal,
}

/// Checks that a battle's two openings are the ones its seed deals.
///
/// # Errors
///
/// Returns an error when no position in the searched window deals them, which
/// is the failure this check exists to find.
pub fn verify(economy: &Economy, stated: &Stated) -> Result<Verified, String> {
    for (side, opening) in [
        ("blue", &stated.sides.blue.opening),
        ("red", &stated.sides.red.opening),
    ] {
        if opening.offers.len() != CHOOSE_COUNT {
            return Err(format!("{side} opening requires {CHOOSE_COUNT} offers"));
        }
        if !usize::try_from(opening.choose).is_ok_and(|at| at < CHOOSE_COUNT) {
            return Err(format!(
                "{side} opening choose {} is outside 0..{CHOOSE_COUNT}",
                opening.choose
            ));
        }
    }
    let matches = locate(economy, stated.seed, &|deal| {
        stated.sides.blue.opening.agrees(&deal.blue) && stated.sides.red.opening.agrees(&deal.red)
    })?;
    let Some(offset) = matches.first().copied() else {
        return Err(format!(
            "seed {} does not deal both stated openings together anywhere in its first {SEARCH_WINDOW} positions",
            stated.seed
        ));
    };
    let mut stream = Stream::seeded(stated.seed);
    stream.skip(offset);
    let deal = deal(economy, &mut stream)?;
    Ok(Verified {
        offset,
        matches,
        deal,
    })
}

#[cfg(all(test, feature = "convert"))]
mod tests {
    use super::{CHOOSE_COUNT, Deal, SEARCH_WINDOW, Stream, deal, locate, stated, verify};
    use crate::convert::battle_from_grbr;
    use crate::economy::Economy;

    const TUFF: &str = "../../tests/grbr/2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].grbr";

    fn battles() -> Vec<crate::battle::Battle> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir("../../tests/grbr").expect("tracked replay directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            if let Ok(battle) = battle_from_grbr(&std::fs::read(&path).unwrap()) {
                out.push(battle);
            }
        }
        out
    }

    /// The generator is Lua's, and a near miss would not land on a recorded
    /// 256-bit state, so this is the whole of that claim.
    #[test]
    fn the_seed_reaches_the_state_the_opening_round_recorded() {
        let mut reached = 0;
        for entry in std::fs::read_dir("../../tests/grbr").expect("tracked replay directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let Ok(record) = crate::record::read(&std::fs::read(&path).unwrap()) else {
                continue;
            };
            let Ok(recorded) = <[u64; 4]>::try_from(
                record.match_rounds.entries[0]
                    .random_state
                    .states
                    .values
                    .as_slice(),
            ) else {
                continue;
            };
            let mut stream = Stream::seeded(record.info.system_seed);
            let mut found = None;
            for offset in 0..SEARCH_WINDOW {
                if stream.state() == recorded {
                    found = Some(offset);
                    break;
                }
                stream.skip(1);
            }
            assert!(
                found.is_some(),
                "{}: seed {} never reaches the state round 0 recorded",
                path.display(),
                record.info.system_seed
            );
            reached += 1;
        }
        assert_eq!(reached, 41);
    }

    /// Every converted battle's deal is the one its own seed produces.
    #[test]
    fn every_tracked_seed_deals_the_opening_its_battle_states() {
        let economy = Economy::embedded().unwrap();
        let (mut checked, mut runs) = (0, Vec::new());
        for battle in battles() {
            let yaml = crate::battle::canonical_yaml(&battle).unwrap();
            let stated = stated(yaml.as_bytes()).unwrap().expect("a battle document");
            let found = verify(&economy, &stated)
                .unwrap_or_else(|error| panic!("seed {}: {error}", battle.seed));
            assert_eq!(found.deal.blue, battle.sides.blue.opening.offers);
            assert_eq!(found.deal.red, battle.sides.red.opening.offers);
            runs.push(found.matches.len());
            checked += 1;
        }
        assert_eq!(checked, 41);
        // A leading value the deal's own rejection loop discards moves the
        // start without moving the deal, so a run of two is not ambiguity
        // about which deal the seed makes.
        runs.sort_unstable();
        assert_eq!((runs.first(), runs.last()), (Some(&1), Some(&2)));
    }

    /// A deal states eight combinations and no specialist twice, because the
    /// two sides draw from one pool that empties as they do.
    #[test]
    fn the_two_sides_draw_from_one_pool() {
        for battle in battles() {
            let blue = &battle.sides.blue.opening.offers;
            let red = &battle.sides.red.opening.offers;
            for held in [blue, red] {
                assert_eq!(held.len(), CHOOSE_COUNT);
            }
            let mut specialists: Vec<i32> = blue
                .iter()
                .chain(red)
                .map(|offer| offer.specialist)
                .collect();
            let mut teams: Vec<i32> = blue.iter().chain(red).map(|offer| offer.team).collect();
            specialists.sort_unstable();
            teams.sort_unstable();
            let held = specialists.len();
            specialists.dedup();
            teams.dedup();
            assert_eq!((specialists.len(), teams.len()), (held, held));
        }
    }

    /// A document whose opening was edited is refused, which is what makes the
    /// field evidence rather than decoration.
    #[test]
    fn an_edited_opening_is_not_dealt_by_its_seed() {
        let economy = Economy::embedded().unwrap();
        let battle = battle_from_grbr(&std::fs::read(TUFF).unwrap()).unwrap();
        let yaml = crate::battle::canonical_yaml(&battle).unwrap();
        let dealt = &battle.sides.blue.opening.offers;
        let swapped = format!("team: {}", dealt[0].team);
        let edited = yaml.replacen(&swapped, &format!("team: {}", dealt[0].team + 1), 1);
        assert_ne!(edited, yaml);
        let stated = stated(edited.as_bytes())
            .unwrap()
            .expect("a battle document");
        assert!(verify(&economy, &stated).is_err());
    }

    /// Without offers the choice no longer identifies a team or specialist.
    #[test]
    fn an_opening_requires_its_alternatives() {
        let battle = battle_from_grbr(&std::fs::read(TUFF).unwrap()).unwrap();
        let mut yaml = serde_yaml::to_value(&battle).unwrap();
        yaml["sides"]["blue"]["opening"]
            .as_mapping_mut()
            .unwrap()
            .remove(serde_yaml::Value::from("offers"));
        assert!(stated(serde_yaml::to_string(&yaml).unwrap().as_bytes()).is_err());
    }

    /// Matching offers alone cannot make an out-of-range choice valid.
    #[test]
    fn a_choice_must_name_a_dealt_combination() {
        let economy = Economy::embedded().unwrap();
        let battle = battle_from_grbr(&std::fs::read(TUFF).unwrap()).unwrap();
        let yaml = crate::battle::canonical_yaml(&battle).unwrap();
        for side in ["blue", "red"] {
            for choose in [-1, 4, i32::MAX] {
                let mut stated = stated(yaml.as_bytes()).unwrap().unwrap();
                let opening = if side == "blue" {
                    &mut stated.sides.blue.opening
                } else {
                    &mut stated.sides.red.opening
                };
                opening.choose = choose;
                let error = verify(&economy, &stated).unwrap_err();
                assert!(
                    error.contains(&format!("{side} opening choose {choose}")),
                    "{error}"
                );
            }
        }
    }

    #[test]
    fn the_opening_serializes_only_the_choice_and_offers() {
        let battle = battle_from_grbr(&std::fs::read(TUFF).unwrap()).unwrap();
        let yaml = serde_yaml::to_value(&battle).unwrap();
        for side in ["blue", "red"] {
            let opening = yaml["sides"][side]["opening"].as_mapping().unwrap();
            assert_eq!(opening.len(), 2);
            assert!(opening.contains_key(serde_yaml::Value::from("choose")));
            assert!(opening.contains_key(serde_yaml::Value::from("offers")));
        }
    }

    /// Nothing but a battle takes this path.
    #[test]
    fn a_layout_is_not_a_battle() {
        let layout = std::fs::read("../../tests/layouts/tuff-replay-round-7.yaml").unwrap();
        assert!(stated(&layout).unwrap().is_none());
        assert!(stated(b"not a document at all").unwrap().is_none());
    }

    /// The searched window is wide enough for every tracked match and no wider
    /// than the check can afford.
    #[test]
    fn the_pool_finishes_its_own_setup_inside_the_window() {
        let economy = Economy::embedded().unwrap();
        let battle = battle_from_grbr(&std::fs::read(TUFF).unwrap()).unwrap();
        let want: Deal = Deal {
            blue: battle.sides.blue.opening.offers.clone(),
            red: battle.sides.red.opening.offers.clone(),
        };
        let found = locate(&economy, battle.seed, &|made| *made == want).unwrap();
        assert!(!found.is_empty());
        assert!(found.iter().all(|offset| *offset < SEARCH_WINDOW));
        // The stream the seed starts is not where the deal starts, so the
        // offset is a real quantity rather than zero.
        assert!(found[0] > 0);
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
