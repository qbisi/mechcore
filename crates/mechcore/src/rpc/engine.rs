//! Stateless calculation engine and parallel worker pool shared by
//! `mechcore rpc`, `mechcore solve`, and `mechcore arena`.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use mechcore_document::{
    economy::Economy,
    r#match::{Action, Match, MatchFormat, Offers, OpeningOffer, SideState, State, TurnActions},
    project::InitializationHistory,
    standalone::{StandaloneBody, StandaloneOpeningSide, StandaloneState},
    transition,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    sync::{Notify, Semaphore},
    task::JoinSet,
};

use crate::{
    cli::Failure,
    kind::Kind,
    rpc::wire::{RequestId, RpcError},
    turn::Side,
};

/// Pre-parsed immutable standalone state context referenced by `context.state` or `context.ref`.
#[derive(Clone)]
pub(crate) struct StateContext {
    pub(crate) state: StandaloneState,
    economy: &'static Economy,
}

pub(crate) type RefStore = HashMap<String, Arc<StateContext>>;

impl StateContext {
    pub(crate) fn from_standalone(state: StandaloneState) -> Result<Self, Failure> {
        if let Some(ref build) = state.build {
            crate::r#match::warn_build(build);
        }
        let economy = Economy::embedded().map_err(Failure::failed)?;
        Ok(Self { state, economy })
    }

    pub(crate) fn from_json_state(value: &Value) -> Result<Self, Failure> {
        if !value.is_object() {
            return Err(Failure::usage("context.state must be a JSON object"));
        }
        let state = StandaloneState::from_json_value(value).map_err(Failure::refused)?;
        Self::from_standalone(state)
    }

    pub(crate) const fn round(&self) -> i32 {
        self.state.round
    }

    fn offers(&self) -> Option<&Offers> {
        match &self.state.body {
            StandaloneBody::Opening(_) => None,
            StandaloneBody::Round(r) => r.state.reinforce_offers.as_ref(),
        }
    }

    fn opening_side(&self, side: Side) -> Option<&StandaloneOpeningSide> {
        match &self.state.body {
            StandaloneBody::Opening(o) => Some(match side {
                Side::Blue => &o.blue,
                Side::Red => &o.red,
            }),
            StandaloneBody::Round(_) => None,
        }
    }

    fn opening_offers(&self, side: Side) -> Option<&[OpeningOffer]> {
        self.opening_side(side).map(|s| s.offers.as_slice())
    }

    fn opened_side(&self, side: Side) -> SideState {
        match &self.state.body {
            StandaloneBody::Opening(o) => {
                let s = match side {
                    Side::Blue => &o.blue,
                    Side::Red => &o.red,
                };
                let mut base = transition::before_opening(s.reactor_core, s.constructions.clone());
                if let Some(supply) = s.supply {
                    base.supply = supply;
                }
                base.officers.clone_from(&s.officers);
                if let Some(shop) = &s.shop {
                    base.shop = shop.clone();
                } else {
                    transition::open_allowances(&mut base);
                }
                base
            }
            StandaloneBody::Round(r) => match side {
                Side::Blue => r.state.blue.clone(),
                Side::Red => r.state.red.clone(),
            },
        }
    }

    fn round_state(&self) -> State {
        match &self.state.body {
            StandaloneBody::Opening(_) => State {
                blue: self.opened_side(Side::Blue),
                red: self.opened_side(Side::Red),
                ..State::default()
            },
            StandaloneBody::Round(r) => r.state.clone(),
        }
    }

    fn deployable(&self, opened: &State, side: Side, position: &SideState) -> Result<(), Failure> {
        if self.round() == 0 {
            return Ok(());
        }
        let mut deployed = opened.clone();
        match side {
            Side::Blue => deployed.blue = position.clone(),
            Side::Red => deployed.red = position.clone(),
        }
        let layout = mechcore_document::project::project_standalone(
            opened,
            &deployed,
            self.round(),
            self.state.effective_build(),
            self.state.map_id,
            Some(self.state.seed),
        )
        .map_err(Failure::refused)?;
        mechcore_document::compile::compile_placement_layout(layout)
            .map(|_| ())
            .map_err(Failure::refused)
    }

    fn placement_layout(
        &self,
        opened: &State,
        side: Side,
        position: &SideState,
    ) -> Result<mechcore_document::Layout, Failure> {
        let mut deployed = opened.clone();
        match side {
            Side::Blue => deployed.blue = position.clone(),
            Side::Red => deployed.red = position.clone(),
        }
        mechcore_document::project::project_standalone(
            opened,
            &deployed,
            self.round(),
            self.state.effective_build(),
            self.state.map_id,
            Some(self.state.seed),
        )
        .map_err(Failure::refused)
    }

