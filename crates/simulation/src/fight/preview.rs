//! A deployment observation initialized by the simulator, without a logic tick.
use super::*;
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize)]
pub struct PreviewRelation {
    pub object: String,
    pub in_range: u32,
    pub out_of_range: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewAttackRelation {
    pub object: String,
    pub via_lock: String,
    pub redirected_by: Option<&'static str>,
    pub in_range: u32,
    pub out_of_range: u32,
}

#[derive(Debug, Serialize)]
pub struct PreviewObject {
    pub id: String,
    pub kind: &'static str,
    pub side: &'static str,
    pub index: Option<i32>,
    pub name: String,
    pub lock_basis: &'static str,
    /// Exact member-level initialization values; formation members can differ.
    pub members: Vec<Value>,
    pub locks: Vec<PreviewRelation>,
    pub locked_by: Vec<PreviewRelation>,
    pub attacks: Vec<PreviewAttackRelation>,
    pub attacked_by: Vec<PreviewAttackRelation>,
}

#[derive(Debug, Serialize)]
pub struct DeploymentPreview {
    pub seed: i32,
    pub seed_source: &'static str,
    pub boundary: &'static str,
    pub tick: u64,
    pub objects: Vec<PreviewObject>,
}

fn side(team: u32) -> &'static str {
    match team {
        0 => "blue",
        1 => "red",
        _ => "neutral",
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "preview converts fixed point to model/display floats"
)]
fn q(value: i64) -> f64 {
    value as f64 / Q32_ONE as f64
}

fn position(p: QVec3, team: u32) -> Value {
    let sign = if team == 1 { -1.0 } else { 1.0 };
    json!({"x":q(p.x)*sign,"y":q(p.z)*sign,"height":q(p.y)})
}

fn object(
    id: String,
    kind: &'static str,
    team: u32,
    index: Option<i32>,
    name: String,
) -> PreviewObject {
    PreviewObject {
        id,
        kind,
        side: side(team),
        index,
        name,
        lock_basis: "none",
        members: Vec::new(),
        locks: Vec::new(),
        locked_by: Vec::new(),
        attacks: Vec::new(),
        attacked_by: Vec::new(),
    }
}

fn relation(rows: &mut Vec<PreviewRelation>, target: &str, in_range: bool) {
    let at = rows
        .iter()
        .position(|r| r.object == target)
        .unwrap_or_else(|| {
            rows.push(PreviewRelation {
                object: target.into(),
                in_range: 0,
                out_of_range: 0,
            });
            rows.len() - 1
        });
    if in_range {
        rows[at].in_range += 1;
    } else {
        rows[at].out_of_range += 1;
    }
}

