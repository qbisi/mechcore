use super::*;

/// A construction that fires: `FightConstruction` as an `ISkillOwner`.
///
/// It is a building — its life, its box and whether it stands are the
/// `BuildingState` the recording holds — that also owns a skill, the way a
/// mech does. `FightConstruction.Update` runs its own
/// `ConstructionSearchTargetController` and then its `SkillManager`, the same
/// manager a `FightMech` updates; what it does not have is a body that moves,
/// so nothing here walks, turns a body or avoids anyone.
///
/// The skill keeps its own lock and finds it with its own search, as a unit's
/// main skill does. The construction's controller keeps a second lock that
/// nothing aims or fires from, and is not carried; what it decides is whether
/// the skill searches at all, which is `searches`.
#[derive(Debug, Clone)]
pub(in crate::fight) struct Construction {
    pub(in crate::fight) team: u32,
    pub(in crate::fight) x: i64,
    pub(in crate::fight) z: i64,
    pub(in crate::fight) x_q32: i64,
    pub(in crate::fight) z_q32: i64,
    /// The row's `radius`, which reach is measured from.
    pub(in crate::fight) radius: i64,
    /// The skill row, with the construction's own damage.
    pub(in crate::fight) attack: AttackConfig,
    /// The rounds left in the magazine.
    pub(in crate::fight) rounds: u32,
    /// How far its weapon turns in one update.
    pub(in crate::fight) turn_q32: i64,
    /// Whether it has a `ConstructionSearchTargetController`, which its row's
    /// `IsEnableSearchTarget` decides: without one, its skill never searches.
    pub(in crate::fight) searches: bool,
    pub(in crate::fight) skill: Skill,
}

impl Construction {
    pub(in crate::fight) fn new(
        building: &BuildingState,
        attack: AttackConfig,
        rotate_speed: i32,
        searches: bool,
    ) -> Self {
        let rounds = attack
            .magazine
            .map_or(u32::MAX, |magazine| magazine.capacity);
        Self {
            team: building.team_id,
            x: building_x(building),
            z: building_z(building),
            x_q32: building.position.x,
            z_q32: building.position.z,
            radius: building_radius(building),
            attack,
            rounds,
            turn_q32: q32_mul(i64::from(rotate_speed) << 32, NATIVE_LOGIC_DELTA_Q32),
            searches,
            // A side's board faces the other side: blue's weapons rest at 0
            // degrees and red's at 180, as its units' bodies do.
            skill: Skill::new(
                vec![if building.team_id == 0 {
                    0
                } else {
                    180_i64 << 32
                }],
                0,
            ),
        }
    }
}

/// The constructions of a layout that fire, by building.
pub(in crate::fight) fn initialize_constructions(
    buildings: &[BuildingState],
    placed: &[ConstructionBuilding],
) -> Result<BTreeMap<u64, Construction>> {
    let mut constructions = BTreeMap::new();
    for placed in placed {
        let Some(attack) = &placed.skill else {
            continue;
        };
        let building = buildings
            .iter()
            .find(|building| {
                building.team_id == placed.team
                    && building.position.x == space_to_q32(placed.x)
                    && building.position.z == space_to_q32(placed.z)
            })
            .ok_or_else(|| Error::new("a construction that fires is not on the board"))?;
        constructions.insert(
            building.building_id,
            Construction::new(
                building,
                attack.clone(),
                placed.rotate_speed,
                placed.searchable,
            ),
        );
    }
    Ok(constructions)
}

impl Simulation {
    /// One construction's update, in the order `FightConstruction.Update`
    /// runs it: its skill, which searches for its own lock, and then its
    /// weapon, which turns towards it.
    pub(in crate::fight) fn step_construction(
        &mut self,
        building_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        if !self.fight_actor_is_alive(FightActorRef::Building(building_id)) {
            let construction = self
                .constructions
                .get_mut(&building_id)
                .expect("construction identity is stable");
            construction.skill.drop_lock();
            construction.skill.set_phase(FightSkillPhase::Idle);
            return Ok(());
        }
        self.update_construction_skill(building_id, step, target_search_order, events)?;
        self.turn_construction_weapon(building_id);
        Ok(())
    }

    /// `SkillManager.UpdateWeaponRotateion`: the weapon turns towards the
    /// lock at the construction's rotate speed.
    pub(in crate::fight) fn turn_construction_weapon(&mut self, building_id: u64) {
        let owner = FightActorRef::Building(building_id);
        let Some(target) = self
            .skill(owner)
            .lock_target
            .and_then(|target| self.fight_actor(target))
        else {
            return;
        };
        let bearing_q32 = self
            .attacker(owner)
            .expect("construction identity is stable")
            .bearing_q32(target.x_q32, target.z_q32);
        self.turn_weapons_towards(owner, bearing_q32);
    }
}
