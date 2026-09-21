//! Stateful build-2259 reinforcement prediction; see `docs/rules/reinforcements.md`.

use crate::battle::{Action, SideState, Turn};
use crate::catalog::{NativeFormation, resolve_unit_type};
use crate::economy::Economy;
use crate::opening::{Prediction, Stated, Stream};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
struct Config {
    ordinary_count: usize,
    unit_count: usize,
    cards: BTreeMap<i32, Card>,
    units: BTreeMap<i32, UnitCard>,
    pools: BTreeMap<i32, UnitPool>,
    weights: BTreeMap<i32, Vec<usize>>,
    prevented: BTreeMap<i32, Vec<i32>>,
    unit_costs: BTreeMap<i32, UnitCost>,
}

#[derive(Deserialize)]
struct Card {
    level: i32,
    group: i32,
    earliest: i32,
    latest: i32,
    repeated: bool,
    cooldown: bool,
    absent_units: Vec<i32>,
}

impl Card {
    fn condition(&self, units: &BTreeSet<i32>) -> bool {
        !self.absent_units.iter().any(|unit| units.contains(unit))
    }
}

#[derive(Deserialize)]
struct UnitCard {
    unit: i32,
    round: i32,
}

#[derive(Deserialize)]
struct UnitPool {
    rounds: Vec<i32>,
    /// What declining the unit offer of the round at the same position in
    /// `rounds` pays.
    decline_supply: Vec<i32>,
    supply_round: i32,
    supplies: Vec<i32>,
}

#[derive(Deserialize)]
struct UnitCost {
    supply: i32,
    unlock: i32,
    tech_step: i32,
    tech_cap: i32,
}

/// The unit index below which a side's formations were on the board before
/// this round's deliveries.
///
/// The round deals its reinforcement offers before its officers deliver, so a
/// squad delivered as the round opens is not part of what the deal reads. The
/// delivered squads took the last indices the round opened with, one per
/// officer whose `active_round` this is and who hands out a squad.
///
/// # Errors
///
/// Refuses a round in which an officer also unlocks a unit, since the state
/// does not say whether the unit was already unlocked and the deal reads the
/// unlocks. No officer of this build unlocks after round 1, which deals none.
fn delivered_before(economy: &Economy, side: &SideState, round: i32) -> Result<i32, String> {
    let mut squads = 0;
    for officer in &side.officers {
        let Some(row) = economy.officer(*officer) else {
            continue;
        };
        let Some(opening) = row.opening_unit else {
            continue;
        };
        if opening.unlock_round == round && round > 1 {
            return Err(format!(
                "officer {officer} unlocks a unit as round {round} opens, which the deal does \
                 not separate from an earlier unlock"
            ));
        }
        if row.active_round == round {
            squads += 1;
        }
    }
    Ok(side.next_index.unit - squads)
}

impl Config {
    fn embedded() -> Result<Self, String> {
        serde_yaml::from_str(include_str!("../../../config/reinforcements.yaml"))
            .map_err(|error| format!("cannot read reinforcement configuration: {error}"))
    }

    fn decline_supply(&self, economy: &Economy, pool: i32, round: i32) -> Result<i32, String> {
        let schedule = self
            .pools
            .get(&pool)
            .ok_or_else(|| format!("unit reinforcement pool {pool} is not in this build"))?;
        match schedule.rounds.iter().position(|unit| *unit == round) {
            Some(at) => schedule.decline_supply.get(at).copied().ok_or_else(|| {
                format!("unit reinforcement pool {pool} states no decline for round {round}")
            }),
            None => Ok(economy.reinforce_decline()),
        }
    }
}

/// What declining round `round`'s reinforcement offer pays, in a match dealt
/// from unit reinforcement pool `pool`.
///
/// An ordinary round pays [`Economy::reinforce_decline`]. A unit round pays
/// its pool's own figure for that round, `UnitReinforceRoundPool.giveUpSupply`
/// at the round's place in `roundGroup`, which grows through the match: pool 1
/// pays 50, 150, 400 and 700 across rounds 2, 5, 8 and 11. No local replay
/// shows a unit-round decline and the round after it, so the figure is the
/// configuration's rather than a measured one.
///
/// # Errors
/// Refuses a pool this build does not configure, or one that states no figure
/// for the round.
pub fn decline_supply(economy: &Economy, pool: i32, round: i32) -> Result<i32, String> {
    Config::embedded()?.decline_supply(economy, pool, round)
}