/// Initializes native preparation and uses existing selectors for the forecast.
#[allow(
    clippy::too_many_lines,
    reason = "projects each native object kind without running combat"
)]
pub(crate) fn preview(
    layout: &CompiledLayout,
    original: &mechcore_document::Layout,
    config: &SimulationConfig,
    seed: i32,
    seed_source: &'static str,
) -> Result<DeploymentPreview> {
    let mut simulation =
        Simulation::new_unprepared(layout, &config.units, &config.towers, &config.maps, seed)?;
    simulation.prepare_and_enter_fight()?;
    // A construction has no deployment presearch lock. Forecast its first
    // lock search at these frozen positions, using the actual shared selector.
    let order = simulation.target_search_order();
    let firing = simulation
        .constructions
        .iter()
        .filter(|(_, c)| c.searches)
        .map(|(&id, _)| id)
        .collect::<Vec<_>>();
    for id in firing {
        simulation.search_skill_lock_target(SkillRef::main(FightActorRef::Building(id)), &order)?;
    }
    let mut objects = BTreeMap::<String, PreviewObject>::new();
    let mut identities = BTreeMap::<FightActorRef, String>::new();
    let mut edges = Vec::<(String, FightActorRef, bool)>::new();
    let mut attack_owners = Vec::new();
    for (&id, actor) in &simulation.actors {
        let p = &actor.placement;
        let key = format!("{}/unit/{}", side(p.team), p.formation_index);
        identities.insert(FightActorRef::Unit(id), key.clone());
        let node = objects.entry(key.clone()).or_insert_with(|| {
            object(
                key.clone(),
                "formation",
                p.team,
                Some(p.formation_index),
                p.type_name.clone(),
            )
        });
        node.lock_basis = "deployment_presearch";
        let state = simulation.unit_snapshot(id);
        let mut skills = Vec::new();
        for snapshot in &state.skills {
            let slot = usize::from(snapshot.skill_slot);
            let (owner, _) = actor.skills.at_slot(slot);
            let skill_ref = SkillRef {
                owner: FightActorRef::Unit(id),
                slot: owner,
            };
            let attacker = simulation.skill_attacker(skill_ref);
            skills.push(json!({"slot":snapshot.skill_slot,"enabled":snapshot.enabled.is_some(),
                "attack_damage":snapshot.enabled.as_ref().map(|s|s.attack_damage),
                "attack_range":snapshot.enabled.as_ref().map(|s|q(s.attack_range)),
                "splash_radius":snapshot.enabled.as_ref().map(|s|q(s.splash_range)),
                "attack_interval":attacker.as_ref().map(|a|q(a.attack_interval_q32)),
                "min_range":attacker.as_ref().map(|a|q(space_to_q32(a.min_range))),
                "targets":attacker.as_ref().map(|a|json!({"ground":a.targets.ground,"air":a.targets.air}))}));
        }
        node.members.push(
            json!({"unit_id":id,"position":position(state.position,p.team),
            "level":p.level,"domain":state.domain,"travelling":actor.travelling,
            "life":state.life,"personal_shield":state.personal_shield,
            "move_speed":q(state.move_speed),"collision_radius":q(state.collision_radius),
            "attack_damage_ground":actor.stats.attack_damage_against(UnitDomain::Ground),
            "attack_damage_air":actor.stats.attack_damage_against(UnitDomain::Air),
            "attack_range_ground":q(actor.stats.attack_range_q32_against(UnitDomain::Ground)),
            "attack_range_air":q(actor.stats.attack_range_q32_against(UnitDomain::Air)),
            "damage_reduction_per_hit":actor.stats.reduce_damage(),"targetable":state.targetable,
            "visibility":state.visibility,"skills":skills}),
        );
        // Travelling units are not active target-search owners until arrival.
        if !actor.travelling
            && !actor.skills.main.disabled
            && let Some(target) = actor.skills.main.lock_target
        {
            attack_owners.push((key.clone(), SkillRef::main(FightActorRef::Unit(id)), target));
            edges.push((
                key,
                target,
                simulation.target_in_attack_range(SkillRef::main(FightActorRef::Unit(id)), target),
            ));
        }
    }
    for building in &simulation.buildings {
        let construction = layout.constructions.iter().find(|c| {
            c.team == building.team_id
                && space_to_q32(c.x) == building.position.x
                && space_to_q32(c.z) == building.position.z
                && c.building_type_id == building.building_type_id
        });
        let interceptor = layout.interceptors.iter().find(|c| {
            c.team == building.team_id
                && space_to_q32(c.x) == building.position.x
                && space_to_q32(c.z) == building.position.z
        });
        let (key, kind, index, name) = if let Some(c) = construction {
            let input = if c.team == 0 {
                &original.blue
            } else {
                &original.red
            };
            let stated = &input.constructions[c.group];
            (
                format!("{}/construction/{}", side(c.team), stated.index),
                "construction",
                Some(stated.index),
                stated.type_name.clone(),
            )
        } else if let Some(c) = interceptor {
            (
                format!("{}/contraption/{}", side(c.team), c.deployment_index),
                "contraption",
                Some(c.deployment_index),
                "interceptor".into(),
            )
        } else {
            let name = match building.building_type_id {
                1 => "energy_tower".to_owned(),
                2 => "research_center".to_owned(),
                other => format!("building_type_{other}"),
            };
            (
                format!("{}/building/{name}", side(building.team_id)),
                "building",
                None,
                name,
            )
        };
        identities.insert(FightActorRef::Building(building.building_id), key.clone());
        let node = objects
            .entry(key.clone())
            .or_insert_with(|| object(key.clone(), kind, building.team_id, index, name));
        let owner = FightActorRef::Building(building.building_id);
        let attacker = simulation.attacker(owner);
        if simulation
            .constructions
            .get(&building.building_id)
            .is_some_and(|c| c.searches)
        {
            node.lock_basis = "initial_search_forecast";
        }
        node.members.push(json!({"building_id":building.building_id,"position":position(building.position,building.team_id),
            "life":building.life,"targetable":building.targetable,
            "attack_damage":attacker.as_ref().map(|a|a.attack_damage),
            "attack_range":attacker.as_ref().map(|a|q(space_to_q32(a.attack_range))),
            "min_range":attacker.as_ref().map(|a|q(space_to_q32(a.min_range))),
            "attack_interval":attacker.as_ref().map(|a|q(a.attack_interval_q32)),
            "interception":interceptor.map(|c|json!({"attack":c.interception.attack,"min_range":q(c.interception.range_min_q32),"max_range":q(c.interception.range_max_q32)}))}));
        if let Some(c) = simulation.constructions.get(&building.building_id)
            && let Some(target) = c.skills.main.lock_target
        {
            if node.lock_basis == "initial_search_forecast" {
                attack_owners.push((key.clone(), SkillRef::main(owner), target));
            }
            edges.push((
                key,
                target,
                simulation.target_in_attack_range(SkillRef::main(owner), target),
            ));
        }
    }
    for mine in &layout.missiles {
        let key = format!("{}/contraption/{}", side(mine.team), mine.deployment_index);
        let mut node = object(
            key.clone(),
            "contraption",
            mine.team,
            Some(mine.deployment_index),
            "missile".into(),
        );
        node.lock_basis = "initial_trigger_forecast";
        node.members.push(json!({"position":position(QVec3{x:space_to_q32(mine.x),y:0,z:space_to_q32(mine.z)},mine.team),
            "trigger_range":q(mine.trigger_range_q32),"attack_damage":mine.shot.damage,
            "splash_radius":q(space_to_q32(mine.shot.splash_radius))}));
        if let Some(target) = simulation.nearest_mine_target(&Mine::new(mine)) {
            edges.push((key.clone(), target, true));
        }
        objects.insert(key, node);
    }
    let mut shield_identities = BTreeMap::new();
    for shield in simulation.shield_states() {
        let index = simulation
            .shield
            .standing
            .iter()
            .find(|s| s.id == shield.shield_id)
            .and_then(|s| s.deployment_index);
        let key = index.map_or_else(
            || format!("shield/{}", shield.shield_id),
            |index| format!("{}/contraption/{index}", side(shield.team_id)),
        );
        let mut node = object(
            key.clone(),
            "shield",
            shield.team_id,
            index,
            "shield".into(),
        );
        node.members.push(json!({"shield_id":shield.shield_id,"position":position(shield.position,shield.team_id),
            "energy":shield.energy,"radius":q(shield.radius),"active":shield.active,"source_kind":shield.source_kind}));
        shield_identities.insert(shield.shield_id, key.clone());
        objects.insert(key, node);
    }
    for (source, skill, lock) in attack_owners {
        // Forecast SearchAttackTarget without changing the presearch lock,
        // sibling assignments, motion requests or any combat state.
        let attacker = simulation
            .skill_attacker(skill)
            .expect("search owner exists");
        let shield = simulation.search_target_shield_in(skill, lock, attacker.shield_range());
        let wall = simulation.wall_in_the_way(skill, None, lock, shield);
        let (destination, redirected_by, in_range) = if let Some(wall) = wall {
            let target = FightActorRef::Building(wall);
            (
                &identities[&target],
                Some("wall"),
                simulation.target_in_attack_range(skill, target),
            )
        } else if let Some(shield) = shield {
            let in_range = simulation
                .shield_attack_point(shield, skill, Some(lock))
                .is_some_and(|(x, z)| attacker.reaches_shield(x, z));
            (&shield_identities[&shield], Some("shield"), in_range)
        } else {
            (
                &identities[&lock],
                None,
                simulation.target_in_attack_range(skill, lock),
            )
        };
        let via_lock = &identities[&lock];
        attack_relation(
            &mut objects.get_mut(&source).expect("owner exists").attacks,
            destination,
            via_lock,
            redirected_by,
            in_range,
        );
        attack_relation(
            &mut objects
                .get_mut(destination)
                .expect("target exists")
                .attacked_by,
            &source,
            via_lock,
            redirected_by,
            in_range,
        );
    }
    for (source, target, in_range) in edges {
        let destination = identities
            .get(&target)
            .ok_or_else(|| Error::new("preview target has no object identity"))?;
        relation(
            &mut objects
                .get_mut(&source)
                .expect("preview owner exists")
                .locks,
            destination,
            in_range,
        );
        relation(
            &mut objects
                .get_mut(destination)
                .expect("preview target exists")
                .locked_by,
            &source,
            in_range,
        );
    }
    for node in objects.values_mut() {
        node.locks.sort_by(|a, b| a.object.cmp(&b.object));
        node.locked_by.sort_by(|a, b| a.object.cmp(&b.object));
        node.attacks.sort_by(|a, b| {
            (&a.object, &a.via_lock, a.redirected_by).cmp(&(
                &b.object,
                &b.via_lock,
                b.redirected_by,
            ))
        });
        node.attacked_by.sort_by(|a, b| {
            (&a.object, &a.via_lock, a.redirected_by).cmp(&(
                &b.object,
                &b.via_lock,
                b.redirected_by,
            ))
        });
    }
    Ok(DeploymentPreview {
        seed,
        seed_source,
        boundary: "fight_start_presearch",
        tick: simulation.step_now,
        objects: objects.into_values().collect(),
    })
}

