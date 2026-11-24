//! The arena's immutable round opening and complete, side-local answers.
use super::{Action, Draw, Failure, Game, Side, SideState, State, TurnActions, Value, dealt_game};
use mechcore_document::r#match::Rules;

impl Game {
    pub(crate) fn arena(
        seed: Option<i32>,
        map: Option<i32>,
        names: [String; 2],
        rules: Rules,
    ) -> Result<Self, Failure> {
        let [blue, red] = names;
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
            Some(blue),
            rules,
            draw,
        )?;
        game.r#match.red.name = red;
        game.turn.side_mut(Side::Red).given = true;
        Ok(game)
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
        let mut position = self.opened(side)?;
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
