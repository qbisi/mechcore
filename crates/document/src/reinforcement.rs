//! Stateful reinforcement prediction; see `docs/rules/reinforcements.md`.

use crate::catalog::{NativeFormation, resolve_unit_type};
use crate::economy::Economy;
use crate::r#match::{Action, Offers, PoolDelta, SideState, State, Turn};
use crate::opening::{Prediction, Stated, Stream};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
struct Config {
    ordinary_count: usize,
    unit_count: usize,
    /// Every level-4 commander skill, in the build's order: what a deal that
    /// offers one of them excludes from the next round.
    level_four_skills: Vec<i32>,
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
    absent_units: Vec<i32>,
    /// `EAppearCondition.SupplyPercent`: the card is offered only while its
    /// units hold a share of every player's investment inside these bounds.
    #[serde(default)]
    supply_share: Option<SupplyShare>,
}

impl Card {
    fn condition(&self, context: &Context) -> bool {
        !self
            .absent_units
            .iter()
            .any(|unit| context.units.contains(unit))
            && self.supply_share.as_ref().is_none_or(|share| {
                context
                    .investments
                    .iter()
                    .all(|investment| share.admits(investment))
            })
    }
}

/// `appearConditionParameter` in percent, and the officer's `unitID`.
#[derive(Deserialize)]
struct SupplyShare {
    low: i64,
    high: i64,
    units: Vec<i32>,
}

impl SupplyShare {
    /// `ReinforcePool.CheckReinforeCondition` on one player's economy.
    ///
    /// A player who has invested nothing passes only a share that may be
    /// zero. The build reads a `low` above `high` as a band the share must
    /// stay outside; no card of this build has one, and [`Dealer::new`]
    /// refuses it.
    fn admits(&self, investment: &Investment) -> bool {
        if investment.total <= 0 {
            return self.low <= 0;
        }
        let held: i64 = self
            .units
            .iter()
            .filter_map(|unit| investment.values.get(unit))
            .sum();
        let share = 100 * held;
        investment.total * self.low <= share && share <= investment.total * self.high
    }
}

/// `PlayerUnitEconomy`: what one player has put into each unit type it
/// fields, and the sum.
#[derive(Default)]
struct Investment {
    values: BTreeMap<i32, i64>,
    total: i64,
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
    /// What one level costs, the same for every level.
    upgrade: i32,
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
        if row.active_round.contains(&round) {
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
    /// The pool as each stated round opened, from round 1.
    #[serde(skip)]
    pub pools: Vec<Pool>,
    #[serde(skip)]
    pub checkpoints: Vec<Checkpoint>,
}

impl Verified {
    /// A captured first deal is not a verified draw, but its refund is checked.
    #[must_use]
    pub fn declined(&self, turn: &Turn) -> Option<i32> {
        self.rounds
            .iter()
            .find(|dealt| dealt.round == turn.round)
            .map(|dealt| dealt.declined)
            .or_else(|| {
                turn.state
                    .reinforce_offers
                    .as_ref()
                    .map(|offers| offers.refund)
            })
    }
}

/// A deployment opening after its reinforcement deal, before either side acts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub random_state: [u64; 4],
    pub reinforce_pool: PoolDelta,
}

impl Checkpoint {
    pub fn write_to(&self, state: &mut State) {
        state.random_state = self.random_state;
        state.reinforce_pool.clone_from(&self.reinforce_pool);
    }
}

/// An opening's offers and the checkpoint after dealing them.
pub struct Dealt {
    pub offers: Option<Offers>,
    pub checkpoint: Checkpoint,
}

/// The pool as a round opens, in the terms a replay's `MatchSnapshotData`
/// restores it from: `ReinforcePool.ApplayOperation` replays the log onto the
/// pool the seed initializes, and `RoundExcludeReinforce` names the rounds
/// that exclude the level-4 commander skills.
#[derive(Debug, Clone)]
pub struct Pool {
    /// `poolOPs`: every removal `(0, id)` and addition `(1, id)` since the
    /// match began, in order.
    pub log: Vec<(i32, i32)>,
    /// Where each earlier round's choices sit in `log`. Both players choose in
    /// one round, and the log keeps the order they chose in, which a match
    /// does not state; no reading of the pool depends on it.
    pub choices: Vec<std::ops::Range<usize>>,
    /// The rounds a level-4 commander skill was offered before, each one
    /// round after the offer.
    pub excluded: Vec<i32>,
}

