//! Current purchase/research choices, checked through the match action path.
use super::{
    Action, Args, Failure, Game, Held, Side, SideState, Value, View, side_option, transition,
    turn_path,
};
use mechcore_document::unit_type_from_id;
use serde_json::json;

pub(super) fn run(mut args: Args) -> Result<View, Failure> {
    let stage = args
        .value("--stage")?
        .ok_or_else(|| Failure::usage("candidates needs --stage buy_unit|upgrade_technology"))?;
    if !matches!(stage.as_str(), "buy_unit" | "upgrade_technology") {
        return Err(Failure::usage(
            "stage must be buy_unit or upgrade_technology",
        ));
    }
    let side = side_option(&mut args, true)?.unwrap_or(Side::Blue);
    let path = args.path("a match document")?;
    args.finish()?;
    let mut held = Held::take(&turn_path(&path))?;
    let mut game = Game::read(&path, &mut held, None, side)?;
    game.editable(side)?;
    if game.round() < 1 {
        return Err(Failure::refused("candidates requires a deployment round"));
    }
    let position = game.position(side)?;
    let mut rows = Vec::new();
    for (unit, techs) in game.economy.unit_technologies() {
        if stage == "buy_unit" {
            purchase(&mut game, side, &position, unit, false, &mut rows)?;
            if !position.energy_tower_skills.contains(&4) {
                purchase(&mut game, side, &position, unit, true, &mut rows)?;
            }
        } else {
            for tech in techs {
                let action = Action::UpgradeTechnology { unit, tech };
                let cost = transition::technology_price(game.economy, &position, tech)
                    .ok_or_else(|| Failure::failed("technology has no price"))?;
                let intent =
                    serde_json::to_value(&action).map_err(|e| Failure::failed(e.to_string()))?;
                let reasons = trial(&mut game, side, std::slice::from_ref(&action))?;
                rows.push(json!({"intent":intent,"dependencies":[],"cost":cost,"legal":reasons.is_empty(),"reasons":reasons}));
            }
        }
    }
    rows.push(json!({"intent":{"type":"pass_stage"},"dependencies":[],"cost":0,"legal":true,"reasons":[]}));
    let mut view = game.view(Some(side), false)?;
    view.candidates = Some(json!({"stage":stage,"choices":rows}));
    Ok(view)
}

fn purchase(
    game: &mut Game,
    side: Side,
    state: &SideState,
    unit: i32,
    elite: bool,
    rows: &mut Vec<Value>,
) -> Result<(), Failure> {
    let mut dependencies = Vec::new();
    let mut cost = 0;
    if !state.unlocked_units.contains(&unit) {
        let Some(price) = transition::unlock_price(game.economy, state, unit) else {
            return Ok(());
        };
        cost += price;
        dependencies.push(Action::UnlockUnit { unit });
    }
    if state.shop.buys_remaining < 1 && !state.energy_tower_skills.contains(&3) {
        dependencies.push(Action::ActiveEnergyTowerSkill { skill: 3 });
        cost += game
            .economy
            .energy_tower_skill(3)
            .ok_or_else(|| Failure::failed("missing Mass Recruitment"))?
            .supply;
    }
    let mut priced = state.clone();
    if elite {
        dependencies.push(Action::ActiveEnergyTowerSkill { skill: 4 });
        priced.energy_tower_skills.push(4);
        cost += game
            .economy
            .energy_tower_skill(4)
            .ok_or_else(|| Failure::failed("missing Elite Recruitment"))?
            .supply;
    }
    let (level, price) = transition::purchase_terms(game.economy, &priced, unit)
        .ok_or_else(|| Failure::failed("unit has no purchase price"))?;
    cost += price;
    let mut sequence = dependencies.clone();
    sequence.push(Action::BuyUnit {
        unit,
        position: None,
        rotated: false,
    });
    let reasons = trial(game, side, &sequence)?;
    let (name, _) = unit_type_from_id(unit).ok_or_else(|| Failure::failed("unnamed unit"))?;
    rows.push(
        json!({"intent":{"type":"buy_unit","name":name,"level":level},
        "dependencies":dependencies,"cost":cost,"legal":reasons.is_empty(),"reasons":reasons}),
    );
    Ok(())
}

