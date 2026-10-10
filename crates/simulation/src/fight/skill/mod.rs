mod around;
mod check;
mod extra;
mod group;
mod perform;
mod preemptive;
mod side_arm;

pub(in crate::fight) use perform::Launch;
pub(in crate::fight) use preemptive::{lock, unlock};

use super::*;

/// What `SkillAttackController` holds as its attack count while no attack
/// has begun: its constructor's and `Exit`'s value.
pub(in crate::fight) const ATTACK_COUNT_RESET: i32 = -1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct PendingRelease {
    pub(in crate::fight) step: u64,
    pub(in crate::fight) target: FightActorRef,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct PendingProjectileRelease {
    pub(in crate::fight) step: u64,
    pub(in crate::fight) target_kind: ObjectKind,
    pub(in crate::fight) target: u64,
    /// Where the burst aimed this projectile when it began: the target's
    /// position then, held within the skill's reach
    /// ([`Simulation::attack_position`]), plus the offset, and the height
    /// that point stands at.
    pub(in crate::fight) target_x_q32: i64,
    pub(in crate::fight) target_y_q32: i64,
    pub(in crate::fight) target_z_q32: i64,
    /// The offset alone. A projectile that follows its target lands it from
    /// where the target stands when the projectile is released: the second
    /// of a Phantom Ray's two projectiles, from where a charging Rhino stands
    /// 0.3 seconds on.
    pub(in crate::fight) offset_x_q32: i64,
    pub(in crate::fight) offset_z_q32: i64,
    /// Where the burst's target stood when it began, which each
    /// projectile's climb is measured to.
    pub(in crate::fight) climb_target: (i64, i64, i64),
    /// Whether it leaves for where its target stands as it is released, not
    /// where the burst aimed it: a burst with no target offset draws no
    /// points as it begins. A Centurion's second Homing Missile, a quarter
    /// second after the first, leaves for where its target stood the tick
    /// before, and the first for where it stood as the burst began.
    pub(in crate::fight) aims_at_release: bool,
    pub(in crate::fight) weapon_index: usize,
    /// The skill of the unit that fires it: a standalone weapon's own.
    pub(in crate::fight) skill_slot: usize,
}

/// Which `FightSkill` a skill is. The build makes one by the path its blow
/// takes: a strike is a plain `FightSkill`, a laser a `FightLaserSkill`, a
/// projectile skill a `FightProjectileSkill`, and a control beam a
/// `FightControllBeamSkill`, which no fight reaches yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum SkillKind {
    Strike,
    /// `SuicideEffect`: a blow that takes its own unit's life.
    Suicide,
    /// `FightAroundSkill`: a preemptive strike about its own unit.
    Around,
    /// `FightSupportSkill`: a preemptive skill whose blow does nothing.
    Support,
    Laser,
    Projectile,
    ControlBeam,
    Sweep,
}

impl SkillKind {
    pub(in crate::fight) const fn of(path: &AttackPath) -> Self {
        match path {
            AttackPath::Direct => Self::Strike,
            AttackPath::Suicide => Self::Suicide,
            AttackPath::Support => Self::Support,
            AttackPath::Around { .. } => Self::Around,
            AttackPath::Laser { .. } => Self::Laser,
            AttackPath::Projectile { .. } => Self::Projectile,
            AttackPath::ControlBeam { .. } => Self::ControlBeam,
            AttackPath::Sweep { .. } => Self::Sweep,
        }
    }
}

/// `SkillAttackController.attackPerformer`: what carries a blow out once
/// it is released.
#[derive(Debug, Clone)]
pub(in crate::fight) enum Performer {
    /// `NormalAttackPerformer`: a strike or a beam, which lands as the blow
    /// is released.
    Normal,
    /// A `ProjectileAttackPerformer`: a burst's projectiles, the first
    /// released with the blow and the rest still to come. The build splits
    /// it into a single and a multiple performer, which is not read.
    Projectile {
        pending: Vec<PendingProjectileRelease>,
        /// The burst's `EvenlyAllocatedAttackTargetPositionController`, for
        /// a skill whose row allocates its projectiles evenly.
        evenly: Option<Box<EvenlyAllocated>>,
    },
    /// A `SweepAttackPerformer` under way: the strip it sweeps and what it
    /// has struck.
    Sweep(Box<super::sweep::Sweep>),
}

impl Performer {
    /// The performer a skill of this kind starts with.
    pub(in crate::fight) const fn of(kind: SkillKind) -> Self {
        match kind {
            SkillKind::Projectile => Self::Projectile {
                pending: Vec::new(),
                evenly: None,
            },
            SkillKind::Strike
            | SkillKind::Suicide
            | SkillKind::Around
            | SkillKind::Support
            | SkillKind::Laser
            | SkillKind::ControlBeam
            | SkillKind::Sweep => Self::Normal,
        }
    }

    /// Whether the performer's work is done: no projectile of a burst left
    /// to release, no sweep under way.
    pub(in crate::fight) fn done(&self) -> bool {
        match self {
            Self::Normal => true,
            Self::Projectile { pending, .. } => pending.is_empty(),
            Self::Sweep(sweep) => sweep.over(),
        }
    }

    /// The burst's projectiles still to be released.
    pub(in crate::fight) fn pending(&self) -> &[PendingProjectileRelease] {
        match self {
            Self::Normal | Self::Sweep(_) => &[],
            Self::Projectile { pending, .. } => pending,
        }
    }

    /// Whether a sweep is under way: `SweepAttackPerformer.IsEnableCheckTarget`
    /// answers no until it is over, and an invalid target does not interrupt
    /// it (`IsInterruptedByInvalidTarget`).
    pub(in crate::fight) const fn sweeping(&self) -> bool {
        matches!(self, Self::Sweep(_))
    }

    /// Stops the burst: `StopAttack` ends a performer's work.
    pub(in crate::fight) fn stop(&mut self) {
        match self {
            Self::Projectile { pending, evenly } => {
                pending.clear();
                *evenly = None;
            }
            Self::Sweep(_) => *self = Self::Normal,
            Self::Normal => {}
        }
    }

    /// Takes out the projectiles due by this step, in order.
    pub(in crate::fight) fn take_due(&mut self, step: u64) -> Vec<PendingProjectileRelease> {
        let Self::Projectile { pending, .. } = self else {
            return Vec::new();
        };
        let mut due = Vec::new();
        pending.retain(|release| {
            if release.step <= step {
                due.push(*release);
                false
            } else {
                true
            }
        });
        due
    }
}

/// `ProjectileMultiAttackPerformer.EvenlyAllocatedAttackTargetPositionController`:
/// the units a burst shares its projectiles among, and the offsets drawn for
/// each as the burst began.
#[derive(Debug, Clone)]
pub(in crate::fight) struct EvenlyAllocated {
    /// `targets`: the units it fires at, the next one first. Each
    /// projectile takes the first that lives and puts it last.
    pub(in crate::fight) targets: Vec<u64>,
    /// `positionOffsets`: what is left of each unit's offsets, the next one
    /// first.
    pub(in crate::fight) offsets: BTreeMap<u64, Vec<(i64, i64)>>,
    /// `WeaponIndex`: the weapon the next projectile leaves, of the two.
    pub(in crate::fight) weapon: usize,
    /// `lastAttackPos`: where the last projectile was aimed, before its
    /// offset.
    pub(in crate::fight) last_attack: (i64, i64),
}

/// The skill's state, as `SkillStateController` holds it.
///
/// Every combination of phase, wind-up, backswing and cooling the kernel
/// used to carry apart is one of these, which the unit pairings were
/// counted against before they were folded: a wind-up and a backswing never
/// run together, and neither does anything else with a cooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum SkillState {
    /// `SkillIdleState`, with the step a finished attack's recovery runs
    /// until, if one still runs: the idle state waits it out before it
    /// starts another.
    Idle { ready_step: Option<u64> },
    /// `SkillPrepareState`, until its prepare time is over.
    Prepare { finish_step: u64 },
    /// `SkillAttackState`, and where in its blow it is.
    Attack(Blow),
    /// `SkillCoolingState`: when it began, and what the weapons still name.
    Cooling {
        started: u64,
        candidate: Option<FightActorRef>,
    },
    /// `SkillReloadingState`: a skill that fires from a magazine and has
    /// emptied it, until the step its reload is over.
    Reloading { finish_step: u64 },
    /// `SkillLockState`: a permanent preemptive skill not yet active, or the
    /// main skill one has replaced. It does not update.
    Locked,
}

/// Where `SkillAttackController` is in a blow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum Blow {
    /// No phase runs: the blow has not begun, or the last one is over.
    Waiting,
    /// The wait before the blow lands, and what it will land on.
    Before(PendingRelease),
    /// The backswing, until its last step.
    After { finish_step: u64 },
}

/// Whether an actor's update goes on to its next part or ends here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum Flow {
    Next,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum FightSkillPhase {
    Idle,
    Prepare { finish_step: u64 },
    Attack,
}

/// `SkillGroup`: a grouped unit's skills and what they share. The core
/// holds it; its siblings are the rest of `SkillGroup.skills`.
#[derive(Debug, Clone)]
pub(in crate::fight) struct Group {
    /// The core's siblings, slot 1 first.
    pub(in crate::fight) siblings: Vec<Skill>,
    /// The rotation of each sibling's weapon, slot 1 first, where the
    /// weapons are fixed to the body: the weapon's own transform, which
    /// outlives what its skill does.
    pub(in crate::fight) sibling_weapon_rotations_q32: Vec<i64>,
    /// What the unit's body is directed at: `FightSkill.ChangeLockTarget`
    /// hands the owner every lock a slot takes or drops, so it is the latest,
    /// the core's or a sibling's.
    pub(in crate::fight) mech_lock: Option<FightActorRef>,
    /// The update the core last started a blow on:
    /// `SkillGroup.OnStartPerformAttack`, which a fusillade's siblings wait
    /// for before they may fire.
    pub(in crate::fight) core_blow_step: Option<u64>,
    pub(in crate::fight) behaviour: GroupBehaviour,
    /// `MechSearchTargetController.searchTargetTime`, for a unit that
    /// searches for itself (`MechData.isEnableMechSearchTarget`): its own
    /// search writes `mech_lock`, and no skill hands it one
    /// (`FightSkill.ChangeLockTarget` asks `IsMechSearchTargetEnabled`).
    pub(in crate::fight) mech_search_time: Option<i32>,
    /// For a batch of standalone weapons, the slot whose skill is the
    /// motion's attacker (`MotionController.attacker`): the first as the
    /// unit is made (`SetMotionAttackerAfterSkill` takes `GetSkills()[0]`),
    /// then whichever searched last while the one holding it held no lock.
    pub(in crate::fight) motion_slot: usize,
    /// Which siblings are a joined row's, slot 1 first, with the row's own
    /// numbers; `None` for a slot of the main row. A joined row's slot is its
    /// row's `FightSkill`, not the main skill's: it reaches its own range
    /// beyond its parent's rather than ten metres, its attack angle is its
    /// row's, and its projectile climbs first, as every extra skill's does.
    pub(in crate::fight) joined: Vec<Option<JoinedSlot>>,
}

/// What a joined row's slot holds of its own row (`FightSkill.Init`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct JoinedSlot {
    /// The row's own range, in space units, beyond its parent's.
    pub(in crate::fight) range: i64,
    /// The row's attack angle, or the whole circle where the row sets none,
    /// in millidegrees either side.
    pub(in crate::fight) half_angle_mdeg: i64,
    /// The technology whose row it is, which switches it with the row's
    /// other skills (`ExtraSkillProvider.DisableSkill`).
    pub(in crate::fight) technology: i32,
}