/// One computed draw with exact stream boundaries, excluding seed warm-up.
#[derive(Debug, Serialize)]
pub struct Round {
    pub round: i32,
    pub unit_reinforcement: bool,
    pub offers: Vec<i32>,
    /// What declining this round's offer pays; see [`decline_supply`].
    pub declined: i32,
    pub before_offset: u32,
    pub after_offset: u32,
    pub before_state: [u64; 4],
    pub after_state: [u64; 4],
}

/// Reinforcement verification covers the complete ordered draw of every turn.
#[derive(Debug, Serialize)]
pub struct Verified {
    pub rounds: Vec<Round>,
    pub offers_checked: usize,
}

struct Dealer {
    config: Config,
    stream: Stream,
    offset: u32,
    pool_id: i32,
    pools: BTreeMap<i32, Vec<i32>>,
    groups: BTreeMap<i32, Vec<i32>>,
    cooldown_rounds: BTreeSet<i32>,
}

impl Dealer {
    fn new(opening: &Prediction) -> Result<Self, String> {
        let config = Config::embedded()?;
        let mut pools: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
        let mut groups: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
        for (&id, card) in &config.cards {
            if card.group > 0 {
                groups.entry(card.group).or_default().push(id);
            }
            if card.group == 0 || opening.initialization.officers.get(&card.group) == Some(&id) {
                pools.entry(card.level).or_default().push(id);
            }
        }
        Ok(Self {
            config,
            stream: Stream::from_state(opening.after_opening_state)?,
            offset: opening.after_opening_offset,
            pool_id: opening.initialization.unit_round_pool,
            pools,
            groups,
            cooldown_rounds: BTreeSet::new(),
        })
    }