fn attack_relation(
    rows: &mut Vec<PreviewAttackRelation>,
    object: &str,
    via_lock: &str,
    redirected_by: Option<&'static str>,
    in_range: bool,
) {
    let index = rows
        .iter()
        .position(|r| {
            r.object == object && r.via_lock == via_lock && r.redirected_by == redirected_by
        })
        .unwrap_or_else(|| {
            rows.push(PreviewAttackRelation {
                object: object.into(),
                via_lock: via_lock.into(),
                redirected_by,
                in_range: 0,
                out_of_range: 0,
            });
            rows.len() - 1
        });
    if in_range {
        rows[index].in_range += 1;
    } else {
        rows[index].out_of_range += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attack_forecasts_keep_locks_and_distinguish_walls_shields_and_crossing() {
        // Wall geometry from the native line-of-fire fixture; the crate test
        // states its layout and never reads the recording fixture.
        let wall = b"kind: layout\nseed: 4242\nround: 1\nblue:\n  units:\n  - {name: marksman, index: 0, position: {x: -90, y: -90}}\n  constructions:\n  - {name: defensive_wall, index: 0, position: {x: -140, y: -55}}\nred:\n  units:\n  - {name: marksman, index: 0, position: {x: 170, y: -20}}\n";
        let result = crate::preview_document(wall, None).unwrap();
        let shooter = result
            .objects
            .iter()
            .find(|o| o.id == "red/unit/0")
            .unwrap();
        assert!(shooter.locks.iter().any(|r| r.object == "blue/unit/0"));
        let attack = shooter
            .attacks
            .iter()
            .find(|r| r.redirected_by == Some("wall"))
            .unwrap();
        assert_eq!(attack.object, "blue/construction/0");
        assert_eq!(attack.via_lock, "blue/unit/0");
        assert_eq!(attack.in_range, 1);
        assert_attack_reverses(&result);

        for (unit, redirects) in [("steel_ball", true), ("crawler", false)] {
            let layout = format!(
                "kind: layout\nseed: 4242\nround: 1\nblue:\n  units:\n  - {{name: {unit}, index: 0, position: {{x: 5, y: -30}}}}\nred:\n  units:\n  - {{name: marksman, index: 0, position: {{x: 0, y: -80}}}}\n  contraptions:\n  - {{name: shield, index: 1, position: {{x: 1, y: -81}}}}\n"
            );
            let result = crate::preview_document(layout.as_bytes(), None).unwrap();
            let shooter = result
                .objects
                .iter()
                .find(|o| o.id == "blue/unit/0")
                .unwrap();
            assert!(!shooter.attacks.is_empty());
            assert_eq!(
                shooter
                    .attacks
                    .iter()
                    .any(|r| r.redirected_by == Some("shield")),
                redirects
            );
            if redirects {
                assert!(shooter.attacks.iter().all(|r| r.in_range > 0));
                assert!(
                    shooter
                        .attacks
                        .iter()
                        .all(|r| r.object == "red/contraption/1" && r.via_lock == "red/unit/0")
                );
            } else {
                assert!(
                    shooter
                        .attacks
                        .iter()
                        .all(|r| r.object == r.via_lock && r.redirected_by.is_none())
                );
            }
            assert_eq!(result.tick, 0);
            assert_attack_reverses(&result);
        }
    }

    fn assert_attack_reverses(result: &DeploymentPreview) {
        for source in &result.objects {
            for attack in &source.attacks {
                let target = result
                    .objects
                    .iter()
                    .find(|o| o.id == attack.object)
                    .unwrap();
                let reverse = target
                    .attacked_by
                    .iter()
                    .find(|r| {
                        r.object == source.id
                            && r.via_lock == attack.via_lock
                            && r.redirected_by == attack.redirected_by
                    })
                    .unwrap();
                assert_eq!(
                    (attack.in_range, attack.out_of_range),
                    (reverse.in_range, reverse.out_of_range)
                );
            }
        }
    }

    const MULTI: &str = "kind: layout\nround: 1\nseed: 42\nblue:\n  units:\n  - {name: crawler, index: 0, position: {x: 5, y: -50}}\n  - {name: stormcaller, index: 1, position: {x: 105, y: -50}}\nred:\n  units:\n  - {name: rhino, index: 0, position: {x: -45, y: -55}}\n  - {name: rhino, index: 1, position: {x: 45, y: -55}}\n";

    #[test]
    fn formation_graph_keeps_multiple_targets_ranges_and_reverse_edges() {
        let result = crate::preview_document(MULTI.as_bytes(), None).unwrap();
        assert_eq!(result.tick, 0);
        let crawlers = result
            .objects
            .iter()
            .find(|o| o.id == "blue/unit/0")
            .unwrap();
        assert_eq!(crawlers.members.len(), 24);
        assert!(crawlers.locks.len() >= 2, "{:?}", crawlers.locks);
        assert!(crawlers.locks.iter().any(|r| r.out_of_range > 0));
        let stormcallers = result
            .objects
            .iter()
            .find(|o| o.id == "blue/unit/1")
            .unwrap();
        assert!(stormcallers.locks.iter().any(|r| r.in_range > 0));
        for source in &result.objects {
            for edge in &source.locks {
                let target = result.objects.iter().find(|o| o.id == edge.object).unwrap();
                let reverse = target
                    .locked_by
                    .iter()
                    .find(|r| r.object == source.id)
                    .unwrap();
                assert_eq!(
                    (edge.in_range, edge.out_of_range),
                    (reverse.in_range, reverse.out_of_range)
                );
            }
        }
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            serde_json::to_value(crate::preview_document(MULTI.as_bytes(), None).unwrap()).unwrap()
        );
    }

    #[test]
    fn effective_properties_include_officer_effects_without_moving_members() {
        let base = crate::preview_document(MULTI.as_bytes(), None).unwrap();
        let buffed = MULTI.replace(
            "blue:\n",
            "blue:\n  officers: [advanced_offensive_tactics]\n",
        );
        let buffed = crate::preview_document(buffed.as_bytes(), None).unwrap();
        let a = &base
            .objects
            .iter()
            .find(|o| o.id == "blue/unit/1")
            .unwrap()
            .members[0];
        let b = &buffed
            .objects
            .iter()
            .find(|o| o.id == "blue/unit/1")
            .unwrap()
            .members[0];
        assert_eq!(a["position"], b["position"]);
        assert!(
            b["attack_damage_ground"].as_i64().unwrap()
                > a["attack_damage_ground"].as_i64().unwrap()
        );
        let config = SimulationConfig::load().unwrap();
        let (_, layout) = crate::layout::compile_preview(MULTI.as_bytes(), &config.units).unwrap();
        let mut simulation =
            Simulation::new_unprepared(&layout, &config.units, &config.towers, &config.maps, 42)
                .unwrap();
        simulation.prepare_and_enter_fight().unwrap();
        for member in &base
            .objects
            .iter()
            .find(|o| o.id == "blue/unit/0")
            .unwrap()
            .members
        {
            let id = member["unit_id"].as_u64().unwrap();
            let actual = simulation.unit_snapshot(id);
            assert_eq!(member["life"], serde_json::to_value(actual.life).unwrap());
            assert_eq!(
                member["position"],
                position(actual.position, actual.team_id)
            );
        }
    }

    #[test]
    fn buildings_devices_and_overlapping_shields_keep_distinct_identities() {
        let input=b"kind: layout\nround: 1\nblue:\n  units: []\n  constructions:\n  - {name: anti_armor_turret, index: 7, position: {x: -100, y: -50}}\n  contraptions:\n  - {name: shield, index: 0, position: {x: 0, y: -150}}\n  - {name: shield, index: 1, position: {x: 0, y: -150}}\n  - {name: interceptor, index: 2, position: {x: 5, y: -105}}\n  - {name: missile, index: 3, position: {x: 200, y: -100}}\nred:\n  units:\n  - {name: rhino, index: 0, position: {x: 105, y: -55}}\n";
        let result = crate::preview_document(input, None).unwrap();
        assert_eq!(result.seed_source, "default");
        for id in [
            "blue/contraption/0",
            "blue/contraption/1",
            "blue/contraption/2",
            "blue/contraption/3",
        ] {
            assert_eq!(result.objects.iter().filter(|o| o.id == id).count(), 1);
        }
        let turret = result
            .objects
            .iter()
            .find(|o| o.id == "blue/construction/7")
            .unwrap();
        assert!(!turret.locks.is_empty());
        assert!(!turret.locked_by.is_empty());
        let interceptor = result
            .objects
            .iter()
            .find(|o| o.id == "blue/contraption/2")
            .unwrap();
        assert!(interceptor.locks.is_empty());
        assert!(crate::preview_document(input, Some(0)).is_err());
    }
}