fn trial(game: &mut Game, side: Side, sequence: &[Action]) -> Result<Vec<Value>, Failure> {
    let saved = game.turn.side(side).clone();
    let result = sequence.iter().try_for_each(|action| {
        game.take(side, action)?;
        game.turn.side_mut(side).decisions.push(action.clone());
        Ok::<_, Failure>(())
    });
    *game.turn.side_mut(side) = saved;
    match result {
        Ok(()) => Ok(Vec::new()),
        Err(error) if error.object("match.candidates")["kind"] == "refused" => Ok(vec![
            json!({"code":"rule_refused","message":error.reason()}),
        ]),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{PathBuf, deal, fs};
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("match.yaml");
        let mut held = Held::take(&turn_path(&path)).unwrap();
        let mut game = deal(
            &path,
            &mut held,
            (Some(12345), Some(1021), None),
            None,
            None,
        )
        .unwrap();
        for side in Side::BOTH {
            let offer = game.opening(side).offers[0];
            game.turn
                .side_mut(side)
                .decisions
                .push(Action::ChooseAdvanceTeam {
                    index: 0,
                    id: offer.team,
                    specialist: offer.specialist,
                });
            game.commit(side, &mut held).unwrap();
        }
        game = game.settle(&mut held).unwrap();
        let blue = &mut game.r#match.turns.last_mut().unwrap().state.blue;
        blue.supply = 5000;
        blue.officers.clear();
        blue.energy_tower_skills.clear();
        blue.unlocked_units = vec![5, 10];
        blue.techs.clear();
        game.r#match
            .blue
            .tech_loadout
            .insert(5, vec![905, 1109, 2305, 3005]);
        game.turn.deploy_time = 3600;
        game.write(&mut held).unwrap();
        (dir, path)
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "tests pass temporary JSON requests"
    )]
    fn request(path: &std::path::Path, value: Value) -> Value {
        crate::requests::answer(path, Side::Blue, &value.to_string(), &|| false)
    }

    fn success(path: &std::path::Path, value: Value) -> Value {
        let answer = request(path, value);
        assert_ne!(answer["schema"], "mechcore.error", "{answer}");
        answer
    }

    fn choices(path: &std::path::Path, stage: &str) -> Vec<Value> {
        success(path, json!({"op":"match.candidates", "stage":stage}))["candidates"]["choices"]
            .as_array()
            .unwrap()
            .clone()
    }

    #[test]
    fn undo_and_replace_restore_observations_atomically_and_keep_privacy() {
        let (_dir, path) = fixture();
        let original = success(&path, json!({"op":"match.show"}));
        let public = fs::read(&path).unwrap();
        let actions = json!([
            {"type":"active_energy_tower_skill","name":"elite_recruitment"},
            {"type":"buy_unit","name":"rhino"}
        ]);
        let acted = success(&path, json!({"op":"match.act","decision":actions}));
        assert_eq!(acted["actions"], actions);
        let shown = success(&path, json!({"op":"match.show"}));
        assert_eq!(acted["state"], shown["state"]);
        assert_eq!(acted["opened"], original["opened"]);
        let other = crate::requests::answer(&path, Side::Red, "{\"op\":\"match.show\"}", &|| false);
        assert_eq!(other["actions"], json!([]));
        assert_eq!(
            other["state"]["blue"]["supply"],
            original["state"]["blue"]["supply"]
        );
        let saved = fs::read(turn_path(&path)).unwrap();
        let failed = request(
            &path,
            json!({"op":"match.act","force_push":true,"decision":[
                {"type":"active_energy_tower_skill","name":"elite_recruitment"},
                {"type":"active_energy_tower_skill","name":"elite_recruitment"}
            ]}),
        );
        assert_eq!(failed["schema"], "mechcore.error");
        assert_eq!(fs::read(turn_path(&path)).unwrap(), saved);
        let dry = success(
            &path,
            json!({"op":"match.act","force_push":true,"dry_run":true,"decision":[]}),
        );
        assert_eq!(dry["state"], original["state"]);
        assert_eq!(fs::read(turn_path(&path)).unwrap(), saved);
        let undone = success(&path, json!({"op":"match.undo","count":2}));
        assert_eq!(undone["state"], original["state"]);
        assert_eq!(undone["actions"], json!([]));
        success(&path, json!({"op":"match.act","decision":actions}));
        let cleared = success(
            &path,
            json!({"op":"match.act","force_push":true,"decision":[]}),
        );
        assert_eq!(cleared["state"], original["state"]);
        assert_eq!(fs::read(&path).unwrap(), public);
        success(&path, json!({"op":"match.commit"}));
        for edit in [
            json!({"op":"match.undo"}),
            json!({"op":"match.act","force_push":true,"decision":[]}),
        ] {
            assert_eq!(request(&path, edit)["schema"], "mechcore.error");
        }
    }

    #[test]
    fn purchase_levels_dependencies_and_research_prices_follow_actions() {
        let (_dir, path) = fixture();
        let before_turn = fs::read(turn_path(&path)).unwrap();
        let before_match = fs::read(&path).unwrap();
        let initial = choices(&path, "buy_unit");
        assert_eq!(fs::read(turn_path(&path)).unwrap(), before_turn);
        assert_eq!(fs::read(&path).unwrap(), before_match);
        let elite = initial
            .iter()
            .find(|r| r["intent"]["name"] == "rhino" && r["intent"]["level"] == 2)
            .unwrap();
        assert_eq!(elite["cost"], 400);
        assert_eq!(elite["legal"], true);
        assert_eq!(elite["dependencies"].as_array().unwrap().len(), 1);
        success(
            &path,
            json!({"op":"match.act","decision":[
                {"type":"active_energy_tower_skill","name":"elite_recruitment"},
                {"type":"buy_unit","name":"rhino"}
            ]}),
        );
        let raised = choices(&path, "buy_unit");
        assert!(
            !raised
                .iter()
                .any(|r| r["intent"]["name"] == "rhino" && r["intent"]["level"] == 1)
        );
        let before = choices(&path, "upgrade_technology");
        let techs: Vec<_> = before
            .iter()
            .filter(|r| r["intent"]["unit"] == "rhino" && r["legal"] == true)
            .collect();
        assert!(techs.len() >= 2);
        let first = techs[0]["intent"].clone();
        let second = techs[1]["intent"].clone();
        let old_cost = techs[1]["cost"].as_i64().unwrap();
        success(&path, json!({"op":"match.act","decision":first}));
        let after = choices(&path, "upgrade_technology");
        assert_eq!(
            after.iter().find(|r| r["intent"] == second).unwrap()["cost"],
            old_cost + 200
        );
        success(&path, json!({"op":"match.undo"}));
        assert_eq!(choices(&path, "upgrade_technology"), before);
        success(
            &path,
            json!({"op":"match.act","decision":{"type":"buy_unit","name":"rhino"}}),
        );
        let exhausted = choices(&path, "buy_unit");
        let next = exhausted
            .iter()
            .find(|r| r["intent"]["name"] == "rhino")
            .unwrap();
        assert_eq!(next["dependencies"][0]["name"], "mass_recruitment");
        assert_eq!(next["cost"], 350);
        assert_eq!(next["legal"], true);
    }

    #[test]
    fn combined_dependencies_quote_the_executable_total_and_enforce_allowances() {
        let (_dir, path) = fixture();
        success(
            &path,
            json!({"op":"match.act","decision":[
                {"type":"buy_unit","name":"rhino"},
                {"type":"buy_unit","name":"rhino"}
            ]}),
        );
        let rows = choices(&path, "buy_unit");
        let choice = rows
            .iter()
            .find(|r| r["intent"]["name"] == "fortress" && r["intent"]["level"] == 2)
            .unwrap();
        assert_eq!(choice["legal"], true);
        assert_eq!(choice["cost"], 950);
        let mut actions = choice["dependencies"].as_array().unwrap().clone();
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0]["type"], "unlock_unit");
        assert_eq!(actions[1]["name"], "mass_recruitment");
        assert_eq!(actions[2]["name"], "elite_recruitment");
        actions.push(json!({"type":"buy_unit","name":"fortress"}));
        let before = success(&path, json!({"op":"match.show"}));
        let after = success(&path, json!({"op":"match.act","decision":actions}));
        assert_eq!(
            before["state"]["blue"]["supply"].as_i64().unwrap()
                - after["state"]["blue"]["supply"].as_i64().unwrap(),
            950
        );
        let exhausted = choices(&path, "buy_unit");
        assert!(
            exhausted
                .iter()
                .filter(|r| r["intent"]["type"] == "buy_unit")
                .all(|r| r["legal"] == false)
        );
        let saved = fs::read(turn_path(&path)).unwrap();
        for edit in [
            json!({"op":"match.undo","count":0}),
            json!({"op":"match.undo","count":99}),
            json!({"op":"match.act","force_push":true,"decision":{"type":"buy_unit","name":"rhino"}}),
        ] {
            assert_eq!(request(&path, edit)["schema"], "mechcore.error");
            assert_eq!(fs::read(turn_path(&path)).unwrap(), saved);
        }
    }

    #[test]
    fn expired_drafts_cannot_be_edited_or_queried_as_live_choices() {
        let (_dir, path) = fixture();
        {
            let mut held = Held::take(&turn_path(&path)).unwrap();
            let mut turn = held.read().unwrap().unwrap();
            turn.opened = "2000-01-01T00:00:00.000000Z".into();
            held.write(&turn).unwrap();
        }
        let old_turn = fs::read(turn_path(&path)).unwrap();
        let old_match = fs::read(&path).unwrap();
        for operation in [
            json!({"op":"match.act","force_push":true,"decision":[]}),
            json!({"op":"match.undo"}),
            json!({"op":"match.candidates","stage":"buy_unit"}),
        ] {
            let refused = request(&path, operation);
            assert_eq!(refused["kind"], "refused");
            assert!(refused["reason"].as_str().unwrap().contains("deadline"));
            assert_eq!(fs::read(turn_path(&path)).unwrap(), old_turn);
            assert_eq!(fs::read(&path).unwrap(), old_match);
        }
    }
}