    fn refresh(&mut self, units: &BTreeSet<i32>) {
        let mut additions: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
        // Native OnNewRound visits levels, then sorted IDs. Replacements are
        // appended only after the original pools have all been visited.
        for pool in self.pools.values_mut() {
            pool.retain(|id| {
                let card = &self.config.cards[id];
                if card.condition(units) {
                    return true;
                }
                let eligible: Vec<i32> = self
                    .groups
                    .get(&card.group)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|id| self.config.cards[id].condition(units))
                    .collect();
                if !eligible.is_empty() {
                    let replacement = eligible[self.stream.pick(0, eligible.len())];
                    additions
                        .entry(self.config.cards[&replacement].level)
                        .or_default()
                        .push(replacement);
                }
                false
            });
        }
        for (level, ids) in additions {
            self.pools.entry(level).or_default().extend(ids);
        }
        for pool in self.pools.values_mut() {
            pool.sort_unstable();
        }
    }

    fn ordinary(&mut self, round: i32, units: &BTreeSet<i32>) -> Result<Vec<i32>, String> {
        self.refresh(units);
        let weights = self
            .config
            .weights
            .range(..=round)
            .next_back()
            .ok_or_else(|| format!("round {round} has no reinforcement probabilities"))?
            .1;
        let allowed = |id: &i32| {
            let card = &self.config.cards[id];
            card.earliest <= round
                && (card.latest <= 0 || round <= card.latest)
                && !(card.cooldown && self.cooldown_rounds.contains(&round))
        };
        let mut weights: Vec<usize> = weights
            .iter()
            .enumerate()
            .map(|(at, weight)| {
                let level = i32::try_from(at + 1).unwrap_or(i32::MAX);
                if self
                    .pools
                    .get(&level)
                    .is_some_and(|pool| pool.iter().any(&allowed))
                {
                    *weight
                } else {
                    0
                }
            })
            .collect();
        let mut taken: BTreeMap<i32, usize> = BTreeMap::new();
        let mut offers = Vec::new();
        for _ in 0..self.config.ordinary_count {
            let total = weights.iter().sum();
            if total == 0 {
                return Err(format!("round {round} exhausted its reinforcement levels"));
            }
            let mut value = self.stream.pick(0, total);
            let at = weights
                .iter()
                .position(|weight| {
                    if value < *weight {
                        true
                    } else {
                        value -= weight;
                        false
                    }
                })
                .ok_or("reinforcement level draw exceeds its weights")?;
            let level = i32::try_from(at + 1).map_err(|error| error.to_string())?;
            let pool = self
                .pools
                .get_mut(&level)
                .ok_or("reinforcement level has no pool")?;
            let candidates: Vec<usize> = pool
                .iter()
                .enumerate()
                .filter_map(|(at, id)| allowed(id).then_some(at))
                .collect();
            let position = taken.entry(level).or_default();
            if *position >= candidates.len() {
                return Err(format!("round {round} exhausted level {level}"));
            }
            // RandOnce skips the random call when only one candidate remains.
            let last = *position + 1 == candidates.len();
            let selected = if last {
                *position
            } else {
                self.stream.pick(*position, candidates.len())
            };
            let index = candidates[selected];
            offers.push(pool[index]);
            if !last {
                pool.swap(*position, index);
            }
            *position += 1;
            if *position == candidates.len() {
                weights[at] = 0;
            }
        }
        for pool in self.pools.values_mut() {
            pool.sort_unstable();
        }
        if offers.iter().any(|id| self.config.cards[id].cooldown) {
            self.cooldown_rounds.insert(round + 1);
        }
        Ok(offers)
    }

    fn unit_offers(
        &mut self,
        round: i32,
        units: &BTreeSet<i32>,
        officers: &BTreeSet<i32>,
        scores: &BTreeMap<i32, i32>,
    ) -> Result<Vec<i32>, String> {
        let mut excluded: BTreeSet<i32> = officers
            .iter()
            .filter_map(|id| self.config.prevented.get(id))
            .flatten()
            .copied()
            .collect();
        let mut candidates: Vec<(i32, i32)> = self
            .config
            .units
            .iter()
            .filter(|(_, card)| {
                card.round == round && !units.contains(&card.unit) && !excluded.contains(&card.unit)
            })
            .map(|(&id, card)| (id, card.unit))
            .collect();
        let mut offers = Vec::new();
        let mut remaining = self.config.unit_count.min(candidates.len());
        while remaining > 0 && !candidates.is_empty() {
            let (id, unit) = candidates[self.stream.pick(0, candidates.len())];
            offers.push(id);
            let old_len = candidates.len();
            candidates.retain(|(_, candidate_unit)| *candidate_unit != unit);
            // Native UnitFirstRand decrements for the selection AND for each
            // removed variant, so it can return fewer than the requested count.
            remaining = remaining.saturating_sub(1 + old_len - candidates.len());
            excluded.insert(unit);
        }
        let mut candidates: Vec<(i32, i32, i32)> = self
            .config
            .units
            .iter()
            .filter(|(_, card)| card.round == round && !excluded.contains(&card.unit))
            .map(|(&id, card)| (scores.get(&card.unit).copied().unwrap_or(0), id, card.unit))
            .collect();
        candidates.sort_unstable();
        while offers.len() < self.config.unit_count {
            let score = candidates
                .first()
                .ok_or_else(|| format!("round {round} exhausted unit reinforcements"))?
                .0;
            let count = candidates
                .iter()
                .take_while(|candidate| candidate.0 == score)
                .count();
            let (_, id, unit) = candidates[self.stream.pick(0, count)];
            offers.push(id);
            candidates.retain(|candidate| candidate.2 != unit);
        }
        let schedule = &self.config.pools[&self.pool_id];
        if round >= schedule.supply_round {
            let at = usize::try_from(round - schedule.supply_round)
                .map_err(|error| error.to_string())?;
            let at = at.min(schedule.supplies.len().saturating_sub(1));
            let supply = *schedule
                .supplies
                .get(at)
                .ok_or("unit reinforcement supply list is empty")?;
            *offers
                .last_mut()
                .ok_or("unit reinforcement deal is empty")? = supply;
        }
        Ok(offers)
    }

    fn choose(&mut self, id: i32) {
        let Some(card) = self.config.cards.get(&id) else {
            return;
        };
        if card.repeated {
            return;
        }
        for pool in self.pools.values_mut() {
            pool.retain(|candidate| *candidate != id);
        }
        if let Some(group) = self.groups.get_mut(&card.group) {
            group.clear();
        }
    }

    fn apply_choices(&mut self, turn: &Turn, offers: &[i32], terminal: bool) -> Result<(), String> {
        for (name, actions) in [("blue", &turn.actions.blue), ("red", &turn.actions.red)] {
            let choices: Vec<_> = actions
                .iter()
                .filter_map(|action| match action {
                    Action::ChooseReinforceItem { offer, id } => Some((*offer, *id)),
                    _ => None,
                })
                .collect();
            if choices.len() > 1 || (turn.round == 1 && !choices.is_empty()) {
                return Err(format!(
                    "round {} {name} has an invalid reinforcement choice count",
                    turn.round
                ));
            }
            if turn.round > 1 && !terminal && choices.is_empty() {
                return Err(format!(
                    "round {} {name} is missing its reinforcement choice",
                    turn.round
                ));
            }
            for (offer, id) in choices {
                if offer == crate::battle::DECLINED_OFFER && id.is_none() {
                    continue;
                }
                let predicted = usize::try_from(offer)
                    .ok()
                    .and_then(|at| offers.get(at))
                    .copied();
                if predicted.is_none() || predicted != id {
                    return Err(format!(
                        "round {} {name} reinforcement choice {offer} / {id:?} does not name a predicted offer",
                        turn.round
                    ));
                }
                self.choose(predicted.ok_or("reinforcement choice is missing")?);
            }
        }
        Ok(())
    }

    fn unit_cost(&self, unit: i32) -> Result<&UnitCost, String> {
        self.config
            .unit_costs
            .get(&unit)
            .ok_or_else(|| format!("unit {unit} has no reinforcement investment cost"))
    }

    fn context(&self, economy: &Economy, stated: &Stated, turn: &Turn) -> Result<Context, String> {
        let mut context = Context::default();
        for side in [&turn.state.blue, &turn.state.red] {
            context.officers.extend(side.officers.iter());
            let delivered = delivered_before(economy, side, turn.round)?;
            for formation in side
                .units
                .iter()
                .filter(|entry| entry.unit.index < delivered)
            {
                let native = resolve_unit_type(&formation.unit.type_name)
                    .ok_or_else(|| {
                        format!("unknown reinforcement unit {}", formation.unit.type_name)
                    })?
                    .native;
                let NativeFormation::Unit(unit) = native else {
                    return Err("formation is not a unit".into());
                };
                context.units.insert(unit);
                *context.scores.entry(unit).or_default() += self.unit_cost(unit)?.supply;
            }
            for &unit in &side.shop.unlocked_units {
                *context.scores.entry(unit).or_default() += self.unit_cost(unit)?.unlock;
            }
        }
        for (side, loadout) in [
            (&turn.state.blue, &stated.blue.tech_loadout),
            (&turn.state.red, &stated.red.tech_loadout),
        ] {
            for technology in &side.techs {
                let owner = economy
                    .technology_owner(*technology)
                    .ok_or_else(|| format!("unknown reinforcement technology {technology}"))?;
                if !loadout
                    .get(&owner)
                    .is_some_and(|ids| ids.contains(technology))
                {
                    return Err(format!(
                        "technology {technology} is outside its unit loadout"
                    ));
                }
            }
            // CalculateUpgradeCost sums base technology costs with each unit's
            // configured step/cap, ignoring officer discounts and paid prices.
            for (&unit, technologies) in loadout {
                let Some(score) = context.scores.get_mut(&unit) else {
                    continue;
                };
                let cost = self.unit_cost(unit)?;
                let step = if cost.tech_step > 0 {
                    cost.tech_step
                } else {
                    economy.technology_repeat_step()
                };
                for (count, technology) in technologies
                    .iter()
                    .filter(|id| side.techs.contains(id))
                    .enumerate()
                {
                    let base = economy.technology(*technology).ok_or_else(|| {
                        format!("technology {technology} has no reinforcement investment cost")
                    })?;
                    let count = i32::try_from(count).map_err(|error| error.to_string())?;
                    let total = base + count * step;
                    let total = if cost.tech_cap > 0 {
                        total.min(cost.tech_cap)
                    } else {
                        total
                    };
                    *score += total;
                }
            }
        }
        Ok(context)
    }
}