    fn allowed(&self, side: Side, decision: &Action) -> Result<(), Failure> {
        let opening = self.round() == 0;
        match decision {
            Action::ChooseAdvanceTeam {
                index: offer,
                id,
                specialist,
            } => {
                if !opening {
                    return Err(Failure::refused(
                        "an opening is round zero's decision, and this round is not round zero",
                    ));
                }
                if self
                    .opening_side(side)
                    .is_some_and(StandaloneOpeningSide::has_chosen)
                {
                    return Err(Failure::refused(format!(
                        "{} has already chosen its round zero advance team",
                        side.name()
                    )));
                }
                let offers = self
                    .opening_offers(side)
                    .ok_or_else(|| Failure::refused("no opening offers present"))?;
                let dealt = usize::try_from(*offer)
                    .ok()
                    .and_then(|at| offers.get(at))
                    .ok_or_else(|| {
                        Failure::refused(format!(
                            "offer {offer} is not one of the {} {} was dealt",
                            offers.len(),
                            side.name()
                        ))
                    })?;
                if dealt.team != *id || dealt.specialist != *specialist {
                    let held = serde_json::to_string(dealt).unwrap_or_default();
                    return Err(Failure::refused(format!(
                        "offer {offer} holds {held}, and the decision names another"
                    )));
                }
                Ok(())
            }
            _ if opening => Err(Failure::refused(
                "round zero holds one decision, and it is the opening",
            )),
            Action::UpgradeTechnology { unit, tech } => {
                let Some(loadout) = self.state.tech_loadout(side.red()) else {
                    return Ok(());
                };
                let brought = loadout.get(unit).is_some_and(|techs| techs.contains(tech));
                if brought {
                    return Ok(());
                }
                Err(Failure::refused(format!(
                    "technology {tech} is not in the loadout {} brought for unit {unit}",
                    side.name()
                )))
            }
            _ => Ok(()),
        }
    }

    fn apply_action(
        &self,
        opened: &State,
        side: Side,
        before: &SideState,
        decision: &Action,
    ) -> Result<SideState, Failure> {
        self.allowed(side, decision)?;
        transition::check_operation(before, decision)
            .map_err(|unsettled| Failure::refused(unsettled.to_string()))?;
        let mut placement = mechcore_document::landing::placement(side.red());
        let mut after = transition::step_placing(
            self.economy,
            before,
            decision,
            self.offers(),
            &mut placement,
        )
        .map_err(|unsettled| Failure::refused(unsettled.to_string()))?;
        mechcore_document::project::record_legacy_pose(before, &mut after);
        self.deployable(opened, side, &after)?;
        transition::check_place(before, decision)
            .map_err(|unsettled| Failure::refused(unsettled.to_string()))?;
        Ok(after)
    }

    fn check_answer(
        &self,
        opened: &State,
        side: Side,
        decisions: &[Action],
        position: &SideState,
        either_conceded: bool,
    ) -> Result<(), Failure> {
        transition::check_reinforcement_finished(position.reinforce_choice, either_conceded)
            .map_err(|error| Failure::refused(error.to_string()))?;
        if self.round() == 0 {
            if self
                .opening_side(side)
                .is_some_and(StandaloneOpeningSide::has_chosen)
            {
                if !decisions.is_empty() {
                    return Err(Failure::refused(
                        "this side has already taken its round zero opening",
                    ));
                }
            } else {
                let [decision] = decisions else {
                    return Err(Failure::refused(
                        "an opening is one decision, and this side has taken another number of them",
                    ));
                };
                let Action::ChooseAdvanceTeam { .. } = decision else {
                    return Err(Failure::refused("round zero's decision is the opening"));
                };
            }
        } else {
            let layout = self.placement_layout(opened, side, position)?;
            mechcore_document::compile::compile_placement_layout(layout)
                .map_err(Failure::refused)?;
        }
        Ok(())
    }