/// `FightSkill.GetAttackRange` of a main row's grouped slot: ten metres
/// beyond its parent's, hard-coded.
pub(in crate::fight) const MAIN_SLOT_RANGE_ADDEND: i64 = 10_000;

/// `SkillGroup.attackBehaviour`: how the group's skills take turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum GroupBehaviour {
    /// `GroupedSkillAttackBehaviour`: each skill attacks on its own.
    Each,
    /// A `GroupedSkillFusilladeBehaviour`: the siblings fire with the core.
    Fusillade,
    /// No `SkillGroup` at all: a `FightSkillBatch` of standalone weapons,
    /// each skill the unit's main skill, searching and attacking on its own.
    Standalone,
}

/// `SkillManager`: the skills an owner runs, a unit's or a construction's
/// alike. Its main skill is the one its data names; an extra skill is one a
/// source such as an extra weapon technology adds beside it
/// (`ExtraSkillSystem.AddMech`), kept in ascending skill ID.
#[derive(Debug, Clone)]
pub(in crate::fight) struct SkillManager {
    /// `SkillManager.mainSkill`.
    pub(in crate::fight) main: Skill,
    /// `SkillManager.extraSkills`.
    pub(in crate::fight) extras: Vec<ExtraSkill>,
    /// `PreemptiveSkillController.isPermanentPreemptiveSkillSet`: the
    /// permanent preemptive skill has taken the main skill's place.
    pub(in crate::fight) preemptive_active: bool,
    /// `PreemptiveSkillController.preemptiveSkill` of a skill that is not
    /// permanent: the extra skill that has the main skill locked while it
    /// attacks.
    pub(in crate::fight) running_preemptive: Option<usize>,
}

impl SkillManager {
    /// A tick on which no skill updates and nothing else moves its schedule:
    /// only the clock a recording reads stands still.
    pub(in crate::fight) fn hold_attack_clocks(&mut self) {
        let skills = std::iter::once(&mut self.main)
            .chain(self.extras.iter_mut().map(|extra| &mut extra.skill));
        for skill in skills {
            skill.attack_time_anchor += 1;
            skill.hold_cooling();
            for sibling in skill.siblings_mut() {
                sibling.attack_time_anchor += 1;
                sibling.hold_cooling();
            }
        }
    }

    /// A unit rising where it fell enters the fight with every skill's
    /// `attackTime` at its interval: the first blow is due at once.
    pub(in crate::fight) fn ready_attack_clocks(&mut self, step: u64) {
        let step = i64::try_from(step).unwrap_or(i64::MAX);
        let skills = std::iter::once(&mut self.main)
            .chain(self.extras.iter_mut().map(|extra| &mut extra.skill));
        for skill in skills {
            skill.attack_time_anchor =
                step - i64::try_from(skill.current_attack_interval).unwrap_or(i64::MAX);
            for sibling in skill.siblings_mut() {
                sibling.attack_time_anchor =
                    step - i64::try_from(sibling.current_attack_interval).unwrap_or(i64::MAX);
            }
        }
    }

    /// A tick on which no skill updates: `FightSkill.Update` adds nothing
    /// to any skill's `attackTime`, so the attack each is waiting for comes
    /// a tick later. A Sandworm's interval stands still while it burrows and
    /// surfaces.
    pub(in crate::fight) fn pause_attack_intervals(&mut self) {
        let skills = std::iter::once(&mut self.main)
            .chain(self.extras.iter_mut().map(|extra| &mut extra.skill));
        for skill in skills {
            skill.next_attack_step = skill.next_attack_step.saturating_add(1);
            skill.attack_time_anchor += 1;
            skill.hold_cooling();
            for sibling in skill.siblings_mut() {
                sibling.next_attack_step = sibling.next_attack_step.saturating_add(1);
                sibling.attack_time_anchor += 1;
                sibling.hold_cooling();
            }
        }
    }
}

/// One `FightSkill` an extra weapon technology adds (`ExtraSkillSystem.AddMech`):
/// `SkillManager.AddSkill` adds every skill its row makes, one for each weapon
/// of a standalone row.
#[derive(Debug, Clone)]
pub(in crate::fight) struct ExtraSkill {
    pub(in crate::fight) skill: Skill,
    /// The technology's row, which the skill is run from.
    pub(in crate::fight) rules: ExtraWeaponConfig,
    /// The terrain its hit leaves, a fire or an oil, if its row leaves one.
    pub(in crate::fight) terrain: Option<crate::layout::TerrainSpec>,
    /// The buff its hit writes on what it struck.
    pub(in crate::fight) buff: Option<crate::layout::SkillBuff>,
    /// The fire its unit's death leaves, for an explosion.
    pub(in crate::fight) dead_fire: Option<crate::layout::TerrainSpec>,
    /// What its own `DataSet` holds, for a skill without a damage rate.
    pub(in crate::fight) skill_corrections: Vec<crate::data::Entry>,
    /// The first of the row's weapons this skill fires: the first of its own
    /// for a standalone row, the first of every other.
    pub(in crate::fight) weapon: usize,
    /// `FightRocketPunchSkill.curAttackCount`: the punches it has thrown
    /// this fight (`OnAttack`), which `ExitFight` sets back to none.
    pub(in crate::fight) punches: u32,
}

impl SkillManager {
    pub(in crate::fight) const fn new(main: Skill) -> Self {
        Self {
            main,
            extras: Vec::new(),
            preemptive_active: false,
            running_preemptive: None,
        }
    }

    /// How many of `GetSkills()`' slots the main skill holds: one, or one
    /// for each skill of its group.
    pub(in crate::fight) fn main_slots(&self) -> usize {
        self.main.group_size().max(1)
    }

    /// The first of `GetSkills()`' slots an extra skill holds: after the
    /// main skill's and every earlier extra skill's, one each or one for
    /// each skill of its group.
    pub(in crate::fight) fn extra_first_slot(&self, index: usize) -> usize {
        self.main_slots()
            + self.extras[..index]
                .iter()
                .map(|extra| extra.skill.group_size().max(1))
                .sum::<usize>()
    }

    /// The skill holding a slot of `GetSkills()`, and which of its group's
    /// skills the slot is: the main skill's for a slot among its group's, an
    /// extra skill's after them.
    pub(in crate::fight) fn at_slot(&self, slot: usize) -> (SkillSlot, usize) {
        let main_slots = self.main_slots();
        if slot < main_slots {
            return (SkillSlot::Main, slot);
        }
        let mut first = main_slots;
        for (index, extra) in self.extras.iter().enumerate() {
            let held = extra.skill.group_size().max(1);
            if slot < first + held {
                return (SkillSlot::Extra(index), slot - first);
            }
            first += held;
        }
        (SkillSlot::Extra(slot - first + self.extras.len()), 0)
    }

    pub(in crate::fight) fn get(&self, slot: SkillSlot) -> &Skill {
        match slot {
            SkillSlot::Main => &self.main,
            SkillSlot::Extra(index) => &self.extras[index].skill,
        }
    }

    pub(in crate::fight) fn get_mut(&mut self, slot: SkillSlot) -> &mut Skill {
        match slot {
            SkillSlot::Main => &mut self.main,
            SkillSlot::Extra(index) => &mut self.extras[index].skill,
        }
    }
}

impl ExtraSkill {
    /// The arc one of its weapons turns within, if its row gives one.
    pub(in crate::fight) fn arc(&self, offset: usize) -> Option<&WeaponArc> {
        self.rules
            .attack
            .weapons
            .arcs
            .as_ref()
            .and_then(|arcs| arcs.get(self.weapon + offset))
    }

    /// Whether its weapon turns on a transform of its own: one that turns
    /// within an arc, and a standalone row's or a side arm's, which
    /// `FightWeapon` gives a transform whatever its arc, turning freely where
    /// it has none: a side arm's weapon whose arc is no wider than its rest
    /// is `RotateType.Free`, not `Fixed`.
    pub(in crate::fight) fn own_transform(&self) -> bool {
        self.rules.attack.weapons.arcs.is_some()
            || matches!(
                self.rules.attack.weapons.mode,
                crate::rules::WeaponMode::Standalone | crate::rules::WeaponMode::SideArm
            )
    }
}

/// Which of an owner's skills: its `SkillManager`'s main skill, or the extra
/// skill at an index of its `extraSkills`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::fight) enum SkillSlot {
    Main,
    Extra(usize),
}

/// One skill of one owner: what the skill machine runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::fight) struct SkillRef {
    pub(in crate::fight) owner: FightActorRef,
    pub(in crate::fight) slot: SkillSlot,
}

impl SkillRef {
    /// An owner's main skill.
    pub(in crate::fight) const fn main(owner: FightActorRef) -> Self {
        Self {
            owner,
            slot: SkillSlot::Main,
        }
    }
}

