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
        let magazine = attack.magazine;
        Self {
            team: building.team_id,
            x: building_x(building),
            z: building_z(building),
            x_q32: building.position.x,
            z_q32: building.position.z,
            radius: building_radius(building),
            attack,
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
                magazine,
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
    /// runs it: `SkillManager.Update`, the skill a unit's is, and then
    /// `UpdateWeaponRotateion`, the weapon turning towards the lock.
    pub(in crate::fight) fn step_construction(
        &mut self,
        building_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let owner = FightActorRef::Building(building_id);
        if !self.fight_actor_is_alive(owner) {
            let skill = self.skill_mut(owner);
            skill.drop_lock();
            skill.set_phase(FightSkillPhase::Idle);
            return Ok(());
        }
        if let Some(update) = self.update_skill(owner, step, target_search_order, events)? {
            self.attack_in_reach(owner, step, update, events)?;
        }
        self.turn_construction_weapon(building_id);
        Ok(())
    }

    /// `SkillIdleState.TryStartAttack` and `SkillAttackState.TryPerformAttack`
    /// with what the skill fires at in reach, and the blow they wind up
    /// performed when it is due at once.
    ///
    /// Both are the skill's in the build, asked by `SkillIdleState.Update` and
    /// `SkillAttackState.Update`. The kernel asks them for a unit from its
    /// motion, on the update `MotionAttackState` finds the target in range
    /// (`attack_in_range`); a construction has no motion that runs, so its
    /// skill asks them here, with the same answers to the same questions.
    fn attack_in_reach(
        &mut self,
        owner: FightActorRef,
        step: u64,
        update: SkillUpdate,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let skill = self.skill(owner);
        let Some(target) = skill.mechanical_attack_target() else {
            return Ok(());
        };
        if !self.target_in_attack_range(owner, target) {
            return Ok(());
        }
        // The attack is entered from idle; the state is not updated on the
        // tick it is entered.
        let entered_attack = skill.phase() == FightSkillPhase::Idle;
        let in_attack_angle = self.target_in_attack_angle(owner, target);
        self.try_start_attack(
            owner,
            step,
            target,
            entered_attack,
            in_attack_angle,
            update.prepare_finished,
        );
        if self
            .skill(owner)
            .pending()
            .is_some_and(|pending| pending.step == step)
        {
            self.release(owner, events)?;
        }
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