/// Whether a side may hold `card` more than once: the pool deals a repeatable
/// card again, and takes any other out once it is chosen
/// (`ReinforcePool.SelectReinforce`). An officer the pool never deals, an
/// opening's specialist, is held once.
///
/// # Errors
/// Refuses a reinforcement configuration that does not parse.
pub fn repeatable(card: i32) -> Result<bool, String> {
    Config::embedded().map(|config| config.cards.get(&card).is_some_and(|card| card.repeated))
}

/// Every level-4 commander skill, in the order a replay's
/// `RoundExcludeReinforce` lists them under each round it excludes them from.
///
/// # Errors
/// Refuses a reinforcement configuration that does not parse.
pub fn level_four_skills() -> Result<Vec<i32>, String> {
    Config::embedded().map(|config| config.level_four_skills)
}

struct Dealer {
    config: Config,
    stream: Stream,
    offset: u32,
    pool_id: i32,
    pools: BTreeMap<i32, Vec<i32>>,
    groups: BTreeMap<i32, Vec<i32>>,
    excluded_rounds: BTreeSet<i32>,
    /// `Pool::log` so far.
    log: Vec<(i32, i32)>,
    /// `Pool::choices` so far.
    choices: Vec<std::ops::Range<usize>>,
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
            if card
                .supply_share
                .as_ref()
                .is_some_and(|share| share.low > share.high)
            {
                return Err(format!(
                    "card {id} bounds a share of supply from above its floor, which the \
                     reinforcement prediction does not model"
                ));
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
            excluded_rounds: BTreeSet::new(),
            log: Vec::new(),
            choices: Vec::new(),
        })
    }

    fn members(&self) -> BTreeSet<i32> {
        self.pools.values().flatten().copied().collect()
    }

    fn checkpoint(&self, initial: &BTreeSet<i32>) -> Checkpoint {
        let members = self.members();
        Checkpoint {
            random_state: self.stream.state(),
            reinforce_pool: PoolDelta {
                removed: initial.difference(&members).copied().collect(),
                added: members.difference(initial).copied().collect(),
            },
        }
    }

    fn restore(&mut self, state: &State) -> Result<(), String> {
        self.stream = Stream::from_state(state.random_state)?;
        let mut members = self.members();
        for ids in [&state.reinforce_pool.removed, &state.reinforce_pool.added] {
            if ids.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err("reinforce_pool lists must be unique and in card ID order".into());
            }
        }
        for id in &state.reinforce_pool.removed {
            if !members.remove(id) {
                return Err(format!(
                    "reinforce_pool removes card {id} outside the initial pool"
                ));
            }
        }
        for id in &state.reinforce_pool.added {
            if !self.config.cards.contains_key(id)
                || !members.insert(*id)
                || state.reinforce_pool.removed.contains(id)
            {
                return Err(format!("reinforce_pool adds invalid or initial card {id}"));
            }
        }
        for variants in self.groups.values_mut() {
            let held = variants.iter().filter(|id| members.contains(id)).count();
            if held > 1 {
                return Err("reinforce_pool holds multiple cards from one group".into());
            }
            if held == 0 {
                variants.clear();
            }
        }
        self.pools.clear();
        for id in members {
            self.pools
                .entry(self.config.cards[&id].level)
                .or_default()
                .push(id);
        }
        Ok(())
    }

    /// Continue from an available post-deal state without its earlier history.
    fn continue_after(&mut self, economy: &Economy, turn: &Turn, last: bool) -> Result<(), String> {
        self.restore(&turn.state)?;
        self.offset = 0;
        let refund = self
            .config
            .decline_supply(economy, self.pool_id, turn.round)?;
        let count = if self.config.pools[&self.pool_id]
            .rounds
            .contains(&turn.round)
        {
            self.config.unit_count
        } else {
            self.config.ordinary_count
        };
        if (turn.round == 1 && turn.state.reinforce_offers.is_some())
            || (turn.round > 1
                && turn
                    .state
                    .reinforce_offers
                    .as_ref()
                    .is_none_or(|offers| offers.refund != refund || offers.dealt.len() != count))
        {
            return Err(format!(
                "round {} captured reinforcement offer shape or refund disagrees",
                turn.round
            ));
        }
        let offers = turn
            .state
            .reinforce_offers
            .as_ref()
            .map_or(&[][..], |offers| offers.dealt.as_slice());
        if offers
            .iter()
            .any(|id| self.config.level_four_skills.contains(id))
        {
            self.excluded_rounds.insert(
                turn.round
                    .checked_add(1)
                    .ok_or("reinforcement round overflow")?,
            );
        }
        self.apply_choices(turn, offers, last)
    }

    fn deal(
        &mut self,
        economy: &Economy,
        stated: &Stated,
        turn: &Turn,
    ) -> Result<Option<Offers>, String> {
        if turn.round == 1 {
            return Ok(None);
        }
        let context = self.context(economy, stated, turn)?;
        let dealt = if self.config.pools[&self.pool_id]
            .rounds
            .contains(&turn.round)
        {
            self.unit_offers(
                turn.round,
                &context.units,
                &context.officers,
                &context.scores,
            )?
        } else {
            self.ordinary(turn.round, &context)?
        };
        Ok(Some(Offers {
            dealt,
            refund: self
                .config
                .decline_supply(economy, self.pool_id, turn.round)?,
        }))
    }

    fn refresh(&mut self, context: &Context) {
        let mut additions: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
        // Native OnNewRound visits levels, then sorted IDs. Replacements are
        // appended only after the original pools have all been visited.
        let log = &mut self.log;
        for pool in self.pools.values_mut() {
            pool.retain(|id| {
                let card = &self.config.cards[id];
                if card.condition(context) {
                    return true;
                }
                log.push((0, *id));
                let eligible: Vec<i32> = self
                    .groups
                    .get(&card.group)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|id| self.config.cards[id].condition(context))
                    .collect();
                if !eligible.is_empty() {
                    let replacement = eligible[self.stream.pick(0, eligible.len())];
                    // The addition is logged beside the removal it replaces.
                    log.push((1, replacement));
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

    fn ordinary(&mut self, round: i32, context: &Context) -> Result<Vec<i32>, String> {
        self.refresh(context);
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
                && !(self.excluded_rounds.contains(&round)
                    && self.config.level_four_skills.contains(id))
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
        if offers
            .iter()
            .any(|id| self.config.level_four_skills.contains(id))
        {
            self.excluded_rounds.insert(round + 1);
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
        // `SelectReinforce` takes the card and then every other variant of its
        // group, logging each one the pool held.
        let variants = self.groups.get(&card.group).cloned().unwrap_or_default();
        for taken in std::iter::once(id).chain(variants.into_iter().filter(|other| *other != id)) {
            for pool in self.pools.values_mut() {
                if let Some(at) = pool.iter().position(|candidate| *candidate == taken) {
                    pool.remove(at);
                    self.log.push((0, taken));
                }
            }
        }
        if let Some(group) = self.groups.get_mut(&card.group) {
            group.clear();
        }
    }

    fn apply_choices(&mut self, turn: &Turn, offers: &[i32], terminal: bool) -> Result<(), String> {
        let from = self.log.len();
        for (name, actions) in [("blue", &turn.actions.blue), ("red", &turn.actions.red)] {
            let choices: Vec<_> = actions
                .iter()
                .filter_map(|action| match action {
                    Action::ChooseReinforceItem { index, id } => Some((*index, *id)),
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
                // The decline sits after the cards dealt.
                if id.is_none() {
                    if usize::try_from(offer).ok() != Some(offers.len()) {
                        return Err(format!(
                            "round {} {name} declines at {offer}, and the decline is offer {}",
                            turn.round,
                            offers.len()
                        ));
                    }
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
        if self.log.len() > from {
            self.choices.push(from..self.log.len());
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
        let sides = [
            (&turn.state.blue, &stated.blue.tech_loadout),
            (&turn.state.red, &stated.red.tech_loadout),
        ];
        let mut investments = Vec::with_capacity(sides.len());
        for (side, _) in sides {
            context.officers.extend(side.officers.iter());
            let delivered = delivered_before(economy, side, turn.round)?;
            let mut investment = Investment::default();
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
                let cost = self.unit_cost(unit)?;
                *context.scores.entry(unit).or_default() += cost.supply;
                // UnitUtility.CalculateUpgradeLevelCost: the card's price and
                // every level the formation has risen through.
                let levels = formation.unit.level.unwrap_or(1) - 1;
                *investment.values.entry(unit).or_default() +=
                    i64::from(cost.supply + levels * cost.upgrade);
            }
            for &unit in &side.unlocked_units {
                *context.scores.entry(unit).or_default() += self.unit_cost(unit)?.unlock;
            }
            investments.push(investment);
        }
        for ((side, loadout), investment) in sides.into_iter().zip(&mut investments) {
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
            for (&unit, technologies) in loadout {
                let researched = self.technology_investment(economy, side, unit, technologies)?;
                if let Some(score) = context.scores.get_mut(&unit) {
                    *score += researched;
                }
                // A player's economy adds a type's unlock price and its
                // technologies once, for each type it fields.
                if let Some(value) = investment.values.get_mut(&unit) {
                    *value += i64::from(self.unit_cost(unit)?.unlock + researched);
                }
            }
            investment.total = investment.values.values().sum();
        }
        context.investments = investments;
        Ok(context)
    }

    /// `UnitTechnologyManager.CalculateUpgradeCost`: the base price of each
    /// technology `side` holds for `unit`, with the unit's step and cap and no
    /// officer discount or paid price.
    fn technology_investment(
        &self,
        economy: &Economy,
        side: &SideState,
        unit: i32,
        technologies: &[i32],
    ) -> Result<i32, String> {
        let cost = self.unit_cost(unit)?;
        let step = if cost.tech_step > 0 {
            cost.tech_step
        } else {
            economy.technology_repeat_step()
        };
        let mut total = 0;
        for (count, technology) in technologies
            .iter()
            .filter(|id| side.techs.contains(id))
            .enumerate()
        {
            let base = economy.technology(*technology).ok_or_else(|| {
                format!("technology {technology} has no reinforcement investment cost")
            })?;
            let count = i32::try_from(count).map_err(|error| error.to_string())?;
            let price = base + count * step;
            total += if cost.tech_cap > 0 {
                price.min(cost.tech_cap)
            } else {
                price
            };
        }
        Ok(total)
    }
}

#[derive(Default)]
struct Context {
    units: BTreeSet<i32>,
    officers: BTreeSet<i32>,
    scores: BTreeMap<i32, i32>,
    /// One per real player, blue then red.
    investments: Vec<Investment>,
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
    walk(economy, stated, opening, true)
}

/// Fills the checkpoints while converting a complete replay; normal verification
/// checks them against the seed rather than trusting them.
#[cfg(feature = "convert")]
pub(crate) fn fill_checkpoints(
    economy: &Economy,
    game: &mut crate::r#match::Match,
) -> Result<(), String> {
    let yaml = crate::r#match::canonical_yaml(game)?;
    let stated = crate::opening::stated(yaml.as_bytes())?.ok_or("not a match")?;
    let opening = crate::opening::verify(economy, &stated)?;
    let walked = walk(economy, &stated, &opening, false)?;
    for (turn, checkpoint) in game.turns.iter_mut().zip(walked.checkpoints) {
        checkpoint.write_to(&mut turn.state);
    }
    Ok(())
}

/// Restore native pre-deal checkpoints when the spectator cache lacks history.
/// The native pool log is cumulative; no missing decisions are inferred.
#[cfg(feature = "convert")]
pub(crate) fn fill_recorded_checkpoints(
    economy: &Economy,
    game: &mut crate::r#match::Match,
    recorded: &[crate::record::MatchRound],
) -> Result<(), String> {
    let yaml = crate::r#match::canonical_yaml(game)?;
    let stated = crate::opening::stated(yaml.as_bytes())?.ok_or("not a match")?;
    let opening = crate::opening::verify(economy, &stated)?;
    for (turn, source) in game.turns.iter_mut().zip(recorded.iter().skip(1)) {
        let mut dealer = Dealer::new(&opening)?;
        let initial = dealer.members();
        let mut members = initial.clone();
        for operation in &source.pool_operations.entries {
            match operation.operation {
                0 if members.remove(&operation.id) => {}
                1 if dealer.config.cards.contains_key(&operation.id)
                    && members.insert(operation.id) => {}
                _ => {
                    return Err(format!(
                        "round {} has invalid pool operation {} on card {}",
                        source.round, operation.operation, operation.id
                    ));
                }
            }
        }
        let random_state = source
            .random_state
            .states
            .values
            .as_slice()
            .try_into()
            .map_err(|_| {
                format!(
                    "round {} requires a four-word reinforcement stream",
                    source.round
                )
            })?;
        dealer.restore(&State {
            random_state,
            reinforce_pool: PoolDelta {
                removed: initial.difference(&members).copied().collect(),
                added: members.difference(&initial).copied().collect(),
            },
            ..Default::default()
        })?;
        for exclusion in &source.excluded_rounds.entries {
            if !same_excluded_cards(&exclusion.ids.values, &dealer.config.level_four_skills) {
                return Err(format!(
                    "round {} excludes unknown reinforcement cards",
                    source.round
                ));
            }
            dealer.excluded_rounds.insert(exclusion.round);
        }
        let offers = dealer
            .deal(economy, &stated, turn)
            .map_err(|error| format!("round {} reinforcement: {error}", turn.round))?;
        if offers != turn.state.reinforce_offers {
            return Err(format!(
                "round {} native checkpoint deals {offers:?}, recorded {:?}",
                turn.round, turn.state.reinforce_offers
            ));
        }
        dealer.checkpoint(&initial).write_to(&mut turn.state);
    }
    // Check transitions between available checkpoints as well as each deal.
    let yaml = crate::r#match::canonical_yaml(game)?;
    let stated = crate::opening::stated(yaml.as_bytes())?.ok_or("not a match")?;
    verify(economy, &stated, &opening)?;
    Ok(())
}

/// Exclusions are membership tests; their serialized enumeration order is immaterial.
#[cfg(feature = "convert")]
pub(crate) fn same_excluded_cards(recorded: &[i32], expected: &[i32]) -> bool {
    recorded.len() == expected.len()
        && recorded.iter().copied().collect::<BTreeSet<_>>() == expected.iter().copied().collect()
}

/// Deals the last opening using only its header, the preceding opening and
/// that round's decisions. Earlier turns are not read.
///
/// # Errors
/// Refuses a missing opening, nonconsecutive checkpoints, invalid pool changes
/// or stream state, invalid choices, and an exhausted or unsupported deal.
pub fn deal_checkpoint(economy: &Economy, stated: &Stated) -> Result<Dealt, String> {
    let next = stated.turns.last().ok_or("no deployment opening to deal")?;
    let opening = crate::opening::predict(economy, stated.seed, stated.map_id)?;
    let mut dealer = Dealer::new(&opening)?;
    let initial = dealer.members();
    if next.round > 1 {
        let current = stated
            .turns
            .iter()
            .rev()
            .nth(1)
            .ok_or("next opening needs the preceding round checkpoint")?;
        if current.round + 1 != next.round {
            return Err("checkpoint rounds are not consecutive".into());
        }
        dealer.continue_after(economy, current, false)?;
    }
    let offers = dealer.deal(economy, stated, next)?;
    Ok(Dealt {
        offers,
        checkpoint: dealer.checkpoint(&initial),
    })
}

/// Replays every stated round through one stream.
fn walk(
    economy: &Economy,
    stated: &Stated,
    opening: &Prediction,
    check_state: bool,
) -> Result<Verified, String> {
    // A match that has not opened its first round has no draw to check. That
    // is a match in progress rather than a match missing something: the
    // header and the openings are all a dealt match has.
    if stated.turns.is_empty() {
        return Ok(Verified {
            rounds: Vec::new(),
            offers_checked: 0,
            pools: Vec::new(),
            checkpoints: Vec::new(),
        });
    }
    let mut dealer = Dealer::new(opening)?;
    let initial = dealer.members();
    let mut checkpoints = Vec::new();
    let mut rounds = Vec::new();
    let mut offers_checked = 0;
    let mut pools = Vec::new();
    for (at, turn) in stated.turns.iter().enumerate() {
        let first = if stated.has_opening {
            1
        } else {
            stated.turns[0].round
        };
        let expected = first
            .checked_add(i32::try_from(at).map_err(|error| error.to_string())?)
            .ok_or("reinforcement round overflow")?;
        if turn.round != expected {
            return Err(format!(
                "reinforcement turns must be contiguous: expected round {expected}, found {}",
                turn.round
            ));
        }
        if at == 0 && !stated.has_opening {
            // This state is already post-deal. Its missing prefix cannot be
            // checked against the seed, only subsequent transitions can.
            dealer.continue_after(economy, turn, at + 1 == stated.turns.len())?;
            continue;
        }
        pools.push(Pool {
            log: dealer.log.clone(),
            choices: dealer.choices.clone(),
            excluded: dealer.excluded_rounds.iter().copied().collect(),
        });
        let before_state = dealer.stream.state();
        let before_offset = dealer.offset + dealer.stream.draws();
        let unit_reinforcement = dealer.config.pools[&dealer.pool_id]
            .rounds
            .contains(&turn.round);
        let expected_offers = dealer
            .deal(economy, stated, turn)
            .map_err(|error| format!("round {} reinforcement: {error}", turn.round))?;
        let offers = expected_offers
            .as_ref()
            .map_or_else(Vec::new, |offers| offers.dealt.clone());
        let last = at + 1 == stated.turns.len();
        let declined = dealer
            .config
            .decline_supply(economy, dealer.pool_id, turn.round)?;
        if turn.state.reinforce_offers != expected_offers {
            return Err(format!(
                "round {} reinforcement offers disagree: predicted {expected_offers:?}, stated {:?}",
                turn.round, turn.state.reinforce_offers
            ));
        }
        let checkpoint = dealer.checkpoint(&initial);
        if check_state
            && (turn.state.random_state != checkpoint.random_state
                || turn.state.reinforce_pool != checkpoint.reinforce_pool)
        {
            return Err(format!(
                "round {} reinforcement checkpoint disagrees with its seeded deal",
                turn.round
            ));
        }
        checkpoints.push(checkpoint);
        dealer.apply_choices(turn, &offers, last)?;
        if turn.round > 1 {
            offers_checked += offers.len();
            rounds.push(Round {
                round: turn.round,
                unit_reinforcement,
                offers,
                declined,
                before_offset,
                after_offset: dealer.offset + dealer.stream.draws(),
                before_state,
                after_state: dealer.stream.state(),
            });
        }
    }
    Ok(Verified {
        rounds,
        offers_checked,
        pools,
        checkpoints,
    })
}

#[cfg(all(test, feature = "convert"))]
mod tests {
    use super::*;

    #[test]
    fn a_native_suffix_deals_from_its_recorded_checkpoint() {
        use crate::r#match::{Match, MatchSide, Opening, TurnActions};
        use crate::record;

        let economy = Economy::embedded().unwrap();
        let opening = crate::opening::predict(economy, 12345, 1011).unwrap();
        let side = |offers: Vec<_>, constructions| MatchSide {
            name: "player".into(),
            opening: Opening {
                choose: None,
                offers,
            },
            constructions,
            tech_loadout: economy.unit_technologies(),
            seed: Some(1),
        };
        let mut opened = SideState {
            reactor_core: 4800,
            random_state: [1, 2, 3, 4],
            ..Default::default()
        };
        crate::transition::open_allowances(&mut opened);
        let mut game = Match {
            has_opening: false,
            game_build: crate::game_build().into(),
            map_id: 1011,
            seed: 12345,
            deploy_time: None,
            blue: side(
                opening.deal.blue.clone(),
                opening.constructions.blue.clone(),
            ),
            red: side(opening.deal.red.clone(), opening.constructions.red.clone()),
            turns: vec![Turn {
                round: 5,
                state: State {
                    blue: opened.clone(),
                    red: opened,
                    ..Default::default()
                },
                actions: TurnActions::default(),
            }],
        };
        let mut dealer = Dealer::new(&opening).unwrap();
        let initial = dealer.members();
        for round in 2..5 {
            dealer.ordinary(round, &Context::default()).unwrap();
        }
        let mut native: record::MatchRound =
            quick_xml::de::from_str("<MatchSnapshotData><round>5</round></MatchSnapshotData>")
                .unwrap();
        native.random_state.states.values = dealer.stream.state().to_vec();
        native.pool_operations.entries = dealer
            .log
            .iter()
            .map(|&(operation, id)| record::PoolOperation { operation, id })
            .collect();
        native.excluded_rounds.entries = dealer
            .excluded_rounds
            .iter()
            .map(|&round| {
                let mut values = dealer.config.level_four_skills.clone();
                values.reverse();
                record::ExcludedRound {
                    round,
                    ids: record::ExcludedIds { values },
                }
            })
            .collect();
        let stated =
            crate::opening::stated(crate::r#match::canonical_yaml(&game).unwrap().as_bytes())
                .unwrap()
                .unwrap();
        game.turns[0].state.reinforce_offers =
            dealer.deal(economy, &stated, &game.turns[0]).unwrap();
        let expected = dealer.checkpoint(&initial);
        let placeholder =
            quick_xml::de::from_str("<MatchSnapshotData><round>0</round></MatchSnapshotData>")
                .unwrap();
        let mut native = vec![placeholder, native];
        fill_recorded_checkpoints(economy, &mut game, &native).unwrap();
        assert_eq!(game.turns[0].state.random_state, expected.random_state);
        assert_eq!(game.turns[0].state.reinforce_pool, expected.reinforce_pool);
        let yaml = crate::r#match::canonical_yaml(&game).unwrap();
        assert_eq!(
            crate::r#match::read(yaml.as_bytes()).unwrap().unwrap(),
            game
        );
        let stated = crate::opening::stated(yaml.as_bytes()).unwrap().unwrap();
        assert!(!stated.has_opening);
        assert_eq!(
            verify(economy, &stated, &opening).unwrap().offers_checked,
            0
        );
        assert!(
            crate::match_replay::match_replay(economy, &stated, crate::game_build())
                .unwrap_err()
                .contains("partial match")
        );
        native[1].random_state.states.values = vec![0; 4];
        assert!(fill_recorded_checkpoints(economy, &mut game, &native).is_err());
    }

    #[test]
    fn a_pool_checkpoint_continues_without_earlier_draws_or_choices() {
        let economy = Economy::embedded().unwrap();
        let mut replacements = 0;
        let mut removals = 0;
        let mut exclusions = 0;
        for seed in 1..=32 {
            let opening = crate::opening::predict(economy, seed, 1011).unwrap();
            let mut dealer = Dealer::new(&opening).unwrap();
            let initial = dealer.members();
            let mut context = Context::default();
            let mut offers = dealer.ordinary(2, &context).unwrap();
            for round in 2..=12 {
                // Exercise disappearance and replacement as investment changes.
                context.investments = vec![Investment {
                    values: BTreeMap::from([(1, 400), (2, 600)]),
                    total: 1000,
                }];
                if round >= 5 {
                    context.units.insert(1);
                }
                if round >= 8 {
                    context.units.insert(2);
                }
                let checkpoint = dealer.checkpoint(&initial);
                replacements += checkpoint.reinforce_pool.added.len();
                removals += checkpoint.reinforce_pool.removed.len();
                let state = State {
                    random_state: checkpoint.random_state,
                    reinforce_pool: checkpoint.reinforce_pool,
                    ..Default::default()
                };
                let mut resumed = Dealer::new(&opening).unwrap();
                resumed.restore(&state).unwrap();
                if offers
                    .iter()
                    .any(|id| dealer.config.level_four_skills.contains(id))
                {
                    resumed.excluded_rounds.insert(round + 1);
                    exclusions += 1;
                }
                // Both players may select the same card; removal is idempotent.
                for id in offers.iter().take(2) {
                    dealer.choose(*id);
                    dealer.choose(*id);
                    resumed.choose(*id);
                    resumed.choose(*id);
                }
                assert_eq!(dealer.members(), resumed.members());
                offers = dealer.ordinary(round + 1, &context).unwrap();
                assert_eq!(
                    offers,
                    resumed.ordinary(round + 1, &context).unwrap(),
                    "seed {seed} round {round}"
                );
                assert_eq!(dealer.checkpoint(&initial), resumed.checkpoint(&initial));
            }
        }
        assert!(replacements > 0 && removals > 0 && exclusions > 0);
    }

    #[test]
    fn a_pool_checkpoint_refuses_noncanonical_or_impossible_membership() {
        let opening = crate::opening::predict(Economy::embedded().unwrap(), 1, 1011).unwrap();
        let initial = Dealer::new(&opening).unwrap();
        let id = *initial.members().first().unwrap();
        for delta in [
            PoolDelta {
                added: vec![id],
                ..Default::default()
            },
            PoolDelta {
                removed: vec![id, id],
                ..Default::default()
            },
            PoolDelta {
                removed: vec![id],
                added: vec![id],
            },
            PoolDelta {
                removed: vec![i32::MAX],
                ..Default::default()
            },
        ] {
            let mut restored = Dealer::new(&opening).unwrap();
            assert!(
                restored
                    .restore(&State {
                        random_state: initial.stream.state(),
                        reinforce_pool: delta,
                        ..Default::default()
                    })
                    .is_err()
            );
        }
    }

    /// A unit-modification officer is dealt while its unit holds at most its
    /// ceiling of every player's investment, and a player who has invested
    /// nothing never stops it.
    #[test]
    fn a_share_of_investment_bounds_every_player() {
        let share = SupplyShare {
            low: 0,
            high: 15,
            units: vec![1],
        };
        let investment = |fortress: i64, marksman: i64| Investment {
            values: BTreeMap::from([(1, fortress), (2, marksman)]),
            total: fortress + marksman,
        };
        assert!(share.admits(&investment(150, 850)));
        assert!(!share.admits(&investment(151, 849)));
        assert!(share.admits(&investment(0, 400)));
        assert!(share.admits(&Investment::default()));
        let floored = SupplyShare { low: 1, ..share };
        assert!(!floored.admits(&Investment::default()));
    }

    /// A unit round's decline pays its pool's figure for that round, and any
    /// other round the ordinary one.
    #[test]
    fn a_decline_pays_by_pool_and_round() {
        let economy = Economy::embedded().unwrap();
        let paid = |pool, round| decline_supply(economy, pool, round).unwrap();
        // Pool 1 deals units in rounds 2, 5, 8 and 11.
        assert_eq!(
            [2, 5, 8, 11].map(|round| paid(1, round)),
            [50, 150, 400, 700]
        );
        assert_eq!(paid(1, 3), economy.reinforce_decline());
        // Pool 22's first unit round is 3, and it pays more than round 2 would.
        assert_eq!(paid(22, 3), 100);
        assert!(decline_supply(economy, 0, 2).is_err());
    }
}