    /// Parses and steps `raw_actions` for `side` without yet running cross-side
    /// end-of-sequence checks.
    pub(crate) fn step_side_actions(
        &self,
        operation: &str,
        side: Side,
        raw_actions: &[Value],
        cancel: &AtomicBool,
    ) -> Result<(Vec<Action>, SideState, Vec<String>), RpcError> {
        let round = self.round();
        let opened_state = self.round_state();
        let opened_side = self.opened_side(side);
        let mut position = opened_side.clone();
        let mut actions = Vec::with_capacity(raw_actions.len());
        let mut history = InitializationHistory::default();

        for (index, value) in raw_actions.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(RpcError::cancelled(operation));
            }
            let step_err = |failure: Failure| {
                RpcError::from_failure(
                    operation,
                    failure.with_details(json!({"side": side, "action_index": index})),
                )
            };
            let action = Action::read_in_round(value, round)
                .map_err(|e| step_err(Failure::usage(e.to_string())))?;
            if actions.last() == Some(&Action::Concede) {
                return Err(step_err(Failure::refused("concede ends the sequence")));
            }
            let next = self
                .apply_action(&opened_state, side, &position, &action)
                .map_err(step_err)?;
            if round > 0 {
                history.record(&position, &next, &action);
            }
            position = next;
            actions.push(action);
        }

        let warnings = if round > 0 {
            history
                .warnings(&opened_side, &position)
                .into_iter()
                .map(|w| format!("round {round} {}: {w}", side.name()))
                .collect()
        } else {
            Vec::new()
        };

        Ok((actions, position, warnings))
    }

    /// Verifies end-of-sequence completeness for `side` given whether either side conceded.
    pub(crate) fn finish_side_actions(
        &self,
        operation: &str,
        side: Side,
        actions: &[Action],
        position: &SideState,
        either_conceded: bool,
    ) -> Result<(), RpcError> {
        let opened_state = self.round_state();
        self.check_answer(&opened_state, side, actions, position, either_conceded)
            .map_err(|e| RpcError::from_failure(operation, e.with_details(json!({"side": side}))))
    }

    /// Parses, steps, and validates `raw_actions` for `side` independently.
    pub(crate) fn validate_side_actions(
        &self,
        operation: &str,
        side: Side,
        raw_actions: &[Value],
    ) -> Result<(Vec<Action>, SideState, Vec<String>), RpcError> {
        let never_cancel = AtomicBool::new(false);
        let (actions, position, warnings) =
            self.step_side_actions(operation, side, raw_actions, &never_cancel)?;
        let conceded = actions.contains(&Action::Concede);
        self.finish_side_actions(operation, side, &actions, &position, conceded)?;
        Ok((actions, position, warnings))
    }

    fn with_updated_sides(
        &self,
        blue: Option<SideState>,
        red: Option<SideState>,
        explicit_shop: [bool; 2],
    ) -> StandaloneState {
        let mut next = self.state.clone();
        match &mut next.body {
            StandaloneBody::Opening(opening) => {
                for (side_state, dst) in [(blue, &mut opening.blue), (red, &mut opening.red)] {
                    if let Some(s) = side_state {
                        dst.reactor_core = s.reactor_core;
                        dst.supply = Some(s.supply);
                        dst.officers = s.officers;
                        dst.constructions = s.constructions;
                        dst.shop = Some(s.shop);
                        dst.reinforce_chosen = Some(false);
                    }
                }
            }
            StandaloneBody::Round(round) => {
                if let Some(b) = blue {
                    round.state.blue = b;
                }
                if let Some(r) = red {
                    round.state.red = r;
                }
                round.explicit_shop = explicit_shop;
            }
        }
        next
    }
}

