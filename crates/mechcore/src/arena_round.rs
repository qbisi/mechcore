//! The arena's immutable round opening, pure-JSON `problem`, and official settlement.

use mechcore_document::standalone::{StandaloneState, problem_json};

use super::{Draw, Failure, Game, MatchOptions, Side, TurnActions, Value, dealt_game};

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

    pub(crate) fn standalone_opening(&self) -> Result<StandaloneState, Failure> {
        StandaloneState::from_match_round(&self.r#match, self.round()).map_err(Failure::failed)
    }

    pub(crate) fn problem_json(&self, ref_id: &str) -> Result<Value, Failure> {
        problem_json(&self.r#match, self.round(), Some(ref_id)).map_err(Failure::failed)
    }

    pub(crate) fn advance_answers(&mut self, actions: TurnActions) -> Result<(), Failure> {
        for (side, seq) in [(Side::Blue, actions.blue), (Side::Red, actions.red)] {
            let seat = self.turn.side_mut(side);
            seat.decisions = seq;
            seat.committed = true;
        }
        self.advance_outcome()
    }
}
