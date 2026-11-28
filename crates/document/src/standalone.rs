//! Standalone `kind: state` documents and expanded `solve` problem serialization.
//!
//! `docs/spec/document/state.md` and `docs/spec/mechcore/solve.md` define both:
//! a standalone state supplements root/side rule context (`build`, `reinforce`,
//! `max_round`, `map_id`, `seed`, `tech_loadout`) so single-step deployment and
//! simulation run without an external `match` header, while `problem_json`
//! produces the pure-JSON expanded problem descriptor for `solve` and `arena`.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as JsonValue};
use serde_yaml::Value as YamlValue;

use crate::layout::StaticPlacement;
use crate::r#match::{
    Action, Allowances, Match, MatchFormat, MatchSide, Offers, OpeningOffer, SideState, State,
};

/// Default seed assigned to a standalone `state` document when `seed` is omitted.
pub const DEFAULT_SEED: i32 = 1;

/// A self-contained `kind: state` document spanning either round 0 (`Opening`)
/// or a deployment round `round >= 1` (`Round`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandaloneState {
    pub build: Option<String>,
    pub reinforce: Option<bool>,
    pub max_round: Option<i32>,
    pub map_id: Option<i32>,
    pub seed: i32,
    pub round: i32,
    pub body: StandaloneBody,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StandaloneBody {
    Opening(Box<StandaloneOpening>),
    Round(Box<StandaloneRound>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandaloneOpening {
    pub blue: StandaloneOpeningSide,
    pub red: StandaloneOpeningSide,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandaloneOpeningSide {
    pub tech_loadout: Option<BTreeMap<i32, Vec<i32>>>,
    pub reactor_core: i32,
    pub seed: Option<i32>,
    pub offers: Vec<OpeningOffer>,
    pub constructions: Vec<StaticPlacement>,
    /// Post-opening runtime fields present after applying round-0 actions via `act`.
    pub supply: Option<i32>,
    pub officers: Vec<i32>,
    pub shop: Option<Allowances>,
    pub reinforce_chosen: Option<bool>,
}

impl StandaloneOpeningSide {
    /// Whether this side has already chosen its round-0 advance team.
    #[must_use]
    pub fn has_chosen(&self) -> bool {
        self.supply.is_some() || !self.officers.is_empty() || self.shop.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandaloneRound {
    pub state: State,
    pub has_random_state: bool,
    pub side_has_random_state: [bool; 2],
    pub tech_loadout: [Option<BTreeMap<i32, Vec<i32>>>; 2],
    pub explicit_shop: [bool; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpeningWire {
    #[serde(default, with = "crate::names::card::many_opt")]
    reinforce_pool: Option<Vec<i32>>,
    blue: OpeningSideWire,
    red: OpeningSideWire,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpeningSideWire {
    reactor_core: i32,
    #[serde(default)]
    seed: Option<i32>,
    offers: Vec<OpeningOffer>,
    constructions: Vec<StaticPlacement>,
    #[serde(default)]
    supply: Option<i32>,
    #[serde(default, with = "crate::names::officer::many")]
    officers: Vec<i32>,
    #[serde(default)]
    shop: Option<Allowances>,
    #[serde(default)]
    reinforce_chosen: Option<bool>,
}

impl StandaloneState {
    /// Whether reinforcements are enabled according to the state header (default `true`).
    #[must_use]
    pub fn effective_reinforce(&self) -> bool {
        self.reinforce.unwrap_or(true)
    }

    /// Declared `build`, defaulting to this binary's `GAME_VERSION`.
    #[must_use]
    pub fn effective_build(&self) -> &str {
        self.build.as_deref().unwrap_or(crate::game_build())
    }

    /// Whether either reactor core has reached zero or below.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        match &self.body {
            StandaloneBody::Opening(opening) => {
                opening.blue.reactor_core <= 0 || opening.red.reactor_core <= 0
            }
            StandaloneBody::Round(round) => {
                round.state.blue.reactor_core <= 0 || round.state.red.reactor_core <= 0
            }
        }
    }

    /// Whether this state represents a clean round-opening checkpoint with no
    /// in-round deployment edits applied yet.
    #[must_use]
    pub fn is_round_opening(&self) -> bool {
        match &self.body {
            StandaloneBody::Opening(opening) => {
                [opening.blue(), opening.red()].into_iter().all(|side| {
                    side.supply.is_none()
                        && side.officers.is_empty()
                        && side.shop.is_none()
                        && side.reinforce_chosen.is_none()
                })
            }
            StandaloneBody::Round(round) => [(0, &round.state.blue), (1, &round.state.red)]
                .into_iter()
                .all(|(idx, side)| {
                    let clean_choice =
                        matches!(side.reinforce_choice, None | Some(Offers::UNANSWERED));
                    let clean_deployment = side.energy_tower_skills.is_empty()
                        && side.battle_skills.iter().all(|s| s.release.is_none())
                        && side.units.iter().all(|u| {
                            !u.recovered
                                && u.travelling.is_none()
                                && u.record_position.is_none()
                                && u.record_rotated.is_none()
                                && (self.round == 1
                                    || u.source != crate::layout::UnitSource::Joined)
                        });
                    let clean_shop = if round.explicit_shop[idx] {
                        let mut opened = side.clone();
                        crate::transition::open_allowances(&mut opened);
                        side.shop == opened.shop
                    } else {
                        true
                    };
                    clean_choice && clean_deployment && clean_shop
                }),
        }
    }

    /// Returns the `tech_loadout` map for the requested side (`false` = blue, `true` = red).
    #[must_use]
    pub fn tech_loadout(&self, red: bool) -> Option<&BTreeMap<i32, Vec<i32>>> {
        let idx = usize::from(red);
        match &self.body {
            StandaloneBody::Opening(opening) => [
                opening.blue.tech_loadout.as_ref(),
                opening.red.tech_loadout.as_ref(),
            ][idx],
            StandaloneBody::Round(round) => round.tech_loadout[idx].as_ref(),
        }
    }

    /// Parses a standalone `kind: state` document from UTF-8 YAML or JSON bytes.
    ///
    /// # Errors
    /// Returns an error when the bytes are not a valid `kind: state` document
    /// or violate state invariants.
    pub fn from_yaml_bytes(bytes: &[u8]) -> Result<Self, String> {
        let value: YamlValue = serde_yaml::from_slice(bytes).map_err(|e| e.to_string())?;
        Self::from_yaml_value(value)
    }

    /// Parses a standalone `kind: state` document from a parsed JSON value,
    /// preserving full `u64` precision for `random_state` words.
    ///
    /// # Errors
    /// Returns an error when the JSON value is not a valid `kind: state` object
    /// or violates state invariants.
    pub fn from_json_value(value: &JsonValue) -> Result<Self, String> {
        let yaml = json_to_yaml(value)?;
        Self::from_yaml_value(yaml)
    }

    /// Parses a standalone `kind: state` document from a `serde_yaml::Value`.
    ///
    /// # Errors
    /// Returns an error when fields, types, or invariants are invalid.
    #[allow(clippy::too_many_lines)]
    pub fn from_yaml_value(mut value: YamlValue) -> Result<Self, String> {
        if value.get("kind").and_then(YamlValue::as_str) != Some("state") {
            return Err("expected kind: state".into());
        }
        let round = value
            .get("round")
            .and_then(YamlValue::as_i64)
            .and_then(|n| i32::try_from(n).ok())
            .filter(|n| *n >= 0)
            .ok_or("state needs round >= 0")?;

        let fields = value.as_mapping_mut().ok_or("state must be a mapping")?;
        let build = remove_opt(fields, "build")
            .map(|v| serde_yaml::from_value::<Option<String>>(v).map_err(|e| format!("build: {e}")))
            .transpose()?
            .flatten();
        if let Some(ref b) = build
            && b.trim().is_empty()
        {
            return Err("state build must not be empty".into());
        }
        let reinforce = remove_opt(fields, "reinforce")
            .map(|v| {
                serde_yaml::from_value::<Option<bool>>(v).map_err(|e| format!("reinforce: {e}"))
            })
            .transpose()?
            .flatten();
        let max_round = optional_i32(fields, "max_round")?;
        if let Some(mr) = max_round
            && mr < 1
        {
            return Err(format!("state max_round must be >= 1, not {mr}"));
        }
        let map_id = optional_i32(fields, "map_id")?;
        let seed = match optional_i32(fields, "seed")? {
            Some(0) => return Err("state seed 0 is not valid".into()),
            Some(s) => s,
            None => DEFAULT_SEED,
        };

        if round == 0 {
            let body = parse_opening_body(fields, reinforce, seed)?;
            let parsed = Self {
                build,
                reinforce,
                max_round,
                map_id,
                seed,
                round,
                body: StandaloneBody::Opening(Box::new(body)),
            };
            parsed.validate()?;
            return Ok(parsed);
        }

        let pool_key = YamlValue::String("reinforce_pool".into());
        let mut compacted_pool = None;
        if let Some(pool_val) = fields.get(&pool_key) {
            if reinforce == Some(false) {
                return Err("state states reinforce_pool while reinforcements are disabled".into());
            }
            if pool_val.is_sequence()
                && let Some(seq) = fields.remove(&pool_key)
            {
                let members = crate::names::card::many::deserialize(seq)
                    .map_err(|e| format!("reinforce_pool: {e}"))?;
                compacted_pool = Some(crate::reinforcement::compact_pool(seed, &members)?);
            }
        }

        let has_random_state = extract_random_state(fields, "state")?;
        let mut side_has_random_state = [false, false];
        let mut tech_loadout = [None, None];
        let mut explicit_shop = [false, false];
        let mut parsed_shop = [None, None];
        let mut reinforce_chosen = [None, None];

        for (idx, side_name) in ["blue", "red"].into_iter().enumerate() {
            let side_val = fields
                .get_mut(YamlValue::String(side_name.into()))
                .ok_or_else(|| format!("state is missing side {side_name}"))?;
            let side_map = side_val
                .as_mapping_mut()
                .ok_or_else(|| format!("state side {side_name} must be a mapping"))?;
            crate::board::normalize_expanded_round_side(side_map, side_name)?;
            side_has_random_state[idx] = extract_random_state(side_map, side_name)?;
            if let Some(loadout) = remove_opt(side_map, "tech_loadout") {
                tech_loadout[idx] = Some(
                    crate::names::loadout::deserialize(loadout)
                        .map_err(|e| format!("{side_name} tech_loadout: {e}"))?,
                );
            }
            if let Some(shop_val) = remove_opt(side_map, "shop") {
                let shop: Allowances = serde_yaml::from_value(shop_val)
                    .map_err(|e| format!("{side_name} shop: {e}"))?;
                check_shop(&shop, side_name)?;
                parsed_shop[idx] = Some(shop);
                explicit_shop[idx] = true;
            }
            reinforce_chosen[idx] = remove_opt(side_map, "reinforce_chosen")
                .map(|v| {
                    serde_yaml::from_value::<bool>(v)
                        .map_err(|e| format!("{side_name} reinforce_chosen: {e}"))
                })
                .transpose()?;
            let unlocked_key = YamlValue::String("unlocked_units".into());
            if !side_map.contains_key(&unlocked_key) {
                side_map.insert(unlocked_key, YamlValue::Sequence(Vec::new()));
            }
            let towers_key = YamlValue::String("tower_strengthen_levels".into());
            if !side_map.contains_key(&towers_key) {
                side_map.insert(
                    towers_key,
                    YamlValue::Sequence(vec![
                        YamlValue::Number(0.into());
                        crate::layout::TOWER_COUNT
                    ]),
                );
            }
        }

        let mut state: State = crate::r#match::payload(value).map_err(|e| e.to_string())?;
        if let Some(delta) = compacted_pool {
            state.reinforce_pool = delta;
        }
        if reinforce == Some(false)
            && (state.reinforce_offers.is_some() || !state.reinforce_pool.is_empty())
        {
            return Err("state states reinforcements while reinforcements are disabled".into());
        }
        if round == 1 && state.reinforce_offers.is_some() {
            return Err("round 1 cannot hold reinforce_offers".into());
        }

        for (idx, (side_name, side)) in [("blue", &mut state.blue), ("red", &mut state.red)]
            .into_iter()
            .enumerate()
        {
            if state.reinforce_offers.is_some() && side.reinforce_choice.is_none() {
                side.reinforce_choice = Some(Offers::UNANSWERED);
            }
            if let Some(chosen) = reinforce_chosen[idx]
                && chosen != side.reinforce_choice.is_some_and(|i| i >= 0)
            {
                return Err(format!(
                    "{side_name} reinforce_chosen disagrees with reinforce_choice"
                ));
            }
            if let Some(shop) = parsed_shop[idx].take() {
                side.shop = shop;
            } else {
                crate::transition::open_allowances(side);
            }
        }

        let parsed = Self {
            build,
            reinforce,
            max_round,
            map_id,
            seed,
            round,
            body: StandaloneBody::Round(Box::new(StandaloneRound {
                state,
                has_random_state,
                side_has_random_state,
                tech_loadout,
                explicit_shop,
            })),
        };
        parsed.validate()?;
        Ok(parsed)
    }

    /// Validates all structural, reinforcement, board, and stream invariants of a standalone state.
    ///
    /// # Errors
    /// Returns an error if any standalone state invariant is violated.
    pub fn validate(&self) -> Result<(), String> {
        if self.seed == 0 {
            return Err("state seed 0 is not valid".into());
        }
        if let Some(mr) = self.max_round
            && mr < 1
        {
            return Err(format!("state max_round must be >= 1, not {mr}"));
        }
        match &self.body {
            StandaloneBody::Opening(opening) => {
                for (name, side) in [("blue", &opening.blue), ("red", &opening.red)] {
                    if side.reactor_core <= 0 {
                        return Err(format!(
                            "{name} round 0 reactor_core must be > 0, not {}",
                            side.reactor_core
                        ));
                    }
                    if side.offers.len() != crate::opening::CHOOSE_COUNT {
                        return Err(format!(
                            "{name} opening requires {} offers, got {}",
                            crate::opening::CHOOSE_COUNT,
                            side.offers.len()
                        ));
                    }
                    if let Some(ref shop) = side.shop {
                        check_shop(shop, name)?;
                    }
                    if side.reinforce_chosen == Some(true) {
                        return Err(format!("{name} round 0 reinforce_chosen cannot be true"));
                    }
                }
            }
            StandaloneBody::Round(round) => {
                let s = &round.state;
                if round.has_random_state && s.random_state == [0; 4] {
                    return Err("state random_state must not be all zeros".into());
                }
                if self.reinforce == Some(false)
                    && (s.reinforce_offers.is_some() || !s.reinforce_pool.is_empty())
                {
                    return Err(
                        "state states reinforcements while reinforcements are disabled".into(),
                    );
                }
                if self.round == 1 && s.reinforce_offers.is_some() {
                    return Err("round 1 cannot hold reinforce_offers".into());
                }
                if self.effective_reinforce() {
                    crate::reinforcement::expand_pool(self.seed, &s.reinforce_pool)?;
                }
                s.check_reinforcement(false)?;
                s.check_towers(true)?;
                s.blue.check_board(false)?;
                s.red.check_board(false)?;
                for (idx, (side_name, side)) in
                    [("blue", &s.blue), ("red", &s.red)].into_iter().enumerate()
                {
                    if round.side_has_random_state[idx] && side.random_state == [0; 4] {
                        return Err(format!("{side_name} random_state must not be all zeros"));
                    }
                    check_shop(&side.shop, side_name)?;
                    check_next_index(side, side_name)?;
                }
            }
        }
        Ok(())
    }

    /// Extracts the standalone opening state of `round` from an immutable [`Match`].
    ///
    /// # Errors
    /// Returns an error when the match lacks the requested round or opening state.
    pub fn from_match_round(game: &Match, round: i32) -> Result<Self, String> {
        if round == 0 {
            if !game.has_opening {
                return Err("match has no round 0 opening state".into());
            }
            let extract_side =
                |side: &MatchSide, name: &str| -> Result<StandaloneOpeningSide, String> {
                    let reactor_core = side.reactor_core.filter(|&c| c > 0).ok_or_else(|| {
                        format!("{name} round 0 state has no positive reactor_core")
                    })?;
                    Ok(StandaloneOpeningSide {
                        tech_loadout: Some(side.tech_loadout.clone()),
                        reactor_core,
                        seed: side.seed,
                        offers: side.opening.offers.clone(),
                        constructions: side.constructions.clone(),
                        supply: None,
                        officers: Vec::new(),
                        shop: None,
                        reinforce_chosen: None,
                    })
                };
            let parsed = Self {
                build: Some(game.game_build.clone()),
                reinforce: Some(game.reinforce),
                max_round: Some(game.max_round),
                map_id: Some(game.map_id),
                seed: game.seed,
                round: 0,
                body: StandaloneBody::Opening(Box::new(StandaloneOpening {
                    blue: extract_side(&game.blue, "blue")?,
                    red: extract_side(&game.red, "red")?,
                })),
            };
            parsed.validate()?;
            return Ok(parsed);
        }
        let turn = game
            .turns
            .iter()
            .find(|t| t.round == round)
            .ok_or_else(|| format!("this match has no round {round}"))?;
        let mut state = turn.state.clone();
        for side in [&mut state.blue, &mut state.red] {
            if side.tower_strengthen_levels.is_empty() {
                side.tower_strengthen_levels = vec![0; crate::layout::TOWER_COUNT];
            }
        }
        state.open_reinforcement();
        crate::transition::open_allowances(&mut state.blue);
        crate::transition::open_allowances(&mut state.red);
        let parsed = Self {
            build: Some(game.game_build.clone()),
            reinforce: Some(game.reinforce),
            max_round: Some(game.max_round),
            map_id: Some(game.map_id),
            seed: game.seed,
            round,
            body: StandaloneBody::Round(Box::new(StandaloneRound {
                state,
                has_random_state: true,
                side_has_random_state: [true, true],
                tech_loadout: [
                    Some(game.blue.tech_loadout.clone()),
                    Some(game.red.tech_loadout.clone()),
                ],
                explicit_shop: [false, false],
            })),
        };
        parsed.validate()?;
        Ok(parsed)
    }

    /// Serializes this standalone state into a pure JSON `kind: state` object.
    ///
    /// # Errors
    /// Returns an error if expanding or compacting `reinforce_pool` requires a missing
    /// `seed`, or if serialization of catalog names fails.
    #[allow(clippy::too_many_lines)]
    pub fn to_json_value(
        &self,
        format: MatchFormat,
        runtime_fields: bool,
    ) -> Result<JsonValue, String> {
        let mut root = JsonMap::new();
        root.insert("kind".into(), JsonValue::String("state".into()));
        if let Some(build) = &self.build {
            root.insert("build".into(), JsonValue::String(build.clone()));
        }
        if let Some(reinforce) = self.reinforce {
            root.insert("reinforce".into(), JsonValue::Bool(reinforce));
        }
        if let Some(max_round) = self.max_round {
            root.insert("max_round".into(), JsonValue::from(max_round));
        }
        if let Some(map_id) = self.map_id {
            root.insert("map_id".into(), JsonValue::from(map_id));
        }
        root.insert("seed".into(), JsonValue::from(self.seed));
        root.insert("round".into(), JsonValue::from(self.round));

        match &self.body {
            StandaloneBody::Opening(opening) => {
                if format == MatchFormat::Expanded && self.effective_reinforce() {
                    let members = crate::reinforcement::initial_members(self.seed)?;
                    root.insert("reinforce_pool".into(), serialize_cards(&members)?);
                }
                for (name, side) in [("blue", &opening.blue), ("red", &opening.red)] {
                    root.insert(
                        name.into(),
                        opening_side_to_json(side, format, runtime_fields)?,
                    );
                }
            }
            StandaloneBody::Round(round) => {
                if round.has_random_state {
                    root.insert(
                        "random_state".into(),
                        random_state_to_json(round.state.random_state),
                    );
                }
                if self.effective_reinforce() {
                    match format {
                        MatchFormat::Expanded => {
                            let members = crate::reinforcement::expand_pool(
                                self.seed,
                                &round.state.reinforce_pool,
                            )?;
                            root.insert("reinforce_pool".into(), serialize_cards(&members)?);
                        }
                        MatchFormat::Compact => {
                            if !round.state.reinforce_pool.is_empty() {
                                root.insert(
                                    "reinforce_pool".into(),
                                    serde_json::to_value(&round.state.reinforce_pool)
                                        .map_err(|e| e.to_string())?,
                                );
                            }
                        }
                    }
                }
                if let Some(offers) = &round.state.reinforce_offers {
                    root.insert(
                        "reinforce_offers".into(),
                        serde_json::to_value(offers).map_err(|e| e.to_string())?,
                    );
                }
                let choice_visibility = if !runtime_fields && self.is_round_opening() {
                    ChoiceVisibility::HideOpening
                } else {
                    ChoiceVisibility::Keep
                };
                for (idx, (name, side)) in [("blue", &round.state.blue), ("red", &round.state.red)]
                    .into_iter()
                    .enumerate()
                {
                    let side_json = round_side_to_json(
                        side,
                        round.tech_loadout[idx].as_ref(),
                        SideJsonOpts {
                            has_random_state: round.side_has_random_state[idx],
                            explicit_shop: round.explicit_shop[idx],
                            runtime_fields,
                            choice_visibility,
                            format,
                        },
                    )?;
                    root.insert(name.into(), side_json);
                }
            }
        }
        Ok(JsonValue::Object(root))
    }

    /// Builds an expanded pure-JSON `problem` object for a standalone state (`history` omitted).
    ///
    /// Materializes omitted `tech_loadout` fields as full maps per `solve.md`.
    ///
    /// # Errors
    /// Returns an error if the state cannot be serialized in expanded form.
    pub fn problem_json(&self, ref_id: Option<&str>) -> Result<JsonValue, String> {
        let mut materialized = self.clone();
        let full_loadout = || -> Result<BTreeMap<i32, Vec<i32>>, String> {
            Ok(crate::economy::Economy::embedded()?.unit_technologies())
        };
        match &mut materialized.body {
            StandaloneBody::Opening(opening) => {
                if opening.blue.tech_loadout.is_none() {
                    opening.blue.tech_loadout = Some(full_loadout()?);
                }
                if opening.red.tech_loadout.is_none() {
                    opening.red.tech_loadout = Some(full_loadout()?);
                }
            }
            StandaloneBody::Round(round) => {
                for slot in &mut round.tech_loadout {
                    if slot.is_none() {
                        *slot = Some(full_loadout()?);
                    }
                }
            }
        }
        let mut problem = JsonMap::new();
        problem.insert(
            "state".into(),
            materialized.to_json_value(MatchFormat::Expanded, false)?,
        );
        if let Some(ref_id) = ref_id {
            problem.insert("ref".into(), JsonValue::String(ref_id.into()));
        }
        Ok(JsonValue::Object(problem))
    }
}

impl StandaloneOpening {
    const fn blue(&self) -> &StandaloneOpeningSide {
        &self.blue
    }

    const fn red(&self) -> &StandaloneOpeningSide {
        &self.red
    }
}

/// Builds the pure-JSON expanded `problem` object (`solve.md`) for solving
/// `target_round` in `game`.
///
/// Includes `state` (expanded standalone opening state of `target_round`),
/// optional `history` (expanded segment states + resolved `actions` for rounds
/// strictly before `target_round`), and optional `ref`.
///
/// # Errors
/// Returns an error if `target_round` is absent or segments cannot be expanded.
pub fn problem_json(
    game: &Match,
    target_round: i32,
    ref_id: Option<&str>,
) -> Result<JsonValue, String> {
    let state = StandaloneState::from_match_round(game, target_round)?
        .to_json_value(MatchFormat::Expanded, false)?;
    let mut problem = JsonMap::new();
    problem.insert("state".into(), state);

    let mut history = Vec::new();
    if target_round >= 1 && game.has_opening {
        let mut open_state = JsonMap::new();
        open_state.insert("kind".into(), JsonValue::String("state".into()));
        open_state.insert("round".into(), JsonValue::from(0));
        if game.reinforce {
            let members = crate::reinforcement::initial_members(game.seed)?;
            open_state.insert("reinforce_pool".into(), serialize_cards(&members)?);
        }
        open_state.insert(
            "blue".into(),
            match_opening_side_to_json(&game.blue, "blue")?,
        );
        open_state.insert("red".into(), match_opening_side_to_json(&game.red, "red")?);
        let blue_open: Vec<Action> = game.blue.opening.action().into_iter().collect();
        let red_open: Vec<Action> = game.red.opening.action().into_iter().collect();
        history.push(serde_json::json!({
            "round": 0,
            "state": JsonValue::Object(open_state),
            "actions": {
                "blue": blue_open,
                "red": red_open,
            },
        }));
    }
    let economy = crate::economy::Economy::embedded()?;
    for turn in &game.turns {
        if turn.round >= target_round {
            break;
        }
        history.push(history_round_entry(game, economy, turn)?);
    }
    if !history.is_empty() {
        problem.insert("history".into(), JsonValue::Array(history));
    }
    if let Some(ref_id) = ref_id {
        problem.insert("ref".into(), JsonValue::String(ref_id.into()));
    }
    Ok(JsonValue::Object(problem))
}

fn history_round_entry(
    game: &Match,
    economy: &crate::economy::Economy,
    turn: &crate::r#match::Turn,
) -> Result<JsonValue, String> {
    let mut round_state = JsonMap::new();
    round_state.insert("kind".into(), JsonValue::String("state".into()));
    round_state.insert("round".into(), JsonValue::from(turn.round));
    round_state.insert(
        "random_state".into(),
        random_state_to_json(turn.state.random_state),
    );
    if game.reinforce {
        let members = crate::reinforcement::expand_pool(game.seed, &turn.state.reinforce_pool)?;
        round_state.insert("reinforce_pool".into(), serialize_cards(&members)?);
    }
    if let Some(offers) = &turn.state.reinforce_offers {
        round_state.insert(
            "reinforce_offers".into(),
            serde_json::to_value(offers).map_err(|e| e.to_string())?,
        );
    }
    let opts = SideJsonOpts {
        has_random_state: true,
        explicit_shop: false,
        runtime_fields: false,
        choice_visibility: ChoiceVisibility::HideOpening,
        format: MatchFormat::Expanded,
    };
    round_state.insert(
        "blue".into(),
        round_side_to_json(&turn.state.blue, None, opts)?,
    );
    round_state.insert(
        "red".into(),
        round_side_to_json(&turn.state.red, None, opts)?,
    );
    let offers = turn.state.reinforce_offers.as_ref();
    let blue_actions = crate::normalize::normalize_turn_side(
        economy,
        turn.round,
        &turn.state.blue,
        &turn.actions.blue,
        offers,
        false,
    )?;
    let red_actions = crate::normalize::normalize_turn_side(
        economy,
        turn.round,
        &turn.state.red,
        &turn.actions.red,
        offers,
        true,
    )?;
    Ok(serde_json::json!({
        "round": turn.round,
        "state": JsonValue::Object(round_state),
        "actions": {
            "blue": blue_actions,
            "red": red_actions,
        },
    }))
}

fn json_to_yaml(value: &JsonValue) -> Result<YamlValue, String> {
    serde_yaml::to_value(value).map_err(|e| format!("cannot convert JSON state to YAML value: {e}"))
}

fn remove_opt(mapping: &mut serde_yaml::Mapping, key: &str) -> Option<YamlValue> {
    mapping.remove(YamlValue::String(key.into()))
}

fn optional_i32(mapping: &mut serde_yaml::Mapping, name: &str) -> Result<Option<i32>, String> {
    remove_opt(mapping, name)
        .map(|v| serde_yaml::from_value::<Option<i32>>(v).map_err(|e| format!("{name}: {e}")))
        .transpose()
        .map(Option::flatten)
}

fn extract_random_state(mapping: &mut serde_yaml::Mapping, owner: &str) -> Result<bool, String> {
    let key = YamlValue::String("random_state".into());
    if let Some(existing) = mapping.get(&key) {
        let words: [u64; 4] = serde_yaml::from_value(existing.clone())
            .map_err(|e| format!("{owner} random_state: {e}"))?;
        if words == [0; 4] {
            return Err(format!("{owner} random_state must not be all zeros"));
        }
        Ok(true)
    } else {
        mapping.insert(key, YamlValue::Sequence(vec![0.into(); 4]));
        Ok(false)
    }
}

fn check_shop(shop: &Allowances, side_name: &str) -> Result<(), String> {
    if shop.buys_remaining < 0 || shop.unlocks_remaining < 0 || shop.contraptions_remaining < 0 {
        return Err(format!("{side_name} shop counts must be >= 0"));
    }
    Ok(())
}

fn check_next_index(side: &SideState, side_name: &str) -> Result<(), String> {
    if side.next_index.unit < 0 || side.next_index.contraption < 0 {
        return Err(format!("{side_name} next_index counts must be >= 0"));
    }
    if let Some(max_unit) = side.units.iter().map(|u| u.index).max()
        && side.next_index.unit <= max_unit
    {
        return Err(format!(
            "{side_name} next_index.unit ({}) must be greater than max unit index ({max_unit})",
            side.next_index.unit
        ));
    }
    if let Some(max_contraption) = side.contraptions.iter().map(|c| c.index).max()
        && side.next_index.contraption <= max_contraption
    {
        return Err(format!(
            "{side_name} next_index.contraption ({}) must be greater than max contraption index ({max_contraption})",
            side.next_index.contraption
        ));
    }
    Ok(())
}

fn parse_opening_body(
    fields: &mut serde_yaml::Mapping,
    reinforce: Option<bool>,
    seed: i32,
) -> Result<StandaloneOpening, String> {
    let _ = remove_opt(fields, "kind");
    let _ = remove_opt(fields, "round");

    let mut tech_loadout = [None, None];
    for (idx, side_name) in ["blue", "red"].into_iter().enumerate() {
        let side_map = fields
            .get_mut(YamlValue::String(side_name.into()))
            .and_then(YamlValue::as_mapping_mut)
            .ok_or_else(|| format!("round 0 state requires side {side_name} mapping"))?;
        if let Some(loadout) = remove_opt(side_map, "tech_loadout") {
            tech_loadout[idx] = Some(
                crate::names::loadout::deserialize(loadout)
                    .map_err(|e| format!("{side_name} tech_loadout: {e}"))?,
            );
        }
        crate::board::normalize_expanded_opening_side(side_map, side_name)?;
    }

    let wire: OpeningWire = serde_yaml::from_value(YamlValue::Mapping(std::mem::take(fields)))
        .map_err(|e| format!("round 0 state is not readable: {e}"))?;

    if let Some(members) = wire.reinforce_pool.as_deref() {
        if reinforce == Some(false) {
            return Err("round 0 states reinforce_pool while reinforcements are disabled".into());
        }
        let delta = crate::reinforcement::compact_pool(seed, members)
            .map_err(|e| format!("round 0: {e}"))?;
        if !delta.is_empty() {
            return Err(
                "round 0 reinforce_pool disagrees with the match seed's initial pool".into(),
            );
        }
    }

    let build_side = |side: OpeningSideWire,
                      loadout: Option<BTreeMap<i32, Vec<i32>>>,
                      name: &str|
     -> Result<StandaloneOpeningSide, String> {
        if side.reactor_core <= 0 {
            return Err(format!(
                "{name} round 0 reactor_core must be > 0, not {}",
                side.reactor_core
            ));
        }
        if side.offers.len() != crate::opening::CHOOSE_COUNT {
            return Err(format!(
                "{name} opening requires {} offers, got {}",
                crate::opening::CHOOSE_COUNT,
                side.offers.len()
            ));
        }
        if let Some(ref shop) = side.shop {
            check_shop(shop, name)?;
        }
        if side.reinforce_chosen == Some(true) {
            return Err(format!("{name} round 0 reinforce_chosen cannot be true"));
        }
        Ok(StandaloneOpeningSide {
            tech_loadout: loadout,
            reactor_core: side.reactor_core,
            seed: side.seed,
            offers: side.offers,
            constructions: side.constructions,
            supply: side.supply,
            officers: side.officers,
            shop: side.shop,
            reinforce_chosen: side.reinforce_chosen,
        })
    };

    let [blue_loadout, red_loadout] = tech_loadout;
    Ok(StandaloneOpening {
        blue: build_side(wire.blue, blue_loadout, "blue")?,
        red: build_side(wire.red, red_loadout, "red")?,
    })
}

fn serialize_cards(ids: &[i32]) -> Result<JsonValue, String> {
    let names = ids
        .iter()
        .map(|&id| crate::names::card::name::<serde_json::Error>(id).map(str::to_owned))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(JsonValue::Array(
        names.into_iter().map(JsonValue::String).collect(),
    ))
}

fn serialize_loadout(
    loadout: &BTreeMap<i32, Vec<i32>>,
    format: MatchFormat,
) -> Result<JsonValue, String> {
    let serializer = serde_json::value::Serializer;
    match format {
        MatchFormat::Compact => crate::names::loadout::serialize_compact(loadout, serializer),
        MatchFormat::Expanded => crate::names::loadout::serialize(loadout, serializer),
    }
    .map_err(|e| e.to_string())
}

fn random_state_to_json(words: [u64; 4]) -> JsonValue {
    JsonValue::Array(words.into_iter().map(JsonValue::from).collect())
}

fn opening_side_to_json(
    side: &StandaloneOpeningSide,
    format: MatchFormat,
    runtime_fields: bool,
) -> Result<JsonValue, String> {
    let mut map = JsonMap::new();
    if let Some(loadout) = &side.tech_loadout {
        map.insert("tech_loadout".into(), serialize_loadout(loadout, format)?);
    }
    map.insert("reactor_core".into(), JsonValue::from(side.reactor_core));
    if let Some(supply) = side.supply {
        map.insert("supply".into(), JsonValue::from(supply));
    }
    if let Some(seed) = side.seed {
        map.insert("seed".into(), JsonValue::from(seed));
    }
    if let Some(chosen) = side.reinforce_chosen {
        map.insert("reinforce_chosen".into(), JsonValue::Bool(chosen));
    } else if runtime_fields {
        map.insert("reinforce_chosen".into(), JsonValue::Bool(false));
    }
    if let Some(shop) = &side.shop {
        map.insert(
            "shop".into(),
            serde_json::to_value(shop).map_err(|e| e.to_string())?,
        );
    }
    if !side.officers.is_empty() {
        let mut names = Vec::with_capacity(side.officers.len());
        for &id in &side.officers {
            let name =
                crate::names::officer_name(id).ok_or_else(|| format!("unknown officer ID {id}"))?;
            names.push(JsonValue::String(name.to_owned()));
        }
        map.insert("officers".into(), JsonValue::Array(names));
    }
    map.insert(
        "offers".into(),
        serde_json::to_value(&side.offers).map_err(|e| e.to_string())?,
    );
    map.insert(
        "constructions".into(),
        match format {
            MatchFormat::Compact => {
                serde_json::to_value(&side.constructions).map_err(|e| e.to_string())?
            }
            MatchFormat::Expanded => JsonValue::Array(crate::board::expanded_constructions_json(
                &side.constructions,
            )?),
        },
    );
    Ok(JsonValue::Object(map))
}

fn match_opening_side_to_json(side: &MatchSide, name: &str) -> Result<JsonValue, String> {
    let reactor_core = side
        .reactor_core
        .filter(|&c| c > 0)
        .ok_or_else(|| format!("{name} round 0 state has no positive reactor_core"))?;
    let mut map = JsonMap::new();
    map.insert("reactor_core".into(), JsonValue::from(reactor_core));
    if let Some(seed) = side.seed {
        map.insert("seed".into(), JsonValue::from(seed));
    }
    map.insert(
        "offers".into(),
        serde_json::to_value(&side.opening.offers).map_err(|e| e.to_string())?,
    );
    map.insert(
        "constructions".into(),
        JsonValue::Array(crate::board::expanded_constructions_json(
            &side.constructions,
        )?),
    );
    Ok(JsonValue::Object(map))
}

#[derive(Clone, Copy)]
enum ChoiceVisibility {
    HideOpening,
    Keep,
}

#[derive(Clone, Copy)]
struct SideJsonOpts {
    has_random_state: bool,
    explicit_shop: bool,
    runtime_fields: bool,
    choice_visibility: ChoiceVisibility,
    format: MatchFormat,
}

fn round_side_to_json(
    side: &SideState,
    tech_loadout: Option<&BTreeMap<i32, Vec<i32>>>,
    opts: SideJsonOpts,
) -> Result<JsonValue, String> {
    let mut serialized = serde_json::to_value(side).map_err(|e| e.to_string())?;
    let obj = serialized
        .as_object_mut()
        .ok_or("serialized SideState must be an object")?;
    if !opts.has_random_state {
        obj.remove("random_state");
    }
    if matches!(opts.choice_visibility, ChoiceVisibility::HideOpening) {
        obj.remove("reinforce_choice");
    }

    let mut ordered = JsonMap::new();
    if let Some(loadout) = tech_loadout {
        ordered.insert(
            "tech_loadout".into(),
            serialize_loadout(loadout, opts.format)?,
        );
    }
    if opts.runtime_fields || opts.explicit_shop {
        ordered.insert(
            "shop".into(),
            serde_json::json!({
                "buys_remaining": side.shop.buys_remaining,
                "unlocks_remaining": side.shop.unlocks_remaining,
                "contraptions_remaining": side.shop.contraptions_remaining,
            }),
        );
    }
    if opts.runtime_fields {
        ordered.insert(
            "reinforce_chosen".into(),
            JsonValue::Bool(side.reinforce_choice.is_some_and(|i| i >= 0)),
        );
    }
    for (k, v) in std::mem::take(obj) {
        ordered.insert(k, v);
    }
    if opts.format == MatchFormat::Expanded {
        crate::board::expand_round_side_json(&mut ordered, side)?;
    }
    Ok(JsonValue::Object(ordered))
}

/// Validates that unit IDs in an explicit `reinforce_pool` list appear sorted and valid
/// (helper check shared with tests).
#[allow(dead_code)]
fn _assert_sorted_cards(ids: &[i32]) -> bool {
    let set: BTreeSet<i32> = ids.iter().copied().collect();
    set.len() == ids.len() && ids.windows(2).all(|w| w[0] < w[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r#match::MatchFormat;
    use serde_json::json;

    #[test]
    fn minimal_standalone_state_and_u64_random_words_round_trip() {
        let large_word = u64::MAX - 42;
        let doc = json!({
            "kind": "state",
            "round": 1,
            "seed": 7,
            "random_state": [large_word, 2, 3, 4],
            "blue": {
                "reactor_core": 4500,
                "supply": 200,
                "next_index": {"unit": 1, "contraption": 0},
                "units": [{"name": "marksman", "index": 0, "value": 100, "position": {"x": 0, "y": -50}}]
            },
            "red": {
                "reactor_core": 4500,
                "supply": 200,
                "next_index": {"unit": 1, "contraption": 0},
                "units": [{"name": "arclight", "index": 0, "value": 100, "position": {"x": 0, "y": -100}}]
            }
        });
        let parsed = StandaloneState::from_json_value(&doc).unwrap();
        assert_eq!(parsed.round, 1);
        assert!(parsed.is_round_opening());
        let expanded = parsed.to_json_value(MatchFormat::Expanded, false).unwrap();
        assert_eq!(expanded["random_state"][0].as_u64(), Some(large_word));
        assert!(expanded["reinforce_pool"].is_array());
        assert!(expanded["blue"].get("tech_loadout").is_none());
        assert!(expanded["blue"].get("random_state").is_none());

        let reparsed = StandaloneState::from_json_value(&expanded).unwrap();
        assert_eq!(
            reparsed
                .to_json_value(MatchFormat::Expanded, false)
                .unwrap(),
            expanded
        );
    }

    #[test]
    fn zero_random_state_and_bad_shop_are_refused() {
        let zero = json!({
            "kind": "state",
            "round": 1,
            "random_state": [0, 0, 0, 0],
            "blue": {"reactor_core": 4500, "supply": 200, "next_index": {"unit": 0, "contraption": 0}, "units": []},
            "red": {"reactor_core": 4500, "supply": 200, "next_index": {"unit": 0, "contraption": 0}, "units": []}
        });
        assert!(StandaloneState::from_json_value(&zero).is_err());

        let bad_shop = json!({
            "kind": "state",
            "round": 1,
            "blue": {
                "shop": {"buys_remaining": -1, "unlocks_remaining": 1, "contraptions_remaining": 8},
                "reactor_core": 4500, "supply": 200, "next_index": {"unit": 0, "contraption": 0}, "units": []
            },
            "red": {"reactor_core": 4500, "supply": 200, "next_index": {"unit": 0, "contraption": 0}, "units": []}
        });
        assert!(StandaloneState::from_json_value(&bad_shop).is_err());
    }

    #[test]
    fn seedless_expanded_pool_and_opening_vs_intermediate_classification() {
        let first_card = crate::names::card::name::<serde_yaml::Error>(
            crate::reinforcement::initial_members(42).unwrap()[0],
        )
        .unwrap();
        let doc = json!({
            "kind": "state",
            "round": 3,
            "reinforce": true,
            "reinforce_pool": [first_card],
            "blue": {
                "shop": {"buys_remaining": 1, "unlocks_remaining": 1, "contraptions_remaining": 8},
                "reactor_core": 4500,
                "supply": 200,
                "next_index": {"unit": 0, "contraption": 0},
                "units": []
            },
            "red": {
                "reactor_core": 4500,
                "supply": 200,
                "next_index": {"unit": 0, "contraption": 0},
                "units": []
            }
        });
        let parsed = StandaloneState::from_json_value(&doc).unwrap();
        assert_eq!(parsed.seed, DEFAULT_SEED);
        assert!(!parsed.is_round_opening());
        let expanded = parsed.to_json_value(MatchFormat::Expanded, false).unwrap();
        let compact = parsed.to_json_value(MatchFormat::Compact, false).unwrap();
        assert_eq!(compact["seed"], DEFAULT_SEED);
        let round_trip = StandaloneState::from_json_value(&compact)
            .unwrap()
            .to_json_value(MatchFormat::Expanded, false)
            .unwrap();
        assert_eq!(round_trip["reinforce_pool"], expanded["reinforce_pool"]);
    }

    #[test]
    fn standalone_problem_json_materializes_tech_loadout() {
        let doc = json!({
            "kind": "state",
            "round": 1,
            "blue": {
                "reactor_core": 4500,
                "supply": 200,
                "next_index": {"unit": 0, "contraption": 0},
                "units": []
            },
            "red": {
                "reactor_core": 4500,
                "supply": 200,
                "next_index": {"unit": 0, "contraption": 0},
                "units": []
            }
        });
        let standalone = StandaloneState::from_json_value(&doc).unwrap();
        let prob = standalone.problem_json(Some("s:1")).unwrap();
        assert!(prob["state"]["blue"]["tech_loadout"].is_object());
        assert!(prob["state"]["red"]["tech_loadout"].is_object());
        assert_eq!(prob["state"]["seed"], DEFAULT_SEED);
    }

    #[test]
    fn match_problem_json_normalizes_history_actions() {
        let economy = crate::economy::Economy::embedded().unwrap();
        let pred = crate::opening::predict(economy, 7, 1001).unwrap();
        let opened_side = SideState {
            random_state: [1, 2, 3, 4],
            reactor_core: 4500,
            supply: 200,
            unlocked_units: vec![6],
            ..Default::default()
        };
        let r1_actions = crate::r#match::TurnActions {
            blue: vec![Action::BuyUnit {
                unit: 6,
                position: Some(crate::layout::Position { x: 0, y: -120 }),
                rotated: false,
            }],
            red: vec![],
        };
        let mut blue_end = opened_side.clone();
        blue_end.supply = 100;
        let r#match = Match {
            has_opening: true,
            game_build: crate::game_build().to_owned(),
            source: crate::r#match::Source::Simulator,
            reinforce: true,
            max_round: 13,
            map_id: 1001,
            seed: 7,
            blue: MatchSide {
                name: "B".into(),
                elo_score: None,
                opening: crate::r#match::Opening {
                    choose: Some(0),
                    offers: pred.deal.blue.clone(),
                },
                constructions: pred.constructions.blue.clone(),
                tech_loadout: economy.unit_technologies(),
                reactor_core: Some(4500),
                seed: Some(101),
            },
            red: MatchSide {
                name: "R".into(),
                elo_score: None,
                opening: crate::r#match::Opening {
                    choose: Some(0),
                    offers: pred.deal.red.clone(),
                },
                constructions: pred.constructions.red.clone(),
                tech_loadout: economy.unit_technologies(),
                reactor_core: Some(4500),
                seed: Some(102),
            },
            turns: vec![
                crate::r#match::Turn {
                    round: 1,
                    state: State {
                        random_state: [1, 2, 3, 4],
                        reinforce_pool: crate::r#match::PoolDelta::default(),
                        reinforce_offers: None,
                        blue: opened_side.clone(),
                        red: opened_side.clone(),
                    },
                    actions: r1_actions,
                },
                crate::r#match::Turn {
                    round: 2,
                    state: State {
                        random_state: [5, 6, 7, 8],
                        reinforce_pool: crate::r#match::PoolDelta::default(),
                        reinforce_offers: None,
                        blue: blue_end,
                        red: opened_side,
                    },
                    actions: crate::r#match::TurnActions::default(),
                },
            ],
            result: None,
        };
        let p2 = problem_json(&r#match, 2, None).unwrap();
        let h1_blue = p2["history"][1]["actions"]["blue"].as_array().unwrap();
        assert_eq!(h1_blue.len(), 2);
        assert_eq!(h1_blue[0]["type"], "buy_unit");
        assert!(h1_blue[0].get("position").is_none());
        assert_eq!(h1_blue[1]["type"], "move_unit");
    }

    #[test]
    fn rejects_later_round_joined_opening_round1_offers_bad_pool_delta_and_bad_offer_count() {
        // 1. round 2 with source: joined is not a clean round opening
        let r2_joined = json!({
            "kind": "state",
            "round": 2,
            "blue": {
                "reactor_core": 4500, "supply": 200,
                "next_index": {"unit": 1, "contraption": 0},
                "units": [{"name": "fang", "index": 0, "value": 100, "source": "joined", "position": {"x": 0, "y": -100}}]
            },
            "red": {"reactor_core": 4500, "supply": 200, "next_index": {"unit": 0, "contraption": 0}, "units": []}
        });
        assert!(
            !StandaloneState::from_json_value(&r2_joined)
                .unwrap()
                .is_round_opening()
        );

        // 2. round 1 cannot carry reinforce_offers
        let r1_offers = json!({
            "kind": "state",
            "round": 1,
            "reinforce_offers": [{"type": "supply", "name": "refund_200", "cost": -200}],
            "blue": {"reactor_core": 4500, "supply": 200, "next_index": {"unit": 0, "contraption": 0}, "units": []},
            "red": {"reactor_core": 4500, "supply": 200, "next_index": {"unit": 0, "contraption": 0}, "units": []}
        });
        assert!(StandaloneState::from_json_value(&r1_offers).is_err());

        // 3. invalid compact delta fails eagerly even before expanded serialization
        let first_card = crate::names::card::name::<serde_yaml::Error>(
            crate::reinforcement::initial_members(7).unwrap()[0],
        )
        .unwrap();
        let bad_delta = json!({
            "kind": "state",
            "seed": 7,
            "round": 2,
            "reinforce_pool": {"added": [first_card]},
            "blue": {"reactor_core": 4500, "supply": 200, "next_index": {"unit": 0, "contraption": 0}, "units": []},
            "red": {"reactor_core": 4500, "supply": 200, "next_index": {"unit": 0, "contraption": 0}, "units": []}
        });
        assert!(StandaloneState::from_json_value(&bad_delta).is_err());

        // 4. round 0 offers count must equal CHOOSE_COUNT (4)
        let r0_bad_offers = json!({
            "kind": "state",
            "round": 0,
            "blue": {"reactor_core": 4500, "offers": [], "constructions": []},
            "red": {"reactor_core": 4500, "offers": [], "constructions": []}
        });
        assert!(StandaloneState::from_json_value(&r0_bad_offers).is_err());
    }

    #[test]
    fn round_zero_standalone_opening_and_problem_json() {
        let economy = crate::economy::Economy::embedded().unwrap();
        let pred = crate::opening::predict(economy, 7, 1001).unwrap();
        let mut r#match = Match {
            has_opening: true,
            game_build: crate::game_build().to_owned(),
            source: crate::r#match::Source::Simulator,
            reinforce: true,
            max_round: 13,
            map_id: 1001,
            seed: 7,
            blue: MatchSide {
                name: "BotBlue".into(),
                elo_score: Some(2100),
                opening: crate::r#match::Opening {
                    choose: None,
                    offers: pred.deal.blue.clone(),
                },
                constructions: pred.constructions.blue.clone(),
                tech_loadout: economy.unit_technologies(),
                reactor_core: Some(4500),
                seed: Some(101),
            },
            red: MatchSide {
                name: "BotRed".into(),
                elo_score: Some(2000),
                opening: crate::r#match::Opening {
                    choose: None,
                    offers: pred.deal.red.clone(),
                },
                constructions: pred.constructions.red.clone(),
                tech_loadout: economy.unit_technologies(),
                reactor_core: Some(4500),
                seed: Some(102),
            },
            turns: Vec::new(),
            result: None,
        };
        let p0 = problem_json(&r#match, 0, Some("solve:r0")).unwrap();
        assert_eq!(p0["ref"], "solve:r0");
        assert!(p0.get("history").is_none());
        assert!(p0["state"]["reinforce_pool"].is_array());
        assert!(p0["state"]["blue"]["tech_loadout"].is_object());
        assert!(p0["state"]["blue"]["constructions"][0]["size"]["width"].is_i64());
        assert!(p0["state"]["blue"].get("name").is_none());
        assert!(p0["state"]["blue"].get("elo_score").is_none());

        let reparsed_r0 = StandaloneState::from_json_value(&p0["state"]).unwrap();
        assert!(reparsed_r0.is_round_opening());
        assert!(!reparsed_r0.is_terminal());

        r#match.reinforce = false;
        let p0_no_reinf = problem_json(&r#match, 0, None).unwrap();
        assert!(p0_no_reinf["state"].get("reinforce_pool").is_none());
        assert!(p0_no_reinf.get("ref").is_none());
    }

    #[test]
    fn standalone_expanded_state_emits_and_validates_towers_and_entity_sizes() {
        let doc = json!({
            "kind": "state",
            "round": 2,
            "seed": 7,
            "blue": {
                "reactor_core": 4500,
                "supply": 200,
                "tower_strengthen_levels": [0, 3],
                "next_index": {"unit": 2, "contraption": 2},
                "battle_skills": [
                    {"index": 0, "name": "field_recovery", "cooldown": 0, "release": {"target": {"unit": 1}}}
                ],
                "units": [
                    {"name": "phantom_ray", "index": 0, "value": 200, "position": {"x": 320, "y": 55}},
                    {"name": "crawler", "index": 1, "value": 100, "recovered": true}
                ],
                "constructions": [
                    {"name": "defensive_wall", "index": 0, "position": {"x": 140, "y": -55}}
                ],
                "contraptions": [
                    {"name": "missile", "index": 0, "position": {"x": 0, "y": -80}},
                    {"name": "interceptor", "index": 1, "position": {"x": 50, "y": -80}}
                ]
            },
            "red": {
                "reactor_core": 4500,
                "supply": 200,
                "next_index": {"unit": 0, "contraption": 0},
                "units": []
            }
        });
        let parsed = StandaloneState::from_json_value(&doc).unwrap();
        let expanded = parsed.to_json_value(MatchFormat::Expanded, false).unwrap();
        assert!(expanded["blue"].get("tower_strengthen_levels").is_none());
        assert_eq!(expanded["blue"]["towers"][1]["level"], 3);
        assert_eq!(
            expanded["blue"]["towers"][0]["size"],
            json!({"width": 20, "depth": 20})
        );
        // Phantom Ray 50x20 in right flank becomes 20x50
        assert_eq!(
            expanded["blue"]["units"][0]["size"],
            json!({"width": 20, "depth": 50})
        );
        assert!(expanded["blue"]["units"][1].get("size").is_none());
        assert_eq!(
            expanded["blue"]["constructions"][0]["size"],
            json!({"width": 60, "depth": 10})
        );
        assert!(expanded["blue"]["contraptions"][0].get("size").is_none());
        assert_eq!(
            expanded["blue"]["contraptions"][1]["size"],
            json!({"width": 30, "depth": 30})
        );

        // Round-trips cleanly through from_json_value -> compact -> expanded
        let reparsed = StandaloneState::from_json_value(&expanded).unwrap();
        assert_eq!(
            reparsed
                .to_json_value(MatchFormat::Expanded, false)
                .unwrap(),
            expanded
        );
        let compact = reparsed.to_json_value(MatchFormat::Compact, false).unwrap();
        assert_eq!(compact["blue"]["tower_strengthen_levels"], json!([0, 3]));
        assert!(compact["blue"].get("towers").is_none());
        assert!(compact["blue"]["units"][0].get("size").is_none());

        // Mismatched size is rejected
        let mut wrong_size = expanded.clone();
        wrong_size["blue"]["units"][0]["size"] = json!({"width": 50, "depth": 20});
        assert!(StandaloneState::from_json_value(&wrong_size).is_err());

        // Tombstone with size is rejected
        let mut tomb_size = expanded;
        tomb_size["blue"]["units"][1]["size"] = json!({"width": 60, "depth": 20});
        assert!(StandaloneState::from_json_value(&tomb_size).is_err());
    }

    #[test]
    fn standalone_omitted_tower_levels_default_to_zero() {
        let economy = crate::economy::Economy::embedded().unwrap();
        let doc = json!({
            "kind": "state",
            "round": 1,
            "seed": 7,
            "blue": {
                "reactor_core": 4500,
                "supply": 400,
                "next_index": {"unit": 0, "contraption": 0},
                "units": []
            },
            "red": {
                "reactor_core": 4500,
                "supply": 400,
                "next_index": {"unit": 0, "contraption": 0},
                "units": []
            }
        });
        let parsed = StandaloneState::from_json_value(&doc).unwrap();
        let state = match &parsed.body {
            StandaloneBody::Round(round) => &round.state,
            StandaloneBody::Opening(_) => panic!("expected round state"),
        };
        assert_eq!(state.blue.tower_strengthen_levels, vec![0, 0]);
        assert_eq!(state.red.tower_strengthen_levels, vec![0, 0]);
        let expanded = parsed.to_json_value(MatchFormat::Expanded, false).unwrap();
        let reparsed = StandaloneState::from_json_value(&expanded).unwrap();
        assert_eq!(parsed, reparsed);

        let after_compact = crate::transition::deployed(
            economy,
            &state.blue,
            &[crate::r#match::Action::StrengthenTower { tower: 0 }],
            false,
            None,
        )
        .unwrap();
        assert_eq!(after_compact.tower_strengthen_levels, vec![1, 0]);

        let mut explicit_empty = doc;
        explicit_empty["blue"]["tower_strengthen_levels"] = json!([]);
        assert!(StandaloneState::from_json_value(&explicit_empty).is_err());
    }

    #[test]
    fn chained_projection_keeps_tombstones_and_legacy_origin_pose() {
        let economy = crate::economy::Economy::embedded().unwrap();
        let initial = SideState {
            supply: 500,
            unlocked_units: vec![6],
            techs: vec![1606],
            next_index: crate::r#match::NextIndex {
                unit: 2,
                contraption: 0,
            },
            units: vec![
                serde_json::from_value(json!({
                    "name": "wasp",
                    "index": 0,
                    "value": 100,
                    "position": {"x": 0, "y": -100}
                }))
                .unwrap(),
                serde_json::from_value(json!({
                    "name": "marksman",
                    "index": 1,
                    "value": 100,
                    "source": "delivered",
                    "position": {"x": 40, "y": -100}
                }))
                .unwrap(),
            ],
            ..Default::default()
        };
        let opening = State {
            blue: initial.clone(),
            ..Default::default()
        };
        let mut after_step1 = crate::transition::deployed(
            economy,
            &initial,
            &[Action::MoveUnit {
                index: 0,
                position: crate::layout::Position { x: 40, y: -140 },
                rotated: false,
            }],
            false,
            None,
        )
        .unwrap();
        crate::project::record_legacy_pose(&initial, &mut after_step1);
        assert_eq!(
            after_step1.units[0].record_position,
            Some(crate::layout::Position { x: 0, y: -100 })
        );
        let mid_state = State {
            blue: after_step1.clone(),
            ..Default::default()
        };
        let mut after_step2 = crate::transition::deployed(
            economy,
            &after_step1,
            &[Action::MoveUnit {
                index: 0,
                position: crate::layout::Position { x: 80, y: -140 },
                rotated: false,
            }],
            false,
            None,
        )
        .unwrap();
        crate::project::record_legacy_pose(&after_step1, &mut after_step2);
        let end_state = State {
            blue: after_step2,
            ..Default::default()
        };
        let projected_direct = crate::project::project_standalone(
            &opening,
            &end_state,
            2,
            crate::game_build(),
            Some(1001),
            Some(7),
        )
        .unwrap();
        let projected_chained = crate::project::project_standalone(
            &mid_state,
            &end_state,
            2,
            crate::game_build(),
            Some(1001),
            Some(7),
        )
        .unwrap();
        assert_eq!(projected_direct, projected_chained);
    }
}