/// Resolves `{"state": <object>}` or `{"ref": "<string>"}`, enforcing strict exclusivity.
fn resolve_context(
    context: Value,
    refs: &RefStore,
    operation: &str,
) -> Result<Arc<StateContext>, RpcError> {
    let Value::Object(mut obj) = context else {
        return Err(RpcError::invalid_params(
            operation,
            Failure::usage("context must be a JSON object"),
        ));
    };
    let state_val = obj.remove("state");
    let ref_val = obj.remove("ref");
    if let Some((unknown, _)) = obj.iter().next() {
        return Err(RpcError::invalid_params(
            operation,
            Failure::usage(format!("unknown field {unknown:?} in context")),
        ));
    }
    match (state_val, ref_val) {
        (Some(state_obj), None) => {
            if !state_obj.is_object() {
                return Err(RpcError::invalid_params(
                    operation,
                    Failure::usage(
                        "context.state must be a JSON object (YAML strings are rejected; use convert)",
                    ),
                ));
            }
            StateContext::from_json_state(&state_obj)
                .map(Arc::new)
                .map_err(|f| RpcError::from_failure(operation, f))
        }
        (None, Some(ref_val)) => {
            let Some(ref_str) = ref_val.as_str() else {
                return Err(RpcError::invalid_params(
                    operation,
                    Failure::usage("context.ref must be a string"),
                ));
            };
            refs.get(ref_str).cloned().ok_or_else(|| {
                RpcError::from_failure(
                    operation,
                    Failure::refused(format!(
                        "context ref {ref_str:?} does not exist or has expired"
                    )),
                )
            })
        }
        (Some(_), Some(_)) => Err(RpcError::invalid_params(
            operation,
            Failure::usage("context must provide exactly one of \"state\" or \"ref\", not both"),
        )),
        (None, None) => Err(RpcError::invalid_params(
            operation,
            Failure::usage("context must provide \"state\" or \"ref\""),
        )),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConvertParams {
    input: Value,
    to: String,
    #[serde(default)]
    round: Option<i32>,
    #[serde(default)]
    apply_actions: Option<bool>,
    #[serde(default = "default_true")]
    expanded: bool,
}

const fn default_true() -> bool {
    true
}

/// Executes stateless `convert` (`match` or `state` -> standalone JSON `state`).
pub(crate) fn run_convert(params: Option<Value>, cancel: &AtomicBool) -> Result<Value, RpcError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(RpcError::cancelled("convert"));
    }
    let Some(params_val) = params else {
        return Err(RpcError::invalid_params(
            "convert",
            Failure::usage("convert requires params"),
        ));
    };
    let params: ConvertParams = serde_json::from_value(params_val)
        .map_err(|e| RpcError::invalid_params("convert", Failure::usage(e.to_string())))?;
    if params.to != "state" {
        return Err(RpcError::invalid_params(
            "convert",
            Failure::usage(format!(
                "unsupported convert target {:?}; expected \"state\"",
                params.to
            )),
        ));
    }
    let format = if params.expanded {
        MatchFormat::Expanded
    } else {
        MatchFormat::Compact
    };

    let (source_kind, converted_state) = match &params.input {
        Value::Object(obj) => {
            if obj.get("kind").and_then(Value::as_str) != Some("state") {
                return Err(RpcError::from_failure(
                    "convert",
                    Failure::refused("convert only accepts kind: match or kind: state"),
                ));
            }
            reject_match_only_opts(params.round, params.apply_actions)?;
            let standalone = StandaloneState::from_json_value(&params.input)
                .map_err(|e| RpcError::from_failure("convert", Failure::refused(e)))?;
            ("state", convert_standalone_state(&standalone, format)?)
        }
        Value::String(text) => {
            let bytes = text.as_bytes();
            let kind = Kind::of(bytes)
                .map_err(|e| RpcError::from_failure("convert", Failure::refused(e)))?;
            match kind {
                Kind::State => {
                    reject_match_only_opts(params.round, params.apply_actions)?;
                    let standalone = StandaloneState::from_yaml_bytes(bytes)
                        .map_err(|e| RpcError::from_failure("convert", Failure::refused(e)))?;
                    ("state", convert_standalone_state(&standalone, format)?)
                }
                Kind::Match => {
                    let json_state = convert_match_to_state(
                        bytes,
                        params.round,
                        params.apply_actions.unwrap_or(false),
                        format,
                        cancel,
                    )?;
                    ("match", json_state)
                }
                other => {
                    return Err(RpcError::from_failure(
                        "convert",
                        Failure::refused(format!(
                            "convert does not accept {} documents; expected match or state",
                            other.name()
                        )),
                    ));
                }
            }
        }
        _ => {
            return Err(RpcError::invalid_params(
                "convert",
                Failure::usage("convert input must be a string or JSON object"),
            ));
        }
    };

    Ok(json!({
        "source_kind": source_kind,
        "state": converted_state,
    }))
}

fn reject_match_only_opts(round: Option<i32>, apply_actions: Option<bool>) -> Result<(), RpcError> {
    if round.is_some() || apply_actions.is_some() {
        return Err(RpcError::invalid_params(
            "convert",
            Failure::usage("round and apply_actions only apply when input is kind: match"),
        ));
    }
    Ok(())
}

fn convert_standalone_state(
    standalone: &StandaloneState,
    format: MatchFormat,
) -> Result<Value, RpcError> {
    if let Some(ref build) = standalone.build {
        crate::r#match::warn_build(build);
    }
    standalone
        .to_json_value(format, false)
        .map_err(|e| RpcError::from_failure("convert", Failure::refused(e)))
}