/// `FightSkill`: the lock and what the weapons fire at, the state the skill
/// is in, and the attack it is making.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a separate field of `FightSkill` or its controllers"
)]
#[derive(Debug, Clone)]
pub(in crate::fight) struct Skill {
    pub(in crate::fight) weapon_rotations_q32: Vec<i64>,
    pub(in crate::fight) next_attack_step: u64,
    /// The interval this cycle was scheduled with, in logic ticks: the
    /// description plus the stagger drawn for it. The build keeps the same
    /// thing in `FightSkill.attackInterval` and answers it from
    /// `GetCurrentAttackInterval`, and a recording carries it so the two can
    /// be compared. Before a unit's first attack it is the description with
    /// the draw its deployment took.
    pub(in crate::fight) current_attack_interval: u64,
    /// The step `FightSkill.attackTime` counts from: it is the steps since,
    /// which every update adds one to, a blow starting sets to nothing, and
    /// a refresh of the interval sets to the interval. A recording carries
    /// `attackTime`; the attack itself is scheduled by `next_attack_step`.
    pub(in crate::fight) attack_time_anchor: i64,
    /// What the mech's body is directed at: the target its search found, which
    /// it moves toward and which a unit with a body keeps facing while it
    /// attacks. A recording carries it as `mech_lock_target`.
    ///
    /// It is not always what the weapons fire at: [`Actor::attack_target`] is,
    /// and the two part company when an enemy construction stands in the line
    /// of fire.
    pub(in crate::fight) lock_target: Option<FightActorRef>,
    /// The enemy construction in the line of fire, and the lock it was found
    /// for.
    ///
    /// `FightSkill.SearchAttackTarget` asks `WallConstructionTargetChecker`
    /// wherever the skill asks what to fire at, and hands the block to the
    /// weapons while the mech keeps its lock. The pairing is what keeps this honest: once
    /// `lock_target` is anything but the lock it was found for, the block no
    /// longer answers, without anyone having to clear it.
    pub(in crate::fight) in_the_way: Option<(u64, FightActorRef)>,
    /// The attack target `FightSkill.SearchLockTarget` left in place as it
    /// changed the lock, and the lock it changed to: until the next
    /// `SearchAttackTarget`, the skill fires at what it fired at. Paired as
    /// `in_the_way` is, it answers only while that lock holds.
    pub(in crate::fight) kept_attack_target: Option<(FightActorRef, FightActorRef)>,
    /// The battlefield shield covering the lock, and the lock it was found
    /// for: `FightSkill.targetEnergyShield`, which
    /// `SkillSearchTargetController.SearchTargetShield` hands the skill in
    /// place of an attack target while the mech keeps its lock.
    pub(in crate::fight) target_shield: Option<(u64, FightActorRef)>,
    pub(in crate::fight) search_target_time: i32,
    pub(in crate::fight) searched_this_tick: bool,
    /// `SkillSearchTargetController.nearestActor`: what each selector the
    /// controller asks writes back, `TrySelect`, `Select` and `PerformSearch`
    /// alike, the candidate it measured nearest (`Selector.Calculate`), none
    /// when it had none. The selectors take the fight by reference, as the
    /// build's write their out parameter.
    pub(in crate::fight) nearest_actor: std::cell::Cell<Option<FightActorRef>>,
    /// Whether `FightCoreSystem.PreCalculate` put this skill among the
    /// attackers it prepared at the tick's start. `TrySelect` answers only for
    /// one of them; any other search is `PerformSearch`. See
    /// [`Simulation::search_prepared`].
    pub(in crate::fight) search_prepared: bool,
    /// The step a bodyless skill whose motion already attacks started its
    /// attack state on, from its own update: the state is not updated on the
    /// tick it is entered, so its first blow waits for the next.
    pub(in crate::fight) started_from_idle: Option<u64>,
    /// Which `SkillStateController` state the skill is in, with what that
    /// state carries.
    pub(in crate::fight) state: SkillState,
    /// The unit's `SkillGroup`, held by its core: none for a skill that is
    /// not grouped, and a group of the core alone for a Vortex.
    pub(in crate::fight) group: Option<Group>,
    /// Whether `ChangeLockTarget` wrote this skill's lock on the current
    /// update, whether or not the lock it wrote is new: a grouped core's
    /// search that finds the unit it already holds still hands it to the
    /// owner.
    pub(in crate::fight) lock_written: bool,
    /// Not `FightSkill.isEnable`: an extra skill whose technology a buff
    /// switched off (`ExtraSkillProvider.DisableSkill`, `FightSkill.Disable`).
    pub(in crate::fight) disabled: bool,
    pub(in crate::fight) kind: SkillKind,
    pub(in crate::fight) performer: Performer,
    /// `SkillAttackController.attackCount`: the blows started since the
    /// skill entered its attack state, less one. The constructor and
    /// leaving the attack state set it to [`ATTACK_COUNT_RESET`], and each
    /// blow the attack point lets through adds one before it lands, so a
    /// blow reads its own index: a laser's damage multiplier is the one at
    /// that index.
    pub(in crate::fight) attack_count: i32,
    /// `SkillAttackController.totalAttackCount`: every blow started this
    /// fight (`ResetInFightAttackData` sets it to nothing as the fight
    /// starts), which leaving the attack state does not reset.
    pub(in crate::fight) total_attack_count: i32,
    /// `SkillAttackController.performCount`: the blows whose cycle ran out,
    /// backswing and all (`ChangeToIdle`), since the skill entered its
    /// attack state; leaving it (`Exit`) clears it.
    pub(in crate::fight) perform_count: u32,
    /// A blow with no backswing whose attacking phase is still under way:
    /// `SkillAttackController.ChangeToIdle` counts it in `performCount` once
    /// the performer's work is done, the last projectile of its burst
    /// released or its sweep over.
    pub(in crate::fight) attacking_unfinished: bool,
    /// Whether a burst was still releasing as the skill's last update began
    /// (`ProjectileMultiAttackPerformer.IsEnableCheckTarget` answering no):
    /// the update its last projectile leaves still names its target.
    pub(in crate::fight) burst_releasing: bool,
    /// The `AttackCountEffectLinker` a technology hands the main skill
    /// (`FightSkill.AddAttackCountEffectLinker`).
    pub(in crate::fight) attack_count_linker: Option<super::attack_count::AttackCountLinker>,
    /// The rounds left in a skill that fires from a magazine
    /// (`SkillData.isLoadingType`), and none for one that does not.
    pub(in crate::fight) rounds: Option<u32>,
    /// What the weapon still fires at once its lock is gone:
    /// `FightSkill.attackTarget` is kept apart from `lockTarget`, and a
    /// grouped core taking a sibling's unit calls the sibling's
    /// `ChangeLockTarget(null)`, which leaves it until the sibling searches
    /// again, as `StopAttack` does when a move ability stops the skills.
    pub(in crate::fight) attack_target_left: Option<FightActorRef>,
    /// `FightSkillBase.IsIdle`: the last lock search found nothing the skill
    /// can fire at and fell back on any live enemy
    /// ([`Simulation::select_alive_target`]). `FightSkill.SearchLockTarget`
    /// then clears what the skill fires at, so the lock is only where the
    /// mech goes.
    pub(in crate::fight) idle: bool,
    /// A main skill's `shouldSideArmFire`: the turn is its side arm's.
    pub(in crate::fight) side_arm_fires: bool,
    /// A main skill's `sideArmFireDelayTicks`: the ticks its side arm waits
    /// in its turn before it may begin a blow.
    pub(in crate::fight) side_arm_fire_delay: u64,
    /// `FightSkill.prevLockTargetForSideArm`: the lock `ChangeLockTarget`
    /// last changed from, which a side arm searches about while the main
    /// skill holds none.
    pub(in crate::fight) prev_lock_for_side_arm: Option<FightActorRef>,
    /// The effect a control beam's attack holds, while it attacks
    /// (`NormalAttackPerformer`'s effect).
    pub(in crate::fight) beam: Option<super::control::Beam>,
}

impl Skill {
    /// A skill entering the fight: idle, no lock, nothing scheduled.
    pub(in crate::fight) fn new(
        weapon_rotations_q32: Vec<i64>,
        group: Option<(usize, GroupBehaviour)>,
        magazine: Option<Magazine>,
        kind: SkillKind,
    ) -> Self {
        let performer = Performer::of(kind);
        let group = group.map(|(skills, behaviour)| Group {
            siblings: (1..skills).map(|_| Self::sibling_entering(kind)).collect(),
            sibling_weapon_rotations_q32: vec![0; skills.saturating_sub(1)],
            mech_lock: None,
            core_blow_step: None,
            behaviour,
            mech_search_time: None,
            motion_slot: 0,
            joined: Vec::new(),
        });
        Self {
            weapon_rotations_q32,
            next_attack_step: 0,
            current_attack_interval: 0,
            attack_time_anchor: 0,
            lock_target: None,
            in_the_way: None,
            kept_attack_target: None,
            target_shield: None,
            // FightSkill owns a second SearchTargetController. FightPrepareState
            // replaces this constructor value with the presearch batch ordinal.
            search_target_time: SEARCH_TARGET_RESET_TICKS,
            searched_this_tick: false,
            nearest_actor: std::cell::Cell::new(None),
            search_prepared: false,
            started_from_idle: None,
            state: SkillState::Idle { ready_step: None },
            group,
            lock_written: false,
            disabled: false,
            kind,
            performer,
            attack_count: ATTACK_COUNT_RESET,
            total_attack_count: 0,
            perform_count: 0,
            attacking_unfinished: false,
            burst_releasing: false,
            attack_count_linker: None,
            rounds: magazine.map(|magazine| magazine.capacity),
            attack_target_left: None,
            idle: false,
            side_arm_fires: false,
            side_arm_fire_delay: 0,
            prev_lock_for_side_arm: None,
            beam: None,
        }
    }

    /// A grouped core's sibling as it enters the fight, and as leaving the
    /// fight or failing a check leaves it: idle, with no lock and nothing
    /// scheduled. Its weapon's pose and its group live on the core.
    pub(in crate::fight) fn sibling_entering(kind: SkillKind) -> Self {
        Self {
            search_target_time: 0,
            ..Self::new(Vec::new(), None, None, kind)
        }
    }

    /// `SkillAttackController.performCount` above zero: a blow of this
    /// attack has been performed, which `PerformAttack` counts as the blow
    /// starts.
    pub(in crate::fight) const fn performed(&self) -> bool {
        self.attack_count > ATTACK_COUNT_RESET
    }

    /// A blow performed takes a round from the magazine, if the skill has one.
    pub(in crate::fight) fn fire_round(&mut self) {
        if let Some(rounds) = &mut self.rounds {
            *rounds = rounds.saturating_sub(1);
        }
    }

    /// `SkillAttackState.TryPerformAttack` once the interval is up: schedules
    /// the next attack after the interval drawn for it, and winds up a blow
    /// that lands after the attack point.
    pub(in crate::fight) fn schedule_blow(
        &mut self,
        step: u64,
        interval: u64,
        attack_point_steps: u64,
        target: FightActorRef,
    ) {
        self.next_attack_step = step.saturating_add(interval);
        self.current_attack_interval = interval;
        self.attack_time_anchor = i64::try_from(step).unwrap_or(i64::MAX);
        if let Some(group) = &mut self.group {
            group.core_blow_step = Some(step);
        }
        self.set_pending(Some(PendingRelease {
            step: step.saturating_add(attack_point_steps),
            target,
        }));
    }

    /// `SkillManager.UpdateWeaponRotateion`: every weapon turns towards a
    /// bearing, by at most one update's turn.
    pub(in crate::fight) fn turn_weapons_towards(&mut self, bearing_q32: i64, turn_q32: i64) {
        for rotation in &mut self.weapon_rotations_q32 {
            *rotation = rotate_towards_q32(*rotation, bearing_q32, turn_q32);
        }
    }

    /// The coarse phase: idle (a cooling reads idle), preparing, attacking.
    pub(in crate::fight) const fn phase(&self) -> FightSkillPhase {
        match self.state {
            SkillState::Idle { .. }
            | SkillState::Cooling { .. }
            | SkillState::Reloading { .. }
            | SkillState::Locked => FightSkillPhase::Idle,
            SkillState::Prepare { finish_step } => FightSkillPhase::Prepare { finish_step },
            SkillState::Attack(_) => FightSkillPhase::Attack,
        }
    }

    /// Moves to a coarse phase, carrying a running backswing across, as the
    /// fields this replaces did.
    pub(in crate::fight) fn set_phase(&mut self, phase: FightSkillPhase) {
        self.enter(match (phase, self.state) {
            (FightSkillPhase::Idle, SkillState::Attack(Blow::After { finish_step })) => {
                SkillState::Idle {
                    ready_step: Some(finish_step),
                }
            }
            (
                FightSkillPhase::Idle,
                state @ (SkillState::Idle { .. } | SkillState::Cooling { .. }),
            )
            | (FightSkillPhase::Attack, state @ SkillState::Attack(_)) => state,
            (FightSkillPhase::Idle, _) => SkillState::Idle { ready_step: None },
            (FightSkillPhase::Prepare { finish_step }, state) => {
                debug_assert!(
                    !matches!(
                        state,
                        SkillState::Idle {
                            ready_step: Some(_)
                        }
                    ),
                    "a skill prepares while recovering: {state:?}"
                );
                SkillState::Prepare { finish_step }
            }
            (
                FightSkillPhase::Attack,
                SkillState::Idle {
                    ready_step: Some(finish_step),
                },
            ) => SkillState::Attack(Blow::After { finish_step }),
            (FightSkillPhase::Attack, state) => {
                debug_assert!(
                    !matches!(state, SkillState::Cooling { .. }),
                    "a cooling skill attacks: {state:?}"
                );
                SkillState::Attack(Blow::Waiting)
            }
        });
    }

    /// Moves `SkillStateController` to a state. Leaving the attack state is
    /// `SkillAttackState.Exit`, which runs `SkillAttackController.Exit`: the
    /// attack count goes back to [`ATTACK_COUNT_RESET`], whatever took the
    /// skill out of its attack.
    pub(in crate::fight) fn enter(&mut self, state: SkillState) {
        if matches!(self.state, SkillState::Attack(_)) && !matches!(state, SkillState::Attack(_)) {
            self.attack_count = ATTACK_COUNT_RESET;
            self.perform_count = 0;
            self.attacking_unfinished = false;
        }
        if matches!(self.state, SkillState::Attack(Blow::After { .. }))
            && state == SkillState::Attack(Blow::Waiting)
        {
            self.perform_count += 1;
        }
        self.state = state;
    }