#[derive(Default)]
struct Context {
    units: BTreeSet<i32>,
    officers: BTreeSet<i32>,
    scores: BTreeMap<i32, i32>,
}

/// Verifies every reinforcement draw, advancing one stream from the opening.
/// State and prior choices are inputs; future offers never select a stream offset.
///
/// # Errors
/// Refuses missing/noncontiguous turns, mismatched offers or choices, unknown
/// prediction inputs, and exhausted pools. Reports the first failing round.
pub fn verify(
    economy: &Economy,
    stated: &Stated,
    opening: &Prediction,
) -> Result<Verified, String> {
    walk(economy, stated, opening, false).map(|walked| walked.verified)
}

/// Deals the offers of the round the battle has just opened, which is its
/// last, or nothing when that round is dealt none.
///
/// The deal is stateful, so this replays every earlier round first: each one's
/// offers are dealt again and the choice taken from them applied. The last
/// round's own `reinforce_offers` are the one thing not read, which is what
/// makes this a deal rather than a check — a match writes what comes back into
/// the state that round opens with, and [`verify`] then agrees with it.
///
/// # Errors
/// The same as [`verify`], for every round but the last.
pub fn deal_last_round(
    economy: &Economy,
    stated: &Stated,
    opening: &Prediction,
) -> Result<Option<Vec<i32>>, String> {
    walk(economy, stated, opening, true).map(|walked| walked.dealt)
}