fn convert_match_to_state(
    bytes: &[u8],
    requested_round: Option<i32>,
    apply_actions: bool,
    format: MatchFormat,
    cancel: &AtomicBool,
) -> Result<Value, RpcError> {
    let segments = mechcore_document::r#match::segments(bytes)
        .map_err(|e| RpcError::from_failure("convert", Failure::refused(e)))?
        .ok_or_else(|| {
            RpcError::from_failure("convert", Failure::refused("input is not a match document"))
        })?;
    if cancel.load(Ordering::Relaxed) {
        return Err(RpcError::cancelled("convert"));
    }
    let game = mechcore_document::r#match::read(bytes)
        .map_err(|e| RpcError::from_failure("convert", Failure::refused(e)))?
        .ok_or_else(|| {
            RpcError::from_failure("convert", Failure::refused("input is not a match document"))
        })?;
    if cancel.load(Ordering::Relaxed) {
        return Err(RpcError::cancelled("convert"));
    }
    crate::r#match::warn_build(&game.game_build);

    let target_round = match requested_round {
        Some(r) => r,
        None => {
            if let Some(last) = segments.rounds.last() {
                last.round
            } else if segments.opening_state.is_some() {
                0
            } else {
                return Err(RpcError::from_failure(
                    "convert",
                    Failure::refused("match stream contains no state segment"),
                ));
            }
        }
    };

    let standalone = StandaloneState::from_match_round(&game, target_round)
        .map_err(|e| RpcError::from_failure("convert", Failure::refused(e)))?;

    if !apply_actions {
        return standalone
            .to_json_value(format, false)
            .map_err(|e| RpcError::from_failure("convert", Failure::refused(e)));
    }

    let has_actions = if target_round == 0 {
        segments.opening.is_some()
    } else {
        segments
            .rounds
            .iter()
            .find(|r| r.round == target_round)
            .is_some_and(|r| r.actions.is_some())
    };
    if !has_actions {
        return Err(RpcError::from_failure(
            "convert",
            Failure::refused(format!(
                "round {target_round} has no action segment to apply"
            )),
        ));
    }
    if standalone.is_terminal() {
        return Err(RpcError::from_failure(
            "convert",
            Failure::refused(format!(
                "round {target_round} is terminal and has no actions"
            )),
        ));
    }

    apply_match_round_actions(&game, target_round, standalone, format, cancel)
}

fn apply_match_round_actions(
    game: &Match,
    target_round: i32,
    standalone: StandaloneState,
    format: MatchFormat,
    cancel: &AtomicBool,
) -> Result<Value, RpcError> {
    let turn_actions = extract_match_round_actions(game, target_round)?;
    let ctx = StateContext::from_standalone(standalone)
        .map_err(|f| RpcError::from_failure("convert", f))?;
    let blue_raw: Vec<Value> = turn_actions
        .blue
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()
        .map_err(|e| RpcError::from_failure("convert", Failure::failed(e.to_string())))?;
    let red_raw: Vec<Value> = turn_actions
        .red
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()
        .map_err(|e| RpcError::from_failure("convert", Failure::failed(e.to_string())))?;

    let (blue_actions, blue_state, _) =
        ctx.step_side_actions("convert", Side::Blue, &blue_raw, cancel)?;
    let (red_actions, red_state, _) =
        ctx.step_side_actions("convert", Side::Red, &red_raw, cancel)?;
    let blue_conceded = blue_actions.contains(&Action::Concede);
    let red_conceded = red_actions.contains(&Action::Concede);
    if blue_conceded && red_conceded {
        return Err(RpcError::from_failure(
            "convert",
            Failure::refused("both sides conceded in the same round"),
        ));
    }
    let either_conceded = blue_conceded || red_conceded;
    ctx.finish_side_actions(
        "convert",
        Side::Blue,
        &blue_actions,
        &blue_state,
        either_conceded,
    )?;
    ctx.finish_side_actions(
        "convert",
        Side::Red,
        &red_actions,
        &red_state,
        either_conceded,
    )?;

    let deployed = ctx.with_updated_sides(Some(blue_state), Some(red_state), [true, true]);
    deployed
        .to_json_value(format, false)
        .map_err(|e| RpcError::from_failure("convert", Failure::refused(e)))
}