    /// `SkillAttackController.performCount` at a step: the blows whose cycle
    /// has run out. The backswing's controller hands back on its last step,
    /// where `ChangeToIdle` counts the blow, while this simulator leaves the
    /// backswing on the update after: a Rhino whose backswing ends on tick
    /// 107 reads 1 there.
    pub(in crate::fight) fn performed_count(&self, step: u64) -> u32 {
        self.perform_count
            + u32::from(matches!(
                self.state,
                SkillState::Attack(Blow::After { finish_step }) if step >= finish_step
            ))
    }

    /// The blow being wound up, if one is.
    pub(in crate::fight) const fn pending(&self) -> Option<PendingRelease> {
        match self.state {
            SkillState::Attack(Blow::Before(pending)) => Some(pending),
            _ => None,
        }
    }

    pub(in crate::fight) const fn pending_mut(&mut self) -> Option<&mut PendingRelease> {
        match &mut self.state {
            SkillState::Attack(Blow::Before(pending)) => Some(pending),
            _ => None,
        }
    }

    pub(in crate::fight) fn set_pending(&mut self, pending: Option<PendingRelease>) {
        match (pending, self.state) {
            (Some(pending), SkillState::Attack(_)) => {
                self.state = SkillState::Attack(Blow::Before(pending));
            }
            (Some(_), state) => panic!("a blow is wound up outside an attack: {state:?}"),
            (None, SkillState::Attack(Blow::Before(_))) => {
                self.state = SkillState::Attack(Blow::Waiting);
            }
            (None, _) => {}
        }
    }

    /// The last step of a running backswing, in an attack or waited out in
    /// idle.
    pub(in crate::fight) const fn backswing_finish_step(&self) -> Option<u64> {
        match self.state {
            SkillState::Attack(Blow::After { finish_step })
            | SkillState::Idle {
                ready_step: Some(finish_step),
            } => Some(finish_step),
            _ => None,
        }
    }

    pub(in crate::fight) fn set_backswing_finish_step(&mut self, finish_step: Option<u64>) {
        self.enter(match (finish_step, self.state) {
            (Some(finish_step), SkillState::Attack(_)) => {
                SkillState::Attack(Blow::After { finish_step })
            }
            (Some(finish_step), SkillState::Idle { .. }) => SkillState::Idle {
                ready_step: Some(finish_step),
            },
            (Some(_), state) => panic!("a backswing runs outside an attack or idle: {state:?}"),
            (None, SkillState::Attack(Blow::After { .. })) => SkillState::Attack(Blow::Waiting),
            (None, SkillState::Idle { .. }) => SkillState::Idle { ready_step: None },
            (None, state) => state,
        });
    }

    /// A tick its state does not update: a cooling under way ends a tick
    /// later. A Fang cooling as the fight is decided is still cooling when
    /// the fight is left.
    fn hold_cooling(&mut self) {
        if let SkillState::Cooling { started, .. } = &mut self.state {
            *started = started.saturating_add(1);
        }
    }

    /// When the cooling began, and what the weapons name through it.
    pub(in crate::fight) const fn cooling(&self) -> Option<(u64, Option<FightActorRef>)> {
        match self.state {
            SkillState::Cooling { started, candidate } => Some((started, candidate)),
            _ => None,
        }
    }

    /// Starts, updates or ends a cooling; ending it leaves the skill idle.
    pub(in crate::fight) fn set_cooling(&mut self, cooling: Option<(u64, Option<FightActorRef>)>) {
        self.enter(match (cooling, self.state) {
            (Some((started, candidate)), _) => SkillState::Cooling { started, candidate },
            (None, SkillState::Cooling { .. }) => SkillState::Idle { ready_step: None },
            (None, state) => state,
        });
    }
}

impl Skill {
    /// The shield the skill fires at in place of its lock, while the lock is
    /// the one it was found for.
    pub(in crate::fight) fn shield_target(&self) -> Option<u64> {
        if self.idle {
            return None;
        }
        match self.target_shield {
            Some((shield, found_for)) if self.lock_target == Some(found_for) => Some(shield),
            _ => None,
        }
    }

    /// What this actor's weapons fire at: the construction in the way if one
    /// stands there for the current lock, the lock itself otherwise, and
    /// nothing while the skill is [`Skill::idle`].
    ///
    /// Range, attack angle, release and the question of whether the target
    /// is still alive are all asked of this. Where to move and where a body
    /// faces are asked of `lock_target`.
    pub(in crate::fight) fn attack_target(&self) -> Option<FightActorRef> {
        if self.idle {
            return None;
        }
        self.checked_attack_target()
    }

    /// What `FightSkill.SearchAttackTarget` leaves the skill firing at, idle
    /// or not: the construction in the way of the lock, or the lock. A
    /// failed check ends its search with it (`CheckWhenLoseTarget`), so an
    /// attack a check ends hands it to the cooling even when the lock came
    /// from `TrySearchAliveTarget`, which leaves the skill idle.
    pub(in crate::fight) fn checked_attack_target(&self) -> Option<FightActorRef> {
        if let Some((kept, under)) = self.kept_attack_target
            && self.lock_target == Some(under)
        {
            return Some(kept);
        }
        match self.in_the_way {
            Some((building, found_for)) if self.lock_target == Some(found_for) => {
                Some(FightActorRef::Building(building))
            }
            _ => self.lock_target,
        }
    }

    /// Drops the mech's target, and every grouped slot with it.
    ///
    /// A group whose mech holds no target holds no slots: every time a Wraith
    /// was recorded losing its lock — to a block it was shooting falling, and
    /// to the last enemy dying — all four slots read empty the same tick, and
    /// the children were allocated again only once the core was attacking,
    /// the usual eight ticks later. Nothing changes for a unit without a
    /// group, whose slot lists are empty.
    pub(in crate::fight) fn drop_lock(&mut self) {
        self.write_lock(None);
        self.set_mech_lock(None);
    }

    /// Every sibling slot left idle, with no allocation and nothing
    /// scheduled, as leaving the fight leaves them; each keeps its interval
    /// and its clock.
    pub(in crate::fight) fn clear_slots(&mut self) {
        self.clear_slots_cooling_before(None);
    }

    /// [`Self::clear_slots`] as a won fight runs on: a slot already cooling
    /// before `step` goes on cooling at what it named, its lock let go, as
    /// any skill does then: a Raiden's second gun cooling as the fight is
    /// decided still names the Fang it fired at.
    pub(in crate::fight) fn clear_slots_cooling_before(&mut self, step: Option<u64>) {
        let kind = self.kind;
        for sibling in self.siblings_mut() {
            if step.is_some_and(|step| sibling.cooling().is_some_and(|(started, _)| started < step))
            {
                sibling.drop_lock();
                sibling.attack_target_left = None;
                continue;
            }
            *sibling = Self {
                current_attack_interval: sibling.current_attack_interval,
                attack_time_anchor: sibling.attack_time_anchor,
                beam: sibling.beam,
                ..Self::sibling_entering(kind)
            };
        }
    }

    /// `FightSkill.ChangeLockTarget`.
    pub(in crate::fight) fn write_lock(&mut self, lock: Option<FightActorRef>) {
        if lock.is_some() {
            self.attack_target_left = None;
        }
        if lock != self.lock_target && self.lock_target.is_some() {
            self.prev_lock_for_side_arm = self.lock_target;
        }
        self.lock_target = lock;
        self.lock_written = true;
    }

    pub(in crate::fight) const fn is_grouped(&self) -> bool {
        self.group.is_some()
    }

    /// What the motion follows: the skill's attack target, or for a batch
    /// of standalone weapons the attack target of its first weapon holding a
    /// lock (`FightSkillBatch.GetLockTarget`): a Mountain whose first gun
    /// cools walks on what another holds.
    pub(in crate::fight) fn batch_attack_target(&self) -> Option<FightActorRef> {
        if !self.standalone() {
            return self.attack_target();
        }
        (0..self.group_size())
            .find(|&slot| self.slot_lock(slot).is_some())
            .and_then(|slot| self.group_attack_target(slot))
    }

    /// A batch of standalone weapons, which no `SkillGroup` holds together.
    pub(in crate::fight) fn standalone(&self) -> bool {
        self.group
            .as_ref()
            .is_some_and(|group| group.behaviour == GroupBehaviour::Standalone)
    }

    /// How many `FightSkill`s the group holds, the core among them; zero for
    /// a skill that is not grouped.
    pub(in crate::fight) fn group_size(&self) -> usize {
        self.group
            .as_ref()
            .map_or(0, |group| group.siblings.len() + 1)
    }

    /// Whether the group fires its siblings with its core.
    pub(in crate::fight) fn fusillade(&self) -> bool {
        self.group
            .as_ref()
            .is_some_and(|group| group.behaviour == GroupBehaviour::Fusillade)
    }

    /// A joined row's own numbers for a slot of it, `None` for a slot of the
    /// main row or a skill no row joined.
    pub(in crate::fight) fn joined(&self, slot: usize) -> Option<JoinedSlot> {
        slot.checked_sub(1).and_then(|sibling| {
            self.group
                .as_ref()
                .and_then(|group| group.joined.get(sibling).copied().flatten())
        })
    }

    /// The core's siblings, none for a skill that is not grouped.
    pub(in crate::fight) fn siblings(&self) -> &[Self] {
        self.group.as_ref().map_or(&[], |group| &group.siblings)
    }

    pub(in crate::fight) fn siblings_mut(&mut self) -> &mut [Self] {
        self.group
            .as_mut()
            .map_or(&mut [], |group| &mut group.siblings)
    }

    /// The rotation of a sibling's weapon fixed to the body.
    pub(in crate::fight) fn sibling_weapon_rotation_q32(&self, slot: usize) -> i64 {
        self.group
            .as_ref()
            .expect("only a grouped skill has siblings")
            .sibling_weapon_rotations_q32[slot - 1]
    }

    /// Hands the owner a lock, where a group keeps one for it and the unit
    /// does not search for itself.
    pub(in crate::fight) fn set_mech_lock(&mut self, lock: Option<FightActorRef>) {
        if let Some(group) = &mut self.group
            && group.mech_search_time.is_none()
        {
            group.mech_lock = lock;
        }
    }

    /// Whether the unit searches for itself (`MechSearchTargetController`).
    pub(in crate::fight) fn mech_searches(&self) -> bool {
        self.group
            .as_ref()
            .is_some_and(|group| group.mech_search_time.is_some())
    }

    /// `RefreshAttackData` from the core to every sibling: each is due when
    /// the core is.
    pub(in crate::fight) fn align_slots_to_core(&mut self, siblings_to_update: bool) {
        // `RefreshAttackData(core, true)`: the core's interval and clock. In
        // the core's update, each sibling's own update this tick then adds
        // one to it.
        let (due, interval, anchor) = (
            self.next_attack_step,
            self.current_attack_interval,
            self.attack_time_anchor,
        );
        for sibling in self.siblings_mut() {
            sibling.next_attack_step = due;
            sibling.current_attack_interval = interval;
            sibling.attack_time_anchor = anchor - i64::from(siblings_to_update);
        }
    }

    /// A skill of the group, `SkillGroup.skills`: the core is the first,
    /// its siblings follow.
    pub(in crate::fight) fn group_skill(&self, slot: usize) -> &Self {
        if slot == 0 {
            self
        } else {
            &self.siblings()[slot - 1]
        }
    }

    pub(in crate::fight) fn group_skill_mut(&mut self, slot: usize) -> &mut Self {
        if slot == 0 {
            self
        } else {
            &mut self.siblings_mut()[slot - 1]
        }
    }

    /// A slot of the group, the core's siblings only.
    pub(in crate::fight) fn sibling(&self, slot: usize) -> &Self {
        &self.siblings()[slot - 1]
    }