/// One battle's rounds, replayed through the dealer.
struct Walked {
    verified: Verified,
    /// The last round's offers, when they were dealt rather than checked.
    dealt: Option<Vec<i32>>,
}

/// Replays every stated round through one stream.
///
/// `deal_last` decides what happens at the last round: a check compares the
/// offers it states, and a deal answers the offers it should state.
fn walk(
    economy: &Economy,
    stated: &Stated,
    opening: &Prediction,
    deal_last: bool,
) -> Result<Walked, String> {
    // A match that has not opened its first round has no draw to check. That
    // is a battle in progress rather than a battle missing something: the
    // header and the openings are all a dealt match has.
    if stated.turns.is_empty() {
        return Ok(Walked {
            verified: Verified {
                rounds: Vec::new(),
                offers_checked: 0,
            },
            dealt: None,
        });
    }
    let mut dealt = None;
    let mut dealer = Dealer::new(opening)?;
    let mut rounds = Vec::new();
    let mut offers_checked = 0;
    for (at, turn) in stated.turns.iter().enumerate() {
        let expected = i32::try_from(at + 1).map_err(|error| error.to_string())?;
        if turn.round != expected {
            return Err(format!(
                "reinforcement turns must be contiguous: expected round {expected}, found {}",
                turn.round
            ));
        }
        let context = dealer
            .context(economy, stated, turn)
            .map_err(|error| format!("round {} reinforcement: {error}", turn.round))?;
        let before_state = dealer.stream.state();
        let before_offset = dealer.offset + dealer.stream.draws();
        let unit_reinforcement = dealer.config.pools[&dealer.pool_id]
            .rounds
            .contains(&turn.round);
        let offers = if turn.round == 1 {
            Ok(Vec::new())
        } else if unit_reinforcement {
            dealer.unit_offers(
                turn.round,
                &context.units,
                &context.officers,
                &context.scores,
            )
        } else {
            dealer.ordinary(turn.round, &context.units)
        }
        .map_err(|error| format!("round {} reinforcement: {error}", turn.round))?;
        let last = at + 1 == stated.turns.len();
        let expected_offers = if turn.round == 1 { None } else { Some(&offers) };
        if deal_last && last {
            dealt = expected_offers.cloned();
        } else if turn.state.reinforce_offers.as_ref() != expected_offers {
            return Err(format!(
                "round {} reinforcement offers disagree: predicted {expected_offers:?}, stated {:?}",
                turn.round, turn.state.reinforce_offers
            ));
        }
        dealer.apply_choices(turn, &offers, last)?;
        if turn.round > 1 {
            offers_checked += offers.len();
            rounds.push(Round {
                round: turn.round,
                unit_reinforcement,
                offers,
                declined: dealer
                    .config
                    .decline_supply(economy, dealer.pool_id, turn.round)?,
                before_offset,
                after_offset: dealer.offset + dealer.stream.draws(),
                before_state,
                after_state: dealer.stream.state(),
            });
        }
    }
    Ok(Walked {
        verified: Verified {
            rounds,
            offers_checked,
        },
        dealt,
    })
}

#[cfg(all(test, feature = "convert"))]
mod tests {
    use super::*;

    /// A unit round's decline pays its pool's figure for that round, and any
    /// other round the ordinary one.
    #[test]
    fn a_decline_pays_by_pool_and_round() {
        let economy = Economy::embedded().unwrap();
        let paid = |pool, round| decline_supply(&economy, pool, round).unwrap();
        // Pool 1 deals units in rounds 2, 5, 8 and 11.
        assert_eq!(
            [2, 5, 8, 11].map(|round| paid(1, round)),
            [50, 150, 400, 700]
        );
        assert_eq!(paid(1, 3), economy.reinforce_decline());
        // Pool 22's first unit round is 3, and it pays more than round 2 would.
        assert_eq!(paid(22, 3), 100);
        assert!(decline_supply(&economy, 0, 2).is_err());
    }