fn extract_match_round_actions(game: &Match, target_round: i32) -> Result<TurnActions, RpcError> {
    if target_round == 0 {
        return Ok(TurnActions {
            blue: game.blue.opening.action().into_iter().collect(),
            red: game.red.opening.action().into_iter().collect(),
        });
    }
    game.turns
        .iter()
        .find(|t| t.round == target_round)
        .map(|t| t.actions.clone())
        .ok_or_else(|| {
            RpcError::from_failure(
                "convert",
                Failure::refused(format!("match has no round {target_round}")),
            )
        })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActParams {
    context: Value,
    #[serde(default)]
    actions: Option<ActActions>,
    #[serde(default = "default_true")]
    expanded: bool,
}

fn deserialize_present_actions<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<Value>>, D::Error> {
    Vec::<Value>::deserialize(deserializer).map(Some)
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ActActions {
    #[serde(default, deserialize_with = "deserialize_present_actions")]
    blue: Option<Vec<Value>>,
    #[serde(default, deserialize_with = "deserialize_present_actions")]
    red: Option<Vec<Value>>,
}

/// Executes stateless `act` dry-run validation & deployment transition.
pub(crate) fn run_act(
    params: Option<Value>,
    refs: &RefStore,
    cancel: &AtomicBool,
) -> Result<Value, RpcError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(RpcError::cancelled("act"));
    }
    let Some(params_val) = params else {
        return Err(RpcError::invalid_params(
            "act",
            Failure::usage("act requires params"),
        ));
    };
    let params: ActParams = serde_json::from_value(params_val)
        .map_err(|e| RpcError::invalid_params("act", Failure::usage(e.to_string())))?;
    let ctx = resolve_context(params.context, refs, "act")?;
    if cancel.load(Ordering::Relaxed) {
        return Err(RpcError::cancelled("act"));
    }
    if ctx.state.is_terminal() {
        return Err(RpcError::from_failure(
            "act",
            Failure::refused("state is already terminal (reactor_core <= 0)"),
        ));
    }
    let actions = params.actions.unwrap_or_default();

    let (blue_state, red_state) = match (actions.blue.as_deref(), actions.red.as_deref()) {
        (Some(blue_raw), Some(red_raw)) => {
            let (blue_actions, blue_state, _) =
                ctx.step_side_actions("act", Side::Blue, blue_raw, cancel)?;
            let (red_actions, red_state, _) =
                ctx.step_side_actions("act", Side::Red, red_raw, cancel)?;
            let blue_conceded = blue_actions.contains(&Action::Concede);
            let red_conceded = red_actions.contains(&Action::Concede);
            if blue_conceded && red_conceded {
                return Err(RpcError::from_failure(
                    "act",
                    Failure::refused("a round ends on its first concession"),
                ));
            }
            let either_conceded = blue_conceded || red_conceded;
            ctx.finish_side_actions(
                "act",
                Side::Blue,
                &blue_actions,
                &blue_state,
                either_conceded,
            )?;
            ctx.finish_side_actions("act", Side::Red, &red_actions, &red_state, either_conceded)?;
            (Some(blue_state), Some(red_state))
        }
        (Some(blue_raw), None) => {
            let (actions, blue_state, _) =
                ctx.step_side_actions("act", Side::Blue, blue_raw, cancel)?;
            let conceded = actions.contains(&Action::Concede);
            ctx.finish_side_actions("act", Side::Blue, &actions, &blue_state, conceded)?;
            (Some(blue_state), None)
        }
        (None, Some(red_raw)) => {
            let (actions, red_state, _) =
                ctx.step_side_actions("act", Side::Red, red_raw, cancel)?;
            let conceded = actions.contains(&Action::Concede);
            ctx.finish_side_actions("act", Side::Red, &actions, &red_state, conceded)?;
            (None, Some(red_state))
        }
        (None, None) => (None, None),
    };

    let next = ctx.with_updated_sides(blue_state, red_state, [true, true]);
    let format = if params.expanded {
        MatchFormat::Expanded
    } else {
        MatchFormat::Compact
    };
    let state_json = next
        .to_json_value(format, true)
        .map_err(|e| RpcError::from_failure("act", Failure::refused(e)))?;

    Ok(json!({ "state": state_json }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SimParams {
    context: Value,
    #[serde(default)]
    actions: Option<SimActions>,
    #[serde(default)]
    seed: Option<i32>,
    #[serde(default)]
    until: Option<Value>,
    #[serde(default)]
    collect: Option<Value>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SimActions {
    blue: Vec<Value>,
    red: Vec<Value>,
}

/// Executes stateless `simulate` (including tick-0 `preview` and limited battle observation).
pub(crate) fn run_simulate(
    params: Option<Value>,
    refs: &RefStore,
    cancel: &AtomicBool,
) -> Result<Value, RpcError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(RpcError::cancelled("simulate"));
    }
    let Some(params_val) = params else {
        return Err(RpcError::invalid_params(
            "simulate",
            Failure::usage("simulate requires params"),
        ));
    };
    let params: SimParams = serde_json::from_value(params_val)
        .map_err(|e| RpcError::invalid_params("simulate", Failure::usage(e.to_string())))?;

    if params.seed == Some(0) {
        return Err(RpcError::invalid_params(
            "simulate",
            Failure::usage("seed 0 is not valid for reproducible simulation"),
        ));
    }
    let is_tick_zero = matches!(
        params.until.as_ref(),
        Some(Value::Object(map)) if map.get("tick").and_then(Value::as_u64) == Some(0)
    );
    let plan = mechcore_simulation::ObservationPlan::parse(params.until, params.collect)
        .map_err(|e| RpcError::invalid_params("simulate", Failure::usage(e)))?;

    let ctx = resolve_context(params.context, refs, "simulate")?;
    if cancel.load(Ordering::Relaxed) {
        return Err(RpcError::cancelled("simulate"));
    }
    if ctx.state.is_terminal() {
        return Err(RpcError::from_failure(
            "simulate",
            Failure::refused("state is already terminal (reactor_core <= 0)"),
        ));
    }
    let actions = params.actions.unwrap_or_default();

    let (blue_actions, blue_state, mut warnings) =
        ctx.step_side_actions("simulate", Side::Blue, &actions.blue, cancel)?;
    let (red_actions, red_state, red_warnings) =
        ctx.step_side_actions("simulate", Side::Red, &actions.red, cancel)?;
    warnings.extend(red_warnings);

    let blue_conceded = blue_actions.contains(&Action::Concede);
    let red_conceded = red_actions.contains(&Action::Concede);
    if blue_conceded && red_conceded {
        return Err(RpcError::from_failure(
            "simulate",
            Failure::refused("a round ends on its first concession"),
        ));
    }
    let either_conceded = blue_conceded || red_conceded;
    ctx.finish_side_actions(
        "simulate",
        Side::Blue,
        &blue_actions,
        &blue_state,
        either_conceded,
    )?;
    ctx.finish_side_actions(
        "simulate",
        Side::Red,
        &red_actions,
        &red_state,
        either_conceded,
    )?;

    if ctx.round() == 0 || either_conceded {
        return Ok(json!({
            "tick": 0,
            "stop": "no_fight",
            "settled": false,
            "collect": {},
        }));
    }

    let seed = params.seed.unwrap_or(ctx.state.seed);

    let opening = ctx.round_state();
    let deployed = State {
        blue: blue_state,
        red: red_state,
        ..opening.clone()
    };
    let layout = mechcore_document::project::project_standalone(
        &opening,
        &deployed,
        ctx.round(),
        ctx.state.effective_build(),
        ctx.state.map_id,
        Some(seed),
    )
    .map_err(|e| RpcError::from_failure("simulate", Failure::refused(e)))?;

    execute_simulation(layout, is_tick_zero, plan, &warnings, cancel)
}

fn execute_simulation(
    layout: mechcore_document::Layout,
    is_tick_zero: bool,
    plan: mechcore_simulation::ObservationPlan,
    warnings: &[String],
    cancel: &AtomicBool,
) -> Result<Value, RpcError> {
    if is_tick_zero {
        mechcore_document::compile::compile_placement_layout(layout.clone())
    } else {
        mechcore_document::compile_layout(layout.clone())
    }
    .map_err(|e| RpcError::from_failure("simulate", Failure::refused(e)))?;

    let settlement_names = plan.settlement_names();
    let mut observation = plan.run(&layout, cancel).map_err(|e| {
        if e.is_cancelled() {
            RpcError::cancelled("simulate")
        } else {
            RpcError::from_failure(
                "simulate",
                Failure::unsimulated(&e).with_details(json!({"status": "unresolved"})),
            )
        }
    })?;

    if let Some(evidence) = observation.evidence {
        let fight = crate::outcome::observed(layout, evidence, observation.tick)
            .map_err(|f| RpcError::from_failure("simulate", f))?;
        for name in settlement_names {
            observation
                .values
                .insert(name, json!({"blue": fight.blue, "red": fight.red}));
        }
    }

    let mut response = json!({
        "tick": observation.tick,
        "stop": observation.stop,
        "settled": observation.settled,
        "collect": observation.values,
    });
    if !warnings.is_empty() {
        response["warnings"] = json!(warnings);
    }
    Ok(response)
}

/// Queued blocking RPC job dispatched onto [`ComputePool`].
pub(crate) enum ComputeJob {
    Convert {
        params: Option<Value>,
    },
    Act {
        params: Option<Value>,
        refs: Arc<RefStore>,
    },
    Simulate {
        params: Option<Value>,
        refs: Arc<RefStore>,
    },
}

impl ComputeJob {
    const fn operation(&self) -> &'static str {
        match self {
            Self::Convert { .. } => "convert",
            Self::Act { .. } => "act",
            Self::Simulate { .. } => "simulate",
        }
    }

    fn execute(self, cancel: &AtomicBool) -> Result<Value, RpcError> {
        match self {
            Self::Convert { params } => run_convert(params, cancel),
            Self::Act { params, refs } => run_act(params, &refs, cancel),
            Self::Simulate { params, refs } => run_simulate(params, &refs, cancel),
        }
    }
}

struct QueuedTask {
    id: RequestId,
    job: ComputeJob,
    cancel: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
}

struct InFlight {
    cancel: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
    queued: bool,
    operation: &'static str,
}

/// Bounded FIFO + blocking-threadpool scheduler for concurrent `convert`/`act`/`simulate`.
pub(crate) struct ComputePool {
    parallel: usize,
    slots: Arc<Semaphore>,
    queue: VecDeque<QueuedTask>,
    running: JoinSet<(RequestId, Result<Value, RpcError>)>,
    in_flight: HashMap<RequestId, InFlight>,
}

impl ComputePool {
    pub(crate) fn new(parallel: usize) -> Self {
        let cap = parallel.max(1);
        Self::with_slots(cap, Arc::new(Semaphore::new(cap)))
    }

    pub(crate) fn with_slots(parallel: usize, slots: Arc<Semaphore>) -> Self {
        Self {
            parallel: parallel.max(1),
            slots,
            queue: VecDeque::new(),
            running: JoinSet::new(),
            in_flight: HashMap::new(),
        }
    }

    pub(crate) fn is_idle(&self) -> bool {
        self.queue.is_empty() && self.running.is_empty()
    }

    pub(crate) fn is_running_empty(&self) -> bool {
        self.running.is_empty()
    }

    pub(crate) fn contains_id(&self, id: &RequestId) -> bool {
        self.in_flight.contains_key(id)
    }

    /// Enqueues a compute job if `id` is not already in flight.
    /// Returns `Some(error_response_err)` synchronously on duplicate in-flight ID.
    pub(crate) fn enqueue(&mut self, id: RequestId, job: ComputeJob) -> Option<RpcError> {
        let operation = job.operation();
        if self.in_flight.contains_key(&id) {
            return Some(RpcError::invalid_params(
                operation,
                Failure::usage("duplicate in-flight request id"),
            ));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_notify = Arc::new(Notify::new());
        self.in_flight.insert(
            id.clone(),
            InFlight {
                cancel: Arc::clone(&cancel),
                cancel_notify: Arc::clone(&cancel_notify),
                queued: true,
                operation,
            },
        );
        self.queue.push_back(QueuedTask {
            id,
            job,
            cancel,
            cancel_notify,
        });
        self.pump();
        None
    }

    /// Cancels `id` if queued or running; ignores unknown/completed IDs.
    /// If `id` was still in the FIFO queue, removes it and immediately returns
    /// `(id, Err(Cancelled))` so the caller writes `-32800` right away.
    pub(crate) fn cancel(&mut self, id: &RequestId) -> Option<(RequestId, RpcError)> {
        let entry = self.in_flight.get_mut(id)?;
        entry.cancel.store(true, Ordering::Relaxed);
        entry.cancel_notify.notify_one();
        if entry.queued {
            let operation = entry.operation;
            self.in_flight.remove(id);
            if let Some(pos) = self.queue.iter().position(|t| &t.id == id) {
                let removed = self.queue.remove(pos).expect("located item");
                return Some((removed.id, RpcError::cancelled(operation)));
            }
        }
        None
    }

    /// Cancels all queued and running tasks and drains tracking structures.
    pub(crate) fn cancel_all(&mut self) {
        self.queue.clear();
        for (_, entry) in self.in_flight.drain() {
            entry.cancel.store(true, Ordering::Relaxed);
            entry.cancel_notify.notify_one();
        }
    }

    /// Awaits the next finished worker job and pumps queued work into the freed slot.
    pub(crate) async fn next_completed(
        &mut self,
    ) -> Option<Result<(RequestId, Result<Value, RpcError>), tokio::task::JoinError>> {
        let joined = self.running.join_next().await?;
        if let Ok((ref id, _)) = joined {
            self.in_flight.remove(id);
        }
        self.pump();
        Some(joined)
    }

    fn pump(&mut self) {
        while self.running.len() < self.parallel {
            let Some(task) = self.queue.pop_front() else {
                break;
            };
            if let Some(entry) = self.in_flight.get_mut(&task.id) {
                entry.queued = false;
            }
            let id = task.id;
            let cancel = task.cancel;
            let notify = task.cancel_notify;
            let job = task.job;
            let operation = job.operation();
            let slots = Arc::clone(&self.slots);
            self.running.spawn(async move {
                if cancel.load(Ordering::Relaxed) {
                    return (id, Err(RpcError::cancelled(operation)));
                }
                let permit = tokio::select! {
                    biased;
                    () = notify.notified() => {
                        return (id, Err(RpcError::cancelled(operation)));
                    }
                    acquired = Arc::clone(&slots).acquire_owned() => match acquired {
                        Ok(permit) => permit,
                        Err(_) => return (id, Err(RpcError::cancelled(operation))),
                    }
                };
                if cancel.load(Ordering::Relaxed) {
                    return (id, Err(RpcError::cancelled(operation)));
                }
                let cancel_exec = Arc::clone(&cancel);
                let result = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    let res = job.execute(&cancel_exec);
                    if cancel_exec.load(Ordering::Relaxed) {
                        Err(RpcError::cancelled(operation))
                    } else {
                        res
                    }
                })
                .await;
                match result {
                    Ok(res) => (id, res),
                    Err(e) => (
                        id,
                        Err(RpcError::from_failure(
                            operation,
                            Failure::failed(e.to_string()),
                        )),
                    ),
                }
            });
        }
    }
}