    pub(in crate::fight) fn sibling_mut(&mut self, slot: usize) -> &mut Self {
        &mut self.siblings_mut()[slot - 1]
    }

    /// What a slot has locked.
    pub(in crate::fight) fn slot_lock(&self, slot: usize) -> Option<FightActorRef> {
        if slot < self.group_size().max(1) {
            self.group_skill(slot).lock_target
        } else {
            None
        }
    }

    /// What a slot's weapon names.
    pub(in crate::fight) fn group_attack_target(&self, slot: usize) -> Option<FightActorRef> {
        if slot < self.group_size().max(1) {
            self.group_skill(slot).weapon_target()
        } else {
            None
        }
    }

    /// What the weapon names: the attack target while the skill holds a
    /// lock, and once it holds none, what a cooling still names or what
    /// the lock's loss left it firing at.
    pub(in crate::fight) fn weapon_target(&self) -> Option<FightActorRef> {
        if self.lock_target.is_some() {
            return self.attack_target();
        }
        match self.state {
            SkillState::Cooling { candidate, .. } => candidate,
            _ => self.attack_target_left,
        }
    }

    /// What an ungrouped skill's weapon names: its attack target, none while
    /// it fires at a shield, which the recording cannot name, and once it
    /// holds no lock, what a cooling still names or what `StopAttack` left.
    pub(in crate::fight) fn named_attack_target(&self) -> Option<FightActorRef> {
        self.attack_target()
            .filter(|_| self.shield_target().is_none())
            .or_else(|| {
                self.cooling()
                    .and_then(|(_, candidate)| candidate)
                    .filter(|_| self.lock_target.is_none())
            })
            .or_else(|| {
                self.attack_target_left
                    .filter(|_| self.lock_target.is_none())
            })
    }

    /// Each slot's lock, the core first.
    pub(in crate::fight) fn slot_locks(&self) -> Vec<Option<FightActorRef>> {
        (0..self.group_size())
            .map(|slot| self.slot_lock(slot))
            .collect()
    }

    /// The lock a recording reads as the unit's: a grouped unit's latest
    /// slot lock, and any other skill's own.
    pub(in crate::fight) fn unit_lock(&self) -> Option<FightActorRef> {
        self.group
            .as_ref()
            .map_or(self.lock_target, |group| group.mech_lock)
    }
}

/// What a skill's update hands on to the rest of its owner's update.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a separate fact the motion update reads"
)]
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::fight) struct SkillUpdate {
    /// A backswing ended before this update.
    pub(in crate::fight) backswing_just_finished: bool,
    /// A prepare ended on this update.
    pub(in crate::fight) prepare_finished: bool,
    /// The blow due on this update was rejected at its attack point.
    pub(in crate::fight) attack_point_rejected: bool,
    /// A burst still had projectiles to release when this update began, so
    /// its skill was not checked on it, the update the last one leaves
    /// included.
    pub(in crate::fight) burst_releasing: bool,
    /// The main skill released its blow in its own update
    /// ([`Simulation::perform_main_blow`]).
    pub(in crate::fight) blow_released: bool,
}