    fn sample() -> Stated {
        let bytes = std::fs::read(
            "../../replay/battle/2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].yaml",
        )
        .unwrap();
        crate::opening::stated(&bytes).unwrap().unwrap()
    }

    /// A match deals the round it has just opened rather than checking one
    /// already written, and what it deals is what the replay dealt.
    ///
    /// Every prefix of a tracked battle is replayed and its last round dealt
    /// again, so this covers a first round that is dealt nothing, an ordinary
    /// round and a unit round alike.
    #[test]
    fn the_round_a_match_opens_is_dealt_as_the_replay_dealt_it() {
        let economy = Economy::embedded().unwrap();
        let full = sample();
        let opening = crate::opening::verify(&economy, &full).unwrap();
        let mut dealt = 0;
        for rounds in 1..=full.turns.len() {
            let mut opened = sample();
            opened.turns.truncate(rounds);
            let stated = opened.turns[rounds - 1].state.reinforce_offers.clone();
            let answered = deal_last_round(&economy, &opened, &opening)
                .unwrap_or_else(|error| panic!("round {rounds}: {error}"));
            assert_eq!(answered, stated, "round {rounds}");
            dealt += usize::from(answered.is_some());
        }
        // The first round is dealt nothing, and every later one a full hand.
        assert_eq!(dealt, full.turns.len() - 1);
        assert!(dealt > 5, "the sample is long enough to cover a unit round");
    }

    #[test]
    fn altered_offers_and_choices_are_refused_at_their_round() {
        let economy = Economy::embedded().unwrap();
        let mut stated = sample();
        let opening = crate::opening::verify(&economy, &stated).unwrap();
        let original = stated.turns[1].state.reinforce_offers.clone();
        stated.turns[1]
            .state
            .reinforce_offers
            .as_mut()
            .unwrap()
            .swap(0, 1);
        assert!(
            verify(&economy, &stated, &opening)
                .unwrap_err()
                .contains("round 2 reinforcement offers disagree")
        );
        stated.turns[1].state.reinforce_offers = None;
        assert!(
            verify(&economy, &stated, &opening)
                .unwrap_err()
                .contains("round 2 reinforcement offers disagree")
        );
        stated.turns[1].state.reinforce_offers = original;
        stated.turns[1].actions.blue = vec![Action::ChooseReinforceItem { offer: 4, id: None }];
        assert!(
            verify(&economy, &stated, &opening)
                .unwrap_err()
                .contains("round 2 blue reinforcement choice")
        );
    }

    #[test]
    fn omitted_or_noncontiguous_rounds_are_not_successful_verification() {
        let economy = Economy::embedded().unwrap();
        let mut stated = sample();
        let opening = crate::opening::verify(&economy, &stated).unwrap();
        stated.turns.remove(1);
        assert!(
            verify(&economy, &stated, &opening)
                .unwrap_err()
                .contains("contiguous")
        );
        // A battle with no rounds at all is a match that has been dealt and
        // not opened, which has no draw to disagree with.
        stated.turns.clear();
        let dealt = verify(&economy, &stated, &opening).unwrap();
        assert!(dealt.rounds.is_empty());
        assert_eq!(dealt.offers_checked, 0);
    }

    #[test]
    fn unit_deals_and_prior_choices_are_checked() {
        let economy = Economy::embedded().unwrap();
        let mut stated = sample();
        let opening = crate::opening::verify(&economy, &stated).unwrap();
        let checked = verify(&economy, &stated, &opening).unwrap();
        let round = checked
            .rounds
            .iter()
            .find(|round| round.unit_reinforcement)
            .unwrap()
            .round;
        stated.turns[usize::try_from(round - 1).unwrap()]
            .state
            .reinforce_offers
            .as_mut()
            .unwrap()
            .swap(0, 1);
        assert!(
            verify(&economy, &stated, &opening)
                .unwrap_err()
                .contains(&format!("round {round} reinforcement offers disagree"))
        );

        let mut stated = sample();
        stated.turns[1]
            .actions
            .blue
            .retain(|action| !matches!(action, Action::ChooseReinforceItem { .. }));
        assert!(
            verify(&economy, &stated, &opening)
                .unwrap_err()
                .contains("round 2 blue is missing its reinforcement choice")
        );
    }
}
