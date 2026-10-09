//! `CloakSystem`: a unit that a technology cloaks while it fights at a
//! distance.
//!
//! `MoveAbilityDynamicProvider.DoActive` hands each unit whose technology
//! is an `IMoveAbilityDynamicSource` a `CloakController` of its side's
//! `TeamCloakManager` (`CloakSystem.Active`), as the fight starts or as it
//! lands, joins or rises again, in state `None`. `CloakSystem` is the first
//! module `FightController.AddModules` adds, so it updates before every
//! other, each side's units in the order `OnFightStart` sorts them
//! (`FightUtility.SkillOwnerComparer`). A unit is ready to cloak while the
//! nearest unit its main skill's search measured stands beyond its range,
//! edge to edge (`IsNothingAround`; `IsMainSearcherSkillIdle` reads a flag
//! nothing sets). `CloakController.Update` moves it on: from `None`, a
//! ready unit starts counting (`IsTransitionCloak`); counting, a ready
//! unit counts a step, which once it reaches the source's delay cloaks it
//! (`Cloak`, `ActorVisibility.Disappear`), and a unit not ready counts from
//! none again; cloaked, a unit not ready starts to show (`IsTransitionNone`);
//! showing, it counts a step whatever happens, which once it reaches the
//! exit delay makes it seen again (`None`, `Normal`). Each blow of any of its
//! skills (`FightMech.OnMechSkillPerformAttack`) starts a cloaked unit
//! showing and sends a counting one back to none. Its technologies switched
//! off start it showing, or a counting one from none, and it holds still
//! until they come back, in `None` (`Disable`, `Enable`); its death shows it
//! at once (`Deactive`). A unit that cannot be seen is still searched and
//! locked, and is out of every skill's range but the one moving
//! underground. `docs/rules/technology_effects.md` states the rule.

use super::*;

/// `CloakController.State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloakState {
    None,
    /// `IsTransitionCloak`: counting towards the cloak.
    Entering,
    Cloaked,
    /// `IsTransitionNone`: counting towards being seen.
    Leaving,
}

/// A unit's `CloakController`.
#[derive(Debug, Clone)]
pub(in crate::fight) struct Cloak {
    state: CloakState,
    /// `enterTimer` and `exitTimer`, in updates.
    enter: u64,
    exit: u64,
    /// `delayEnterConfig` and `delayExitConfig`, in updates.
    enter_steps: u64,
    exit_steps: u64,
    /// `isEnable`.
    enabled: bool,
}

impl Cloak {
    /// `BreakCloak`: a cloaked unit starts showing, a counting one counts
    /// from none.
    fn break_cloak(&mut self) {
        match self.state {
            CloakState::Cloaked => {
                self.exit = 0;
                self.state = CloakState::Leaving;
            }
            CloakState::Entering => self.enter = 0,
            CloakState::None | CloakState::Leaving => {}
        }
    }
}

impl Simulation {
    /// `TeamCloakManager.OnEnterFight` of every unit the fight starts with on
    /// the ground, whose controller its activation during deployment made:
    /// `ResetData` leaves it counting with its count full, so a ready unit
    /// cloaks on its first update. A travelling unit's is made as it lands.
    pub(in crate::fight) fn enter_cloak_fight(&mut self) {
        let held = self
            .actors
            .iter()
            .filter(|(_, actor)| {
                !actor.travelling && actor.placement.effects.single.cloak.is_some()
            })
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        for unit in held {
            self.activate_cloak(unit);
            let cloak = self
                .actors
                .get_mut(&unit)
                .and_then(|actor| actor.cloak.as_mut())
                .expect("made above");
            cloak.enter = cloak.enter_steps;
            cloak.exit = 0;
            cloak.state = CloakState::Entering;
        }
    }

    /// `CloakController.Active`, made on first need (`TeamCloakManager.
    /// GetCtr`): `None`, enabled.
    pub(in crate::fight) fn activate_cloak(&mut self, unit: u64) {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        let Some(source) = actor.placement.effects.single.cloak else {
            return;
        };
        let cloak = actor.cloak.get_or_insert_with(|| Cloak {
            state: CloakState::None,
            enter: 0,
            exit: 0,
            enter_steps: seconds_q32_to_steps(source.enter_q32),
            exit_steps: seconds_q32_to_steps(source.exit_q32),
            enabled: true,
        });
        cloak.state = CloakState::None;
        cloak.enabled = true;
    }