impl Simulation {
    /// `SkillCoolingState`: holds a skill whose attack has finished idle and
    /// without a lock for its cooling time, its weapon on what it last fired
    /// at, then clears the weapon and lets the idle skill search. Answers
    /// whether it held. Only `finish_attack` starts a cooling.
    ///
    /// A Marksman's cooling is 0.2 seconds, four steps: its skill reads
    /// cooling for four ticks and idle, emptied, for one more, and prepares
    /// on the next — after a kill whose replacement is out of reach
    /// (`crawlers-vs-marksman.yaml`) as after a fallen block
    /// (`layouts/wall-line-width.yaml`).
    pub(in crate::fight) fn hold_through_cooling(
        &mut self,
        skill_ref: SkillRef,
        step: u64,
    ) -> bool {
        let Some((started, held)) = self.skill(skill_ref).cooling() else {
            return false;
        };
        let cooling_steps = native_time_units_to_steps(
            self.skill_attacker(skill_ref)
                .expect("skill owner identity is stable")
                .attack
                .cooling_time_units(),
        );
        if step > started.saturating_add(cooling_steps) {
            self.skill_mut(skill_ref).set_cooling(None);
            return false;
        }
        // `SkillCoolingState.Update` only counts its time: the weapons go on
        // naming what the attack left them, and a skill that left them
        // firing at a shield names nothing until it searches again.
        let candidate = held.filter(|_| step < started.saturating_add(cooling_steps));
        let skill = self.skill_mut(skill_ref);
        // Cooling holds no lock; it hands the owner nothing while it has
        // none to drop, so a grouped unit keeps what a sibling took.
        if skill.lock_target.is_some() {
            skill.write_lock(None);
        }
        skill.set_cooling(Some((started, candidate)));
        skill.search_target_time = 0;
        // A standalone weapon's skill leaves the motion to the batch, which
        // may hold a lock through another weapon.
        if self.skill(skill_ref).standalone() {
            return true;
        }
        self.cool_motion(skill_ref);
        true
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the search timer's update, to be split with the attack state"
    )]
    pub(in crate::fight) fn update_fight_skill_target_search(
        &mut self,
        skill_ref: SkillRef,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        if self.is_side_arm(skill_ref) {
            return self.update_side_arm_idle_search(skill_ref, target_search_order);
        }
        // Target-build MechData disables MechSearchTargetController for every
        // supported non-supergiant unit, so live periodic selection belongs to
        // the main FightSkill. Prepare and Attack retain this private counter;
        // Attack only enters the selector when its private attack target is no
        // longer alive.
        let attacker = self
            .skill_attacker(skill_ref)
            .expect("skill owner identity is stable");
        let (quick_switch_target, team) = (attacker.attack.quick_switch_target, attacker.team);
        let skill = self.skill(skill_ref);
        let target = skill
            .attack_target()
            .and_then(|target| self.fight_actor(target));
        // `FightActor.IsValidTarget` asks the side too: a unit a beam turned
        // finds what it was after on its own side, and searches.
        let target_alive =
            target.is_some_and(|target| target.alive && target.targetable && target.team != team);
        if !target_alive
            && skill
                .backswing_finish_step()
                .is_some_and(|finish_step| finish_step >= step)
        {
            let quick_switch_interval_due = quick_switch_target && step > skill.next_attack_step;
            if !quick_switch_interval_due {
                return Ok(());
            }
            self.skill_mut(skill_ref).set_backswing_finish_step(None);
        }
        // A skill that splashes about itself checks nothing as it winds up
        // ([`Self::self_splash_winding_up`]): Disintegration goes on naming
        // the Fang that died a second into its wind-up until its blow. Nor
        // does a burst still releasing, whose quick switch is the check's
        // (`ProjectileMultiAttackPerformer.IsEnableCheckTarget` answers
        // `performIndex == 0`): a Mountain under Saturation Bombardment names
        // the Rhino its third projectiles killed until its fourth are out.
        let skill = self.skill(skill_ref);
        if (matches!(skill.phase(), FightSkillPhase::Prepare { .. })
            || skill.phase() == FightSkillPhase::Attack)
            && (!quick_switch_target
                || target_alive
                || !skill.performer.pending().is_empty()
                || self.self_splash_winding_up(skill_ref, step))
        {
            return Ok(());
        }
        // `SkillIdleState.CanStartSearchTarget` asks the lock, not what the
        // weapons fire at: a lock that died is searched for at once, even
        // while a construction that stood in its way still stands.
        let lock_alive = skill
            .lock_target
            .and_then(|lock| self.fight_actor(lock))
            .is_some_and(|lock| lock.alive && lock.team != team);
        // A skill left idle by its last search fires at nothing by design,
        // so only its lock and the timer are asked.
        if (target_alive || skill.idle) && lock_alive && skill.search_target_time > 0 {
            self.skill_mut(skill_ref).search_target_time -= 1;
            return Ok(());
        }

        self.skill_mut(skill_ref).searched_this_tick = true;

        // With no enemy unit left the selector answers the defeated side's
        // towers, as any search does: nothing in target selection asks
        // `FightCrystal.IsTower`. A unit that updates after the last enemy
        // died takes one on the tick the fight is decided.
        let located = |error: Error| {
            error.context(format!("logic step {step} actor {}", skill_ref.owner.id()))
        };
        let grouped_core = skill_ref.owner.unit_id().is_some()
            && self.skill(skill_ref).is_grouped()
            && self
                .skill(skill_ref)
                .siblings()
                .iter()
                .any(|slot| slot.lock_target.is_some());
        let side_arm_lock = self.side_arm_lock_for_main(skill_ref);
        let mut selected_candidate = if side_arm_lock.is_some() {
            side_arm_lock
        } else if grouped_core {
            // A grouped core's search is `PerformGroupedSkillSearch` as its
            // siblings' is: around what they hold, and among it when
            // nothing else answers in reach.
            self.select_group_lock_replacement(skill_ref, 0, target_search_order)
                .map_err(located)?
        } else {
            self.select_normal_target_with_order(
                skill_ref,
                target_search_order,
                !self.prepared_at_tick_start(skill_ref),
            )
            .map_err(located)?
        };
        if grouped_core {
            self.take_from_siblings(skill_ref, selected_candidate);
        }
        let idle = selected_candidate.is_none();
        if idle {
            selected_candidate = self
                .select_alive_target(skill_ref, grouped_core.then_some(0), target_search_order)
                .map_err(located)?;
        }
        self.skill_mut(skill_ref).idle = idle;
        if let Some(FightActorRef::Building(building_id)) = selected_candidate {
            let skill = self.skill_mut(skill_ref);
            // A building lock is written as any lock is, and the construction
            // in its way is asked for at once, as `SkillIdleState.TryPerform`
            // does after `TrySearchLockTarget`.
            skill.write_lock(Some(FightActorRef::Building(building_id)));
            skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
            skill.set_phase(FightSkillPhase::Idle);
            self.search_attack_target(skill_ref);
            self.hand_motion_after_lock_search(skill_ref);
            return Ok(());
        }
        let selected = selected_candidate;
        let skill = self.skill_mut(skill_ref);
        skill.write_lock(selected);
        skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
        self.search_attack_target(skill_ref);
        self.hand_motion_after_lock_search(skill_ref);
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::fight) fn step_actor(
        &mut self,
        actor_id: u64,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        self.refresh_target_query_snapshot();
        let target_search_order = self.target_search_order();
        if self.step_actor_before_buffs(actor_id, step, &target_search_order, events)? {
            self.step_actor_buffs(actor_id, events)?;
        }
        Ok(())
    }

    /// One unit's update, in the order `FightMech.Update` runs it: its skill,
    /// then its motion, then, while the fight is on, its buffs. The motion
    /// moves the body ([`Self::step_actor_rvo_position`]) between the two,
    /// which the caller does, so this answers whether the buffs are still to
    /// run ([`Self::step_actor_buffs`]).
    ///
    /// The skill's part starts its attack once what it fires at is in its
    /// attack area (`SkillIdleState.TryPerform`); the motion's part,
    /// `update_motion`, attacks a target in range and leaves one out of range
    /// or walks towards it. `BuffManager.Update` runs whichever way the two
    /// before it ended.
    pub(in crate::fight) fn step_actor_before_buffs(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<bool> {
        if !self.actors[&actor_id].alive() {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            // `FightCoreSystem.TeamUpdate` updates only a living unit. One
            // that died earlier on this tick keeps its buffs and its skill
            // through it, until `DeadEffectSystem` calls its `OnDead`: what
            // it fired lands as they leave its damage, on what it aimed at.
            let died_this_tick = actor.target_query_alive;
            if !died_this_tick {
                actor.exit_fight_on_death();
            }
            self.sync_beam(actor_id);
            if !died_this_tick {
                self.drop_buffs_of_the_dead(actor_id)?;
            }
            return Ok(false);
        }
        // `FightMech.Update` runs the unit's own search before its skills.
        self.update_mech_search(actor_id, target_search_order)?;
        self.step_actor_skill_and_motion(actor_id, step, target_search_order, events)?;
        self.sync_beam(actor_id);
        // With the fight over, `FightMech.Update` returns before
        // `BuffManager.Update`: no buff runs on, steps or runs out.
        Ok(self.ending.stop_step.is_none())
    }

    /// `BuffManager.Update`, last in `FightMech.Update`, after the motion
    /// moved the body: a unit a buff's step kills dies where it has just
    /// moved to.
    pub(in crate::fight) fn step_actor_buffs(
        &mut self,
        actor_id: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        self.update_buffs(actor_id, events)?;
        self.invoke_delayed_buffs(actor_id, events)
    }

    fn step_actor_skill_and_motion(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        if !self.actors[&actor_id].skills_active {
            // `SkillManager.Update` returns at once while its manager is not
            // active; the motion updates all the same. No skill counts its
            // `attackTime` towards its interval (`FightSkill.Update`), so
            // each next attack waits a tick more.
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skills
                .pause_attack_intervals();
            self.update_transition(actor_id)?;
            return Ok(());
        }
        // `SkillManager.Update` runs the unit's skills in ascending ID, and
        // then the motion updates.
        let (extras_before, extras_after) = self.extra_skills_around_main(actor_id);
        self.step_extra_skills(actor_id, &extras_before, step, target_search_order, events)?;
        let core_lock = self.actors[&actor_id].skills.main.lock_target;
        let motion_before = self.actors[&actor_id].motion.state;
        let core_was_attacking =
            self.actors[&actor_id].skills.main.phase() == FightSkillPhase::Attack;
        let body_rotation_q32 = self.actors[&actor_id].body_rotation_q32;
        self.skill_mut(SkillRef::main(FightActorRef::Unit(actor_id)))
            .lock_written = false;
        self.count_side_arm_fire_delay(FightActorRef::Unit(actor_id));
        let update = self.update_skill(
            SkillRef::main(FightActorRef::Unit(actor_id)),
            step,
            target_search_order,
            events,
        )?;
        let update = match update {
            Some(update) => Some(self.perform_main_blow(actor_id, step, events, update)?),
            None => None,
        };
        // A unit that searches for itself is its motion's attacker, so its
        // first weapon's skill starts and fires in its own update
        // (`SkillIdleState.TryStartAttack`, `SkillAttackState.TryPerformAttack`),
        // as the others do.
        if self.actors[&actor_id].skills.main.mech_searches()
            && matches!(
                self.actors[&actor_id].skills.main.phase(),
                FightSkillPhase::Idle | FightSkillPhase::Attack
            )
            && let Some(update) = update
        {
            self.start_standalone_core(actor_id, step, false, update.prepare_finished);
            if self.actors[&actor_id]
                .skills
                .main
                .pending()
                .is_some_and(|pending| pending.step == step)
            {
                let _attack_point_rejected =
                    self.release(SkillRef::main(FightActorRef::Unit(actor_id)), events)?;
            }
        }
        let fusillade = self.actors[&actor_id].skills.main.fusillade();
        if self.actors[&actor_id].skills.main.is_grouped() && !fusillade {
            // The core's `ChangeLockTarget` reaches the owner first; its
            // siblings update after it and may overwrite it.
            let skill = &mut self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skills
                .main;
            if skill.lock_written || skill.lock_target != core_lock {
                skill.set_mech_lock(skill.lock_target);
            }
            let core_entered_attack =
                !core_was_attacking && skill.phase() == FightSkillPhase::Attack;
            self.update_group_slots(
                SkillRef::main(FightActorRef::Unit(actor_id)),
                step,
                core_entered_attack,
                body_rotation_q32,
                target_search_order,
                events,
            )?;
        }
        self.step_extra_skills(actor_id, &extras_after, step, target_search_order, events)?;
        // A unit whose own blow took its life updates no further.
        if !self.actors[&actor_id].alive() {
            return Ok(());
        }
        // `FightSkill.Update` turns its weapons after its state has updated,
        // and before the motion turns the body: the skill's checks on this
        // update see the weapons as they were, and their arc is the one its
        // parent made as it was.
        let arc_parent_before = self.actors[&actor_id].arc_parent_q32();
        self.aim_standalone_turret(actor_id);
        self.step_motion(actor_id, step, events, update, motion_before)?;
        self.turn_arc_weapons(actor_id, arc_parent_before);
        if self.actors[&actor_id].skills.main.is_grouped() && fusillade {
            let skill = &mut self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .skills
                .main;
            if skill.lock_written || skill.lock_target != core_lock {
                skill.set_mech_lock(skill.lock_target);
            }
            let core_entered_attack =
                !core_was_attacking && skill.phase() == FightSkillPhase::Attack;
            self.update_group_slots(
                SkillRef::main(FightActorRef::Unit(actor_id)),
                step,
                core_entered_attack,
                body_rotation_q32,
                target_search_order,
                events,
            )?;
        }
        Ok(())
    }

    /// `MotionController.Update` after the unit's skills: a transition's,
    /// or the motion's own when the main skill's update or an extra skill
    /// that took the motion asks for it, a batch's, or a command's path.
    fn step_motion(
        &mut self,
        actor_id: u64,
        step: u64,
        events: &mut Vec<Event>,
        update: Option<SkillUpdate>,
        motion_before: MotionState,
    ) -> Result<()> {
        let was_moving = motion_before == MotionState::Moving;
        if let SkillSlot::Extra(_) = self.actors[&actor_id].motion.attacker {
            // The main skill letting its target go idles the motion in its
            // own update here, where the build's motion asks its attacker as
            // it updates: an extra skill that took the motion since finds it
            // in the state the skills found it.
            self.actors
                .get_mut(&actor_id)
                .expect("actor identity is stable")
                .motion
                .state = motion_before;
        }
        // `SkillManager.Update` ends with `PreemptiveSkillController.Update`,
        // before the motion updates.
        self.update_preemptive(actor_id, events)?;
        if let Flow::Done = self.update_transition(actor_id)? {
            // `TransitionState.Update` is the motion's whole update.
        } else if let Some(update) = update {
            self.update_motion(actor_id, step, events, update)?;
        } else if let SkillSlot::Extra(_) = self.actors[&actor_id].motion.attacker {
            // `MotionController.Update` asks an extra skill that took the
            // motion, whatever the main skill did.
            self.update_motion(actor_id, step, events, SkillUpdate::default())?;
        } else if self.actors[&actor_id].skills.main.state == SkillState::Locked {
            // `MotionController.Update` asks a main skill its
            // `SkillLockState` holds, which has let its lock go: a Mustang
            // whose interceptor prepares stands idle from the next update.
            self.update_motion(actor_id, step, events, SkillUpdate::default())?;
        } else if self.actors[&actor_id].skills.main.standalone()
            && (self.ending.stop_step.is_none()
                || self.actors[&actor_id].skills.main.mech_searches())
        {
            // `MotionController.Update` asks the batch, which the first
            // weapon's cooling does not hold: another weapon may lock. A unit
            // that searches for itself goes on after its own lock, a tower
            // the fight's last tick has not yet torn down.
            self.update_motion(actor_id, step, events, SkillUpdate::default())?;
        } else if self.actors[&actor_id].command.is_some()
            && self.actors[&actor_id].motion.state == MotionState::Attacking
            && (self.actors[&actor_id].skills.main.cooling().is_some()
                || self.actors[&actor_id]
                    .skills
                    .main
                    .attack_target()
                    .is_some_and(|target| !self.fight_actor_is_alive(target)))
        {
            // `MotionAttackState.Update` under a command asks what the
            // cooling still names, or the dead target a burst still fires
            // at.
            self.update_motion(actor_id, step, events, SkillUpdate::default())?;
        } else if was_moving
            && self.actors[&actor_id].command.is_some()
            && self.actors[&actor_id].skills.main.attack_target().is_none()
        {
            // `MotionController.Update` runs whatever the skill did: a
            // command walks its path while the skill cools or reloads, and
            // once a won fight has stopped it, unless what the cooling names
            // has come into range. A move state entered on this update is not
            // updated on it.
            if let Flow::Next = self.move_into_cooled_target(actor_id) {
                self.follow_command(actor_id);
            }
        }
        Ok(())
    }

    /// `SkillManager.Update`: one skill's update, for whoever owns it.
    ///
    /// A magazine emptied or refilling (`SkillReloadingState`), the checks the
    /// skill's state runs (`SkillCoolingState`, `SkillPrepareState` and
    /// `SkillAttackState` asking `CheckAttackable`), a grouped skill's slots,
    /// the ends of a burst, the idle search and its timer, the state moving
    /// on as its time is up, and what is due to be performed. Each part says
    /// whether the update goes on; `None` is an update that ended here.
    pub(in crate::fight) fn update_skill(
        &mut self,
        skill_ref: SkillRef,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<Option<SkillUpdate>> {
        // `SkillLockState` does not update.
        if self.skill(skill_ref).state == SkillState::Locked {
            return Ok(None);
        }
        let backswing_just_finished = self
            .skill(skill_ref)
            .backswing_finish_step()
            .is_some_and(|finish_step| finish_step < step);
        let burst_releasing = !self.skill(skill_ref).performer.pending().is_empty();
        self.skill_mut(skill_ref).burst_releasing = burst_releasing;
        // A sweep that struck its last stretch on the update before is over.
        if let Performer::Sweep(sweep) = &self.skill(skill_ref).performer
            && sweep.over()
        {
            self.skill_mut(skill_ref).performer = Performer::Normal;
        }
        if let Flow::Done = self.exit_fight_when_over(skill_ref) {
            return Ok(None);
        }
        if let Flow::Done = self.update_reload(skill_ref, step) {
            return Ok(None);
        }
        if let Flow::Done = self.update_skill_checks(skill_ref, step, target_search_order)? {
            return Ok(None);
        }
        if let Flow::Done = self.finish_attack_at_dead_target(skill_ref, step) {
            return Ok(None);
        }
        // `SkillIdleState.Update` performs nothing for a skill that is not
        // enabled: it searches for no lock, no attack target, and starts no
        // attack, and keeps what it named.
        if self.skill(skill_ref).disabled
            && matches!(self.skill(skill_ref).state, SkillState::Idle { .. })
        {
            self.try_side_arm_reset_fire_mark(skill_ref);
            return Ok(None);
        }
        let quick_switch_target = self
            .skill_attacker(skill_ref)
            .expect("skill owner identity is stable")
            .attack
            .quick_switch_target;
        let quick_switch_backswing_due = {
            let skill = self.skill(skill_ref);
            quick_switch_target
                && skill
                    .backswing_finish_step()
                    .is_some_and(|finish_step| finish_step >= step)
                && step > skill.next_attack_step
        };
        let quick_switch_dead_backswing_due = quick_switch_backswing_due
            && self
                .skill(skill_ref)
                .attack_target()
                .is_some_and(|target| !self.fight_actor_is_alive(target));
        if let Flow::Done = self.projectile_burst_lost_target(skill_ref, step, events)? {
            // A burst at a block that fell in the way of a live lock leaves
            // the motion's update to it, which holds the block and turns past
            // it to the lock.
            let lock_lives = self
                .skill(skill_ref)
                .lock_target
                .is_some_and(|lock| self.fight_actor_is_alive(lock));
            return Ok(lock_lives.then(|| SkillUpdate {
                burst_releasing,
                ..SkillUpdate::default()
            }));
        }
        // `SkillIdleState.TryPerform` reaches `SearchAttackTarget` on every
        // update the skill is idle with a lock.
        if self.skill(skill_ref).phase() == FightSkillPhase::Idle {
            self.search_attack_target(skill_ref);
        }
        self.update_fight_skill_target_search(skill_ref, step, target_search_order)?;
        self.try_perform_main(skill_ref, step);
        let prepare_finished = self.advance_skill_state(
            skill_ref,
            step,
            backswing_just_finished,
            quick_switch_backswing_due,
            quick_switch_dead_backswing_due,
        );
        if let Flow::Done = self.retarget_pending_blow(skill_ref) {
            return Ok(None);
        }
        let (flow, attack_point_rejected) =
            self.perform_due_blows(skill_ref, step, prepare_finished, events)?;
        if let Flow::Done = flow {
            return Ok(None);
        }
        Ok(Some(SkillUpdate {
            backswing_just_finished,
            prepare_finished,
            attack_point_rejected,
            burst_releasing,
            blow_released: false,
        }))
    }

    /// `SkillReloadingState`, for a skill that fires from a magazine: an
    /// empty magazine takes the skill into the reload on its next update,
    /// whatever state it is in; the reload keeps the lock, lasts its time, and
    /// hands the skill back to idle, full, its search timer reset
    /// (`SkillReloadingState.Exit`). The state's update is the skill's whole
    /// update. A skill without a magazine never reloads.
    fn update_reload(&mut self, skill_ref: SkillRef, step: u64) -> Flow {
        let Some(magazine) = self
            .skill_attacker(skill_ref)
            .expect("skill owner identity is stable")
            .attack
            .magazine
        else {
            return Flow::Next;
        };
        let skill = self.skill_mut(skill_ref);
        match skill.state {
            SkillState::Reloading { finish_step } => {
                if step >= finish_step {
                    // The Rapid-Fire Turret of `rapid-fire-head-on.yaml` fires
                    // its 11th shot at the Crawler it fired its 10th at, where
                    // a search from its weapon would have taken another: the
                    // idle the reload hands the lock to keeps it.
                    skill.rounds = Some(magazine.capacity);
                    skill.enter(SkillState::Idle { ready_step: None });
                    skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
                }
                Flow::Done
            }
            _ if skill.rounds == Some(0) => {
                skill.enter(SkillState::Reloading {
                    finish_step: step
                        .saturating_add(native_time_units_to_steps(magazine.reload_time_units())),
                });
                Flow::Done
            }
            _ => Flow::Next,
        }
    }

    /// `SkillIdleState.TryStartAttack` and `SkillAttackState.TryPerformAttack`
    /// for a skill whose target is in reach: an idle skill enters its prepare
    /// or attack state, and an attacking one begins the next blow's wait once
    /// its interval is up.
    ///
    /// `entered_attack` is whether the owner comes into its attack on this
    /// update: a state is not updated on the tick it is entered, so a blow
    /// waits for the tick after.
    pub(in crate::fight) fn try_start_attack(
        &mut self,
        skill_ref: SkillRef,
        step: u64,
        target: FightActorRef,
        entered_attack: bool,
        in_attack_angle: bool,
        prepare_finished: bool,
    ) {
        let attack = self
            .skill_attacker(skill_ref)
            .expect("skill owner identity is stable")
            .attack;
        let prepare_steps = native_time_units_to_steps(attack.prepare_time_units());
        let attack_point_steps = native_time_units_to_steps(attack.attack_point_time_units());
        let backswing_steps = native_time_units_to_steps(attack.backswing_time_units());
        // `SkillIdleState.Update` and `SkillAttackState.TryPerformAttack` ask
        // `CanFireByTakeTurns` before they begin anything.
        let in_turn = self.can_fire_by_take_turns(skill_ref);
        let side_arm = self.is_side_arm(skill_ref);
        let skill = self.skill_mut(skill_ref);
        let mut entered_skill_phase = false;
        // `SkillIdleState.TryStartAttack` enters the attack or prepare
        // state on the tick the unit comes into its attack area,
        // which is the tick its motion starts attacking; the state
        // is not updated until the tick after, where the first
        // blow's wait begins.
        // A bodyless unit whose facing its motion is still correcting
        // enters it all the same; only the blow waits for the facing.
        if in_turn
            && in_attack_angle
            && skill.pending().is_none()
            && skill.backswing_finish_step().is_none()
            && skill.phase() == FightSkillPhase::Idle
        {
            let from_idle_state = matches!(skill.state, SkillState::Idle { ready_step: None });
            let from = step;
            skill.set_phase(if prepare_steps == 0 {
                FightSkillPhase::Attack
            } else {
                FightSkillPhase::Prepare {
                    finish_step: from.saturating_add(prepare_steps),
                }
            });
            // Leaving `SkillIdleState` is entering a state too, and the blow
            // waits for the tick after it just the same: a Fortress whose
            // weapons turn onto a Crawler while its motion already attacks
            // reads `SkillAttackState` on the tick they come into its angle
            // and releases on the next, as a Sledgehammer or Typhoon that
            // locks a new target does. A skill leaving its cooling does not
            // wait.
            entered_skill_phase = prepare_steps > 0 || entered_attack || from_idle_state;
        }
        // A skill already in its attack state starts its next blow's wait on
        // the tick its interval is up, whatever its motion did: a Crawler
        // pushed out of reach during its backswing and back in on the tick
        // after starts its next blow on the tick it returns, as the game's
        // skill state reads in the Rhino's formation fight.
        // `SkillAttackState.TryPerformAttack` asks the attack angle and
        // nothing of the motion, so a bodyless unit back in its attack out
        // of angle is not held once the angle answers: a Wasp moved during
        // its backswing fires on the first tick its target is in its angle.
        let blow_due = in_turn
            && skill.pending().is_none()
            && skill.backswing_finish_step().is_none()
            && skill.phase() == FightSkillPhase::Attack
            && !entered_skill_phase
            && skill.started_from_idle != Some(step)
            // A state is not updated on the tick it is entered: the
            // first blow waits for the tick after the prepare ends.
            && !prepare_finished
            && step >= skill.next_attack_step;
        // A side arm whose turn has come with its target out of its angle
        // gives the turn back.
        if blow_due && !in_attack_angle && side_arm {
            self.force_side_arm_end_fire_turn(skill_ref);
        }
        if blow_due && in_attack_angle {
            // `FightSkill.ResetAttackData` draws the interval before
            // `SkillAttackController.PerformAttack` fits the blow into it.
            let interval = self
                .draw_attack_interval(skill_ref)
                .expect("every skill owner's team owns one attack random stream");
            let attack_point_steps =
                fitted_attack_point(attack_point_steps, backswing_steps, interval);
            // `SkillAttackController.PerformAttack` begins with the linker's
            // `TryEffect`, so the blow it winds up deals what that wrote.
            if let (SkillSlot::Main, Some(actor_id)) = (skill_ref.slot, skill_ref.owner.unit_id()) {
                self.actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable")
                    .try_attack_count_effect();
            }
            self.skill_mut(skill_ref)
                .schedule_blow(step, interval, attack_point_steps, target);
            self.set_fire_turns_mark(skill_ref);
        }
    }

    /// The checks the skill's state runs at the start of its update:
    /// `SkillCoolingState` holding, `SkillPrepareState` and `SkillAttackState`
    /// asking `CheckAttackable`.
    fn update_skill_checks(
        &mut self,
        skill_ref: SkillRef,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<Flow> {
        // `SkillCoolingState.Update` asks a side arm whether it keeps its
        // turn before its time.
        if matches!(self.skill(skill_ref).state, SkillState::Cooling { .. }) {
            self.try_side_arm_reset_fire_mark(skill_ref);
        }
        if self.hold_through_cooling(skill_ref, step) {
            return Ok(Flow::Done);
        }
        // `SkillPrepareState.Update` asks `SkillAttackableChecker.Check` on
        // every update; a failed check leaves the skill idle with its targets
        // cleared. The check decides the attack target again on the way,
        // which is where a block coming into the line of fire is met.
        if matches!(
            self.skill(skill_ref).phase(),
            FightSkillPhase::Prepare { .. }
        ) && !self.check_attackable(skill_ref, target_search_order)?
        {
            self.force_side_arm_end_fire_turn(skill_ref);
            self.enter_idle_clearing_targets(skill_ref);
            return Ok(Flow::Done);
        }
        // `SkillAttackState.Update` has a side arm search again, where its
        // lock no longer suits, before anything else it asks.
        if matches!(self.skill(skill_ref).state, SkillState::Attack(_))
            && self.is_side_arm(skill_ref)
            && self.need_refresh_side_arm_target(skill_ref)
        {
            self.search_skill_lock_target(skill_ref, target_search_order)?;
        }
        // `SkillAttackState.Update` asks `CheckAttackable` between two blows,
        // and a failed check finishes the attack.
        if self.between_blows(skill_ref, step) {
            if !self.attack_state_check_attackable(skill_ref, target_search_order)? {
                self.finish_attack(skill_ref, step);
                return Ok(Flow::Done);
            }
            // The blow being wound up is performed on the skill's attack
            // target, which the check may just have changed.
            let skill = self.skill_mut(skill_ref);
            if !skill.is_grouped()
                && let Some(target) = skill.attack_target()
                && let Some(pending) = skill.pending_mut()
            {
                pending.target = target;
            }
        }
        Ok(Flow::Next)
    }

    /// `SkillManager.Update` with `isFighting` false: from the tick after a
    /// side is decided, `FightCoreSystem.IsStepFinish` has set `isFightOver`
    /// and `TeamUpdate` runs every mech's update with the fight off. No skill
    /// runs its state machine. One that holds a lock exits the fight
    /// (`FightSkill.ExitFight`): `StopAttack`, which also ends a burst still
    /// firing, its idle state and its lock cleared. The rest are left as they
    /// are, a cooling one still cooling.
    fn exit_fight_when_over(&mut self, skill_ref: SkillRef) -> Flow {
        if self.ending.stop_step.is_none() {
            return Flow::Next;
        }
        // A manager holding fire (`isHoldFire`) updates its main skill
        // whatever the fight does: a Sandworm below goes on searching once
        // the fight is decided, and walks on to the defeated side's tower.
        if let (FightActorRef::Unit(actor_id), SkillSlot::Main) = (skill_ref.owner, skill_ref.slot)
            && self.actors[&actor_id]
                .underground
                .as_ref()
                .is_some_and(|underground| underground.below)
        {
            return Flow::Next;
        }
        if self.skill(skill_ref).lock_target.is_some() {
            let clear_velocity = self.ending.terminal_drain_pending;
            let skill = self.skill_mut(skill_ref);
            skill.drop_lock();
            skill.set_phase(FightSkillPhase::Idle);
            skill.performer.stop();
            // `ExitFight` resets its `AttackCountEffectLinker` too.
            if let (FightActorRef::Unit(actor_id), SkillSlot::Main) =
                (skill_ref.owner, skill_ref.slot)
            {
                self.actors
                    .get_mut(&actor_id)
                    .expect("actor identity is stable")
                    .attack_count_condition(false);
            }
            if let Some(actor) = self.moving_mut(skill_ref) {
                if clear_velocity {
                    actor.motion.current_velocity_x_q32 = 0;
                    actor.motion.current_velocity_z_q32 = 0;
                }
                // The skill letting its target go leaves a command active.
                actor.lose_target_motion(true);
            }
        }
        Flow::Done
    }

    /// A skill in its attack state whose target is dead ends its attack.
    /// `SkillAttackState.Update` asks `CheckAttackable` before the next
    /// blow, except through a backswing, whose wait is built with
    /// `isEnableCheckTarget` off. `SkillAttackableChecker.Check` searches
    /// again for a dead lock, and a skill that cannot switch quickly fails
    /// the check when that changes its target, so it finishes. A skill that
    /// can switch quickly takes the new target in place, which the checker
    /// answers.
    ///
    /// Nothing clears a lock when its target dies, and the skill updates
    /// before the motion, so a skill's own kill is seen here the update
    /// after it, while the motion has already gone idle on it.
    fn finish_attack_at_dead_target(&mut self, skill_ref: SkillRef, step: u64) -> Flow {
        let quick_switch_target = self
            .skill_attacker(skill_ref)
            .expect("skill owner identity is stable")
            .attack
            .quick_switch_target;
        let skill = self.skill(skill_ref);
        // A sweep under way runs on whatever becomes of its target, and so
        // does a burst still releasing, which goes on naming it
        // ([`Self::projectile_burst_lost_target`]): Electromagnetic Barrage
        // names the Crawler the beam killed until its last shell is out.
        let dead_target = !quick_switch_target
            && skill.phase() == FightSkillPhase::Attack
            && skill.backswing_finish_step().is_none()
            && !skill.performer.sweeping()
            && skill.performer.pending().is_empty()
            && !self.self_splash_winding_up(skill_ref, step)
            && skill
                .attack_target()
                .is_some_and(|target| !self.fight_actor_is_alive(target));
        if dead_target {
            self.finish_attack(skill_ref, step);
            return Flow::Done;
        }
        Flow::Next
    }

    /// `SkillIdleState.TryPerform` of a unit's main skill, after its search:
    /// a skill that holds what it fires at in its attack area enters its
    /// prepare, or its attack with no prepare, in its own update, whatever
    /// its motion does after it (`CanStartAttack`, `TryStartAttack`). The
    /// state is not updated on the update it is entered: the first blow
    /// waits for the next.
    fn try_perform_main(&mut self, skill_ref: SkillRef, step: u64) {
        let FightActorRef::Unit(actor_id) = skill_ref.owner else {
            return;
        };
        if skill_ref.slot != SkillSlot::Main {
            return;
        }
        let actor = &self.actors[&actor_id];
        let skill = &actor.skills.main;
        if skill.standalone()
            || skill.mech_searches()
            || skill.is_grouped()
            || actor.travelling
            // A unit moving below starts nothing until it has surfaced.
            || actor
                .underground
                .as_ref()
                .is_some_and(|underground| underground.state == super::underground::AbilityState::Moving)
            || skill.idle
            || skill.phase() != FightSkillPhase::Idle
            || !matches!(skill.state, SkillState::Idle { ready_step: None })
            || skill.pending().is_some()
            || skill.backswing_finish_step().is_some()
        {
            return;
        }
        let Some(target) = skill.attack_target() else {
            return;
        };
        if !self.target_in_attack_area(skill_ref, target) || !self.may_start_attack(skill_ref) {
            return;
        }
        let prepare_steps = native_time_units_to_steps(
            self.skill_attacker(skill_ref)
                .expect("skill owner identity is stable")
                .attack
                .prepare_time_units(),
        );
        let skill = self.skill_mut(skill_ref);
        skill.started_from_idle = Some(step);
        skill.set_phase(if prepare_steps == 0 {
            FightSkillPhase::Attack
        } else {
            FightSkillPhase::Prepare {
                finish_step: step.saturating_add(prepare_steps),
            }
        });
    }

    /// Whether a side has an enemy left as this tick's updates see it: one
    /// alive, or one killed on this tick, which its side keeps until
    /// `DeadEffectSystem` takes it as the tick ends. The Melting Point's beam
    /// kills the last enemy, and its barrage still releases its shell at it
    /// on the same tick.
    pub(in crate::fight) fn enemy_left(&self, team: u32) -> bool {
        self.actors.values().any(|actor| {
            actor.placement.team != team && (actor.alive() || actor.target_query_alive)
        })
    }

    /// A burst whose target died while it was still firing: with no enemy
    /// left it stops, and otherwise the rest of it is fired where it was aimed
    /// while the unit stands.
    fn projectile_burst_lost_target(
        &mut self,
        skill_ref: SkillRef,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<Flow> {
        let skill = self.skill(skill_ref);
        let active_projectile_burst_lost_target = !skill.performer.pending().is_empty()
            && skill
                .attack_target()
                .is_some_and(|target| !self.fight_actor_is_alive(target));
        if active_projectile_burst_lost_target {
            let owner_team = self
                .skill_attacker(skill_ref)
                .expect("skill owner identity is stable")
                .team;
            let has_alive_enemy = self.enemy_left(owner_team);
            // The skill still names its dead target. With no command the
            // motion's lock is dead and it stops idle; a command keeps the
            // motion active on it (`attack_under_command`), so a unit under
            // one stays attacking until the burst is out and the skill lets
            // the target go: a Phantom Ray on a Mobile Beacon reads attacking
            // on the tick its burst's second shot goes out at the Vortex that
            // died after its first, and moving the next. A block that fell in
            // the way of a live lock leaves the lock alive, and the motion,
            // which asks the lock, attacking: two Stormcallers whose burst
            // went on at a fallen block of replay 134369439 round 8 read
            // attacking through it, turning to their lock.
            let lock_lives = self
                .skill(skill_ref)
                .lock_target
                .is_some_and(|lock| self.fight_actor_is_alive(lock));
            // The idle state publishes its point as it is entered, not on
            // every update the burst goes on: a Stormcaller an RVO solve
            // nudged while its burst fires on goes back to where it stopped.
            if let Some(actor) = self.moving_mut(skill_ref)
                && actor.command.is_none()
                && !lock_lives
            {
                let entered_idle = actor.motion.state != MotionState::Idle;
                actor.lose_target_motion(entered_idle);
            }
            if !has_alive_enemy {
                self.skill_mut(skill_ref).performer.stop();
                return Ok(Flow::Done);
            }
            let due = self.skill_mut(skill_ref).performer.take_due(step);
            for pending in due {
                self.release_pending_projectile(skill_ref, pending, events)?;
            }
            self.finish_attacking(skill_ref);
            return Ok(Flow::Done);
        }
        Ok(Flow::Next)
    }

    /// Moves the skill's state on where its time is up: a backswing that has
    /// ended, a prepare that has ended. Answers whether the prepare ended.
    fn advance_skill_state(
        &mut self,
        skill_ref: SkillRef,
        step: u64,
        backswing_just_finished: bool,
        quick_switch_backswing_due: bool,
        quick_switch_dead_backswing_due: bool,
    ) -> bool {
        let skill = self.skill_mut(skill_ref);
        if quick_switch_backswing_due && !quick_switch_dead_backswing_due {
            skill.set_backswing_finish_step(None);
        }
        if backswing_just_finished {
            skill.set_backswing_finish_step(None);
            // The attack state outlives the backswing: `SkillAttackController`
            // has no phase running until the next blow's wait begins, and the
            // checker is asked in that gap.
            skill.set_phase(FightSkillPhase::Attack);
        }
        let prepare_finished = matches!(
            skill.phase(),
            FightSkillPhase::Prepare { finish_step } if finish_step <= step
        );
        if prepare_finished {
            skill.set_phase(FightSkillPhase::Attack);
        }
        prepare_finished
    }

    /// The blow being wound up follows what the skill fires at, and a bodyless
    /// one whose target left its attack area is dropped.
    fn retarget_pending_blow(&mut self, skill_ref: SkillRef) -> Flow {
        let attacker = self
            .skill_attacker(skill_ref)
            .expect("skill owner identity is stable");
        let follows = attacker.has_body && attacker.attack.quick_switch_target;
        let skill = self.skill(skill_ref);
        let bodyful_quick_switch_target = (follows && skill.pending().is_some())
            .then_some(skill.attack_target())
            .flatten()
            .filter(|&target_id| self.target_in_attack_area(skill_ref, target_id));
        if let Some(target_id) = bodyful_quick_switch_target {
            self.skill_mut(skill_ref)
                .pending_mut()
                .expect("pending attack identity is stable")
                .target = target_id;
        }
        let active_attack_rejected = self
            .skill(skill_ref)
            .pending()
            .is_some_and(|pending| self.bodyless_attackable_invalid(skill_ref, pending.target));
        if active_attack_rejected {
            // The build's SkillPrepareState and SkillAttackState both run
            // CheckAttackable before advancing their current attack phase.
            // A failed check enters SkillIdleState in the same update.
            let skill = self.skill_mut(skill_ref);
            skill.drop_lock();
            skill.set_pending(None);
            skill.set_phase(FightSkillPhase::Idle);
            if let Some(actor) = self.moving_mut(skill_ref) {
                actor.lose_target_motion(true);
            }
            return Flow::Done;
        }
        Flow::Next
    }

    /// Performs what is due this update: the blow wound up, the shots of a
    /// burst, a grouped skill's core and its slots. Answers whether the blow
    /// was rejected at its attack point.
    fn perform_due_blows(
        &mut self,
        skill_ref: SkillRef,
        step: u64,
        entered_attack: bool,
        events: &mut Vec<Event>,
    ) -> Result<(Flow, bool)> {
        let released_this_step = self
            .skill(skill_ref)
            .pending()
            .is_some_and(|pending| pending.step <= step);
        let attack_point_rejected = if released_this_step {
            self.release(skill_ref, events)?
        } else {
            false
        };
        if released_this_step && self.motion_state(skill_ref.owner) != MotionState::Attacking {
            return Ok((Flow::Done, attack_point_rejected));
        }
        let due = self.skill_mut(skill_ref).performer.take_due(step);
        for pending in due {
            self.release_pending_projectile(skill_ref, pending, events)?;
        }
        if self.skill(skill_ref).performer.sweeping()
            && let Some(actor_id) = skill_ref.owner.unit_id()
        {
            self.update_sweep(actor_id, events)?;
        }
        self.finish_attacking(skill_ref);
        if self.skill(skill_ref).is_grouped() {
            let actor_id = skill_ref
                .owner
                .unit_id()
                .expect("only a unit's skill is grouped");
            self.perform_group_blows(actor_id, step, entered_attack, events)?;
        }
        Ok((Flow::Next, attack_point_rejected))
    }

    /// Whether a blow wound up by an owner without a body has lost what it
    /// was wound up for: at the attack point `SkillAttackState` checks the
    /// attack area again, measured from the root, where an owner whose weapons
    /// turn on their own goes on.
    pub(in crate::fight) fn bodyless_attackable_invalid(
        &self,
        skill_ref: SkillRef,
        target: FightActorRef,
    ) -> bool {
        let Some(attacker) = self.skill_attacker(skill_ref) else {
            return false;
        };
        // A skill that splashes about itself checks no target through its
        // blow (`SkillAttackController`'s waits and `NormalAttackPerformer.
        // IsEnableCheckTarget` answer `!IsSelfSplash`): a Whirlwind strikes
        // about its Rhino though the lock it wound up on has died.
        !attacker.has_body
            && !attacker.attack.self_splash
            && !self.target_in_attack_area(skill_ref, target)
    }

    /// Schedules the next attack and remembers the interval it used.
    pub(in crate::fight) fn sample_attack_interval(
        &mut self,
        skill_ref: SkillRef,
        step: u64,
    ) -> Result<u64> {
        let sampled = self.draw_attack_interval(skill_ref)?;
        let skill = self.skill_mut(skill_ref);
        skill.current_attack_interval = sampled;
        skill.attack_time_anchor = i64::try_from(step).unwrap_or(i64::MAX);
        Ok(step.saturating_add(sampled))
    }
}
