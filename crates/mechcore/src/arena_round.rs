//! The arena's immutable round opening and complete, side-local answers.
use super::{
    Action, Draw, Failure, Game, MatchOptions, Side, SideState, State, TurnActions, Value,
    dealt_game,
};

impl Game {
    pub(crate) fn arena_new(
        seed: Option<i32>,
        map: Option<i32>,
        options: MatchOptions,
    ) -> Result<Self, Failure> {
        let mut draw = seed.map_or_else(Draw::new, |seed| Draw(u64::from(seed.unsigned_abs())));
        let seed = seed.unwrap_or_else(|| draw.seed().max(1));
        if seed <= 0 {
            return Err(Failure::refused(
                "an arena battle requires a positive match seed",
            ));
        }
        let mut game = dealt_game(
            std::path::Path::new(""),
            Some(seed),
            map,
            None,
            None,
            None,
            options,
            draw,
        )?;
        game.turn.side_mut(Side::Red).given = true;
        Ok(game)
    }

    pub(crate) fn arena_load(
        path: &std::path::Path,
        round: i32,
        max_round: Option<i32>,
        base_core: Option<i32>,
    ) -> Result<Self, Failure> {
        let mut document = super::read_match(path)?;
        if let Some(max_round) = max_round {
            if max_round < 1 {
                return Err(Failure::refused("match max_round must be >= 1"));
            }
            document.max_round = max_round;
        }
        if let Some(base_core) = base_core {
            if base_core <= 0 {
                return Err(Failure::refused("match base_core must be > 0"));
            }
            if round != 0 || !document.has_opening {
                return Err(Failure::refused(
                    "--base-core only applies when loading round 0",
                ));
            }
            document.blue.reactor_core = Some(base_core);
            document.red.reactor_core = Some(base_core);
        }
        super::warn_build(&document.game_build);
        super::fork_position(std::path::PathBuf::new(), document, None, Some(round))
    }

    pub(crate) fn set_arena_names(&mut self, [blue, red]: [String; 2]) {
        self.r#match.blue.name = blue;
        self.r#match.red.name = red;
        // Historical ratings belong to the recorded players.
        self.r#match.blue.elo_score = None;
        self.r#match.red.elo_score = None;
    }

    pub(crate) fn document(&self) -> Result<String, Failure> {
        mechcore_document::r#match::canonical_yaml(&self.r#match).map_err(Failure::failed)
    }

    pub(crate) fn problem_yaml(&self) -> Result<String, Failure> {
        mechcore_document::r#match::round_problem_yaml(&self.r#match).map_err(Failure::failed)
    }

    pub(crate) fn answer_actions(
        &self,
        side: Side,
        input: &[Value],
    ) -> Result<(Vec<Action>, SideState), Failure> {
        let mut position = self.opened(side);
        let mut actions = Vec::with_capacity(input.len());
        for (index, value) in input.iter().enumerate() {
            let context = |failure: Failure| {
                failure.with_details(serde_json::json!({"side":side,"action_index":index}))
            };
            let action = Action::read_in_round(value, self.round())
                .map_err(|e| context(Failure::usage(e.to_string())))?;
            if actions.last() == Some(&Action::Concede) {
                return Err(context(Failure::refused("concede ends the sequence")));
            }
            position = self
                .apply_action(side, &position, &action)
                .map_err(context)?;
            actions.push(action);
        }
        self.check_answer(side, &actions, &position)
            .map_err(|e| e.with_details(serde_json::json!({"side":side})))?;
        Ok((actions, position))
    }

    pub(crate) fn trial_state(&self, blue: SideState, red: SideState) -> Result<Value, Failure> {
        let mut state = self
            .r#match
            .turns
            .last()
            .map_or_else(State::default, |turn| turn.state.clone());
        state.blue = blue;
        state.red = red;
        let mut value = serde_json::to_value(&state).map_err(|e| Failure::failed(e.to_string()))?;
        for (side, pos) in [("blue", &state.blue), ("red", &state.red)] {
            value[side]["reinforce_chosen"] =
                Value::Bool(pos.reinforce_choice.is_some_and(|i| i >= 0));
            value[side]["shop"] = serde_json::json!({"buys_remaining":pos.shop.buys_remaining,
                "unlocks_remaining":pos.shop.unlocks_remaining,"contraptions_remaining":pos.shop.contraptions_remaining});
        }
        Ok(value)
    }

    pub(crate) fn advance_answers(&mut self, actions: TurnActions) -> Result<(), Failure> {
        for (side, seq) in [(Side::Blue, actions.blue), (Side::Red, actions.red)] {
            let seat = self.turn.side_mut(side);
            seat.decisions = seq;
            seat.committed = true;
        }
        self.advance().map_err(Failure::failed)
    }
}