    /// `CloakController.Deactive`, as the unit dies: a cloaked unit is seen
    /// at once.
    pub(in crate::fight) fn deactivate_cloak(&mut self, unit: u64) {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        let Some(cloak) = actor.cloak.as_mut() else {
            return;
        };
        match cloak.state {
            CloakState::Cloaked => {
                cloak.state = CloakState::None;
                actor.visibility = Visibility::Normal;
            }
            CloakState::Entering => cloak.enter = 0,
            CloakState::None | CloakState::Leaving => {}
        }
        cloak.enabled = false;
    }

    /// `CloakController.Disable` and `Enable`.
    pub(in crate::fight) fn switch_cloak(&mut self, unit: u64, on: bool) {
        let actor = self
            .actors
            .get_mut(&unit)
            .expect("actor identity is stable");
        let Some(cloak) = actor.cloak.as_mut() else {
            return;
        };
        if on {
            cloak.state = CloakState::None;
            cloak.enabled = true;
        } else {
            cloak.break_cloak();
            cloak.enabled = false;
        }
    }

    /// `CloakController.OnISkillOwnerAttack`, on each blow of any of the
    /// unit's skills.
    pub(in crate::fight) fn cloak_on_blow(&mut self, unit: u64) {
        if let Some(cloak) = self
            .actors
            .get_mut(&unit)
            .and_then(|actor| actor.cloak.as_mut())
        {
            cloak.break_cloak();
        }
    }

    /// `CloakSystem.Update`: each side's units, by the second coordinate
    /// where each stands, then the first.
    pub(in crate::fight) fn step_cloaks(&mut self) {
        let mut order = self
            .actors
            .iter()
            .filter(|(_, actor)| actor.cloak.is_some())
            .map(|(&id, actor)| ((actor.placement.team, actor.z_q32, actor.x_q32), id))
            .collect::<Vec<_>>();
        order.sort_unstable();
        for (_, unit) in order {
            let ready = self.cloak_ready(unit);
            let actor = self
                .actors
                .get_mut(&unit)
                .expect("actor identity is stable");
            let cloak = actor.cloak.as_mut().expect("filtered above");
            match cloak.state {
                CloakState::Leaving => {
                    cloak.exit += 1;
                    if cloak.exit >= cloak.exit_steps {
                        cloak.state = CloakState::None;
                        cloak.exit = 0;
                        actor.visibility = Visibility::Normal;
                    }
                }
                _ if !cloak.enabled => {}
                CloakState::None => {
                    if ready {
                        cloak.state = CloakState::Entering;
                    }
                }
                CloakState::Entering => {
                    if ready {
                        cloak.enter += 1;
                        if cloak.enter >= cloak.enter_steps {
                            cloak.state = CloakState::Cloaked;
                            cloak.enter = 0;
                            actor.visibility = Visibility::Disappear;
                        }
                    } else {
                        cloak.enter = 0;
                    }
                }
                CloakState::Cloaked => {
                    if !ready {
                        cloak.exit = 0;
                        cloak.state = CloakState::Leaving;
                    }
                }
            }
        }
    }

    /// `!IsMainSearcherSkillIdle() && IsNothingAround()`: the nearest unit the
    /// main skill's search measured, if any, stands beyond its range, edge to
    /// edge (`FightActor.Distance2D`). `IsMainSearcherSkillIdle` reads
    /// `FightSkillBase.IsIdle`, which nothing in a fight sets: a unit whose
    /// skill stands idle at the fight's start cloaks on its first update.
    fn cloak_ready(&self, unit: u64) -> bool {
        let actor = &self.actors[&unit];
        let main = SkillRef::main(FightActorRef::Unit(unit));
        let Some(nearest) = self.skill(main).nearest_actor.get() else {
            return true;
        };
        let Some(view) = self.fight_actor(nearest) else {
            return true;
        };
        let distance = target_edge_distance_q32(
            actor.x_q32,
            actor.z_q32,
            actor.rules.collision_radius(),
            view.x_q32,
            view.z_q32,
            view.radius,
        );
        distance > space_to_q32(self.main_attack_range(unit))
    }
}
