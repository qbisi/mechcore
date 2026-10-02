//! Formation experience, `ExpSystem` as the build runs it inside the tick.
//!
//! Every hit adds its attacker to the target's attack list, first hit first
//! and once. A kill hands out the target's experience at once:
//!
//! - the killer's formation takes the whole of it, when the killer is a unit
//!   that may take experience; a kill by anything else doubles the pool below
//!   instead;
//! - a pool of the same size (`assistKillExpRate` of it, which is 1.0) is
//!   split evenly between every formation that hit the target and every
//!   formation of the killer's side standing within `assistExpRange` of it,
//!   each formation once.
//!
//! A formation starts at -1.0, the build's reset value, and then at the
//! experience the layout brings it when that is above zero; its first gain
//! starts from zero, and no gain carries it past its bar.

use super::*;

const ONE_Q32: i64 = 1 << 32;

const UNIT_EXPERIENCE: &str = include_str!("../../../../config/unit_experience.yaml");

/// `ExpSystem`: what each formation has gained, who has hit what, and what
/// a kill hands out.
pub(in crate::fight) struct ExpSystem {
    /// The bars and loot of every unit type.
    pub(in crate::fight) table: ExperienceTable,
    /// Each formation's experience, by formation.
    pub(in crate::fight) formations: BTreeMap<u64, FormationExperience>,
    /// Who has hit each target, first hit first.
    pub(in crate::fight) attackers: BTreeMap<ObjectRef, Vec<ObjectRef>>,
    /// What each building's destruction hands out, by building.
    pub(in crate::fight) building_exp: BTreeMap<u64, i64>,
}

impl ExpSystem {
    /// No experience gained yet, with each building's loot.
    pub(in crate::fight) fn new(building_exp: BTreeMap<u64, i64>) -> Result<Self> {
        Ok(Self {
            table: ExperienceTable::load()?,
            formations: BTreeMap::new(),
            attackers: BTreeMap::new(),
            building_exp,
        })
    }
}

#[derive(serde::Deserialize)]
struct ExperienceFile {
    assist_kill_exp_rate: i64,
    assist_exp_range: i64,
    units: Vec<ExperienceRow>,
}

#[derive(serde::Deserialize)]
struct ExperienceRow {
    #[serde(rename = "type")]
    type_name: String,
    upgrade_exp: Vec<i64>,
    loot_exp: Vec<i64>,
}

/// `config/unit_experience.yaml`: each unit's bars and what it hands out.
pub(in crate::fight) struct ExperienceTable {
    assist_kill_exp_rate: i64,
    /// Metres.
    assist_exp_range: i64,
    units: BTreeMap<String, ExperienceRow>,
}

impl ExperienceTable {
    pub(in crate::fight) fn load() -> Result<Self> {
        let file: ExperienceFile = serde_yaml::from_str(UNIT_EXPERIENCE)
            .map_err(|error| Error::new(format!("config/unit_experience.yaml: {error}")))?;
        Ok(Self {
            assist_kill_exp_rate: file.assist_kill_exp_rate,
            assist_exp_range: file.assist_exp_range,
            units: file
                .units
                .into_iter()
                .map(|row| (row.type_name.clone(), row))
                .collect(),
        })
    }

    fn row(&self, type_name: &str) -> Result<&ExperienceRow> {
        self.units
            .get(type_name)
            .ok_or_else(|| Error::new(format!("{type_name:?} has no experience row")))
    }

    /// The bar a formation of this level fills at: `GetUpgradeExp(level + 1)`,
    /// level 9's being level 8's.
    fn bar(&self, type_name: &str, level: i64) -> Result<i64> {
        let row = self.row(type_name)?;
        let index = usize::try_from(level.clamp(1, 8) - 1).unwrap_or_default();
        row.upgrade_exp
            .get(index)
            .copied()
            .ok_or_else(|| Error::new(format!("{type_name:?} has no bar at level {level}")))
    }

    /// What one unit of this level hands out when it is killed.
    fn loot(&self, type_name: &str, level: i64) -> Result<i64> {
        let row = self.row(type_name)?;
        let index = usize::try_from(level.clamp(1, 9) - 1).unwrap_or_default();
        row.loot_exp
            .get(index)
            .copied()
            .ok_or_else(|| Error::new(format!("{type_name:?} hands out nothing at level {level}")))
    }
}

/// One formation's experience, `FPoint` raw.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct FormationExperience {
    team: u32,
    experience: i64,
    bar: i64,
}

impl FormationExperience {
    fn full(self) -> bool {
        self.experience >= self.bar
    }
}

impl Simulation {
    /// Every formation's bar, and the experience it opens the fight with.
    ///
    /// `MechTeam` is built at -1.0, and `MapPlayerDataReader.LoadUnitDatas`
    /// then hands it the unit's experience through `MechTeam.SetExpInt`,
    /// which takes a value above zero whole and leaves -1.0 otherwise.
    pub(in crate::fight) fn seed_experience(&mut self) -> Result<()> {
        for actor in self.actors.values() {
            if self
                .exp
                .formations
                .contains_key(&actor.placement.formation_id)
            {
                continue;
            }
            let bar = self
                .exp
                .table
                .bar(&actor.placement.type_name, actor.placement.level)?;
            self.exp.formations.insert(
                actor.placement.formation_id,
                FormationExperience {
                    team: actor.placement.team,
                    experience: if actor.placement.exp > 0 {
                        actor.placement.exp << 32
                    } else {
                        -ONE_Q32
                    },
                    bar: bar << 32,
                },
            );
        }
        Ok(())
    }

    pub(in crate::fight) fn formation_states(&self) -> Vec<FormationState> {
        self.exp
            .formations
            .iter()
            .map(|(formation_id, formation)| FormationState {
                formation_id: *formation_id,
                team_id: formation.team,
                experience: formation.experience,
                max_experience: formation.bar,
            })
            .collect()
    }

    /// What killing the target hands out, or nothing when it hands out none.
    fn provided(&self, target: FightActorRef) -> Result<i64> {
        match target {
            FightActorRef::Unit(id) => {
                let actor = &self.actors[&id];
                self.exp
                    .table
                    .loot(&actor.placement.type_name, actor.placement.level)
            }
            FightActorRef::Building(id) => Ok(self.exp.building_exp.get(&id).copied().unwrap_or(0)),
        }
    }

    /// A unit that may take experience: alive, in a formation whose bar is
    /// not full.
    fn may_gain(&self, unit_id: u64) -> Option<u64> {
        let actor = self.actors.get(&unit_id)?;
        let formation = actor.placement.formation_id;
        (actor.alive()
            && self
                .exp
                .formations
                .get(&formation)
                .is_some_and(|state| !state.full()))
        .then_some(formation)
    }

    /// `MechTeam.PruneExp`, as the fight ends: a formation's gain is held to
    /// a whole number, and one that has never gained keeps -1.0.
    pub(in crate::fight) fn prune_experience(&mut self) {
        for state in self.exp.formations.values_mut() {
            if state.experience > 0 {
                state.experience &= !(ONE_Q32 - 1);
            }
        }
    }

    /// `MechTeam.AddExp` with no rate on it: a formation below zero starts
    /// from zero, and the bar caps it.
    fn gain(&mut self, formation: u64, amount: i64) {
        if let Some(state) = self.exp.formations.get_mut(&formation) {
            state.experience = state
                .experience
                .max(0)
                .saturating_add(amount)
                .min(state.bar);
        }
    }

    /// `ExpSystem.OnActorHitted`: the hit's attacker joins the target's list,
    /// and a kill hands the target's experience out.
    pub(in crate::fight) fn count_experience(
        &mut self,
        source: Option<ObjectRef>,
        source_team: u32,
        target: FightActorRef,
        killed: bool,
    ) -> Result<()> {
        let provided = self.provided(target)?;
        // A building that hands out nothing keeps no list; a unit always does.
        if matches!(target, FightActorRef::Building(_)) && provided <= 0 {
            return Ok(());
        }
        let attackers = self.exp.attackers.entry(target.object_ref()).or_default();
        if let Some(source) = source
            && !attackers.contains(&source)
        {
            attackers.push(source);
        }
        if !killed {
            return Ok(());
        }
        self.hand_out(source, source_team, target, provided)
    }

    /// `ExpSystem.DoCalculateExp`.
    fn hand_out(
        &mut self,
        source: Option<ObjectRef>,
        source_team: u32,
        target: FightActorRef,
        provided: i64,
    ) -> Result<()> {
        let base = provided << 32;
        let mut pool = q32_mul(base, self.exp.table.assist_kill_exp_rate);
        let killer = source
            .filter(|source| source.kind == ObjectKind::Unit)
            .map(|source| source.id);
        match killer {
            Some(unit) => {
                if let Some(formation) = self.may_gain(unit) {
                    self.gain(formation, base);
                }
            }
            None => pool = pool.saturating_add(base),
        }
        let mut shared = Vec::<u64>::new();
        for attacker in self
            .exp
            .attackers
            .get(&target.object_ref())
            .cloned()
            .unwrap_or_default()
        {
            if attacker.kind != ObjectKind::Unit {
                continue;
            }
            if let Some(formation) = self.may_gain(attacker.id)
                && !shared.contains(&formation)
            {
                shared.push(formation);
            }
        }
        // A missile's projectile is no one's: the side the hit is recorded
        // under is the killer's. An air drop has no owner at all, and what it
        // kills counts for the dead one's enemies, whichever side dropped
        // it: a Vulcan landing among its own Crawlers hands the other side
        // their experience.
        let side = match source {
            Some(source) => self.side_of(source).or(Some(source_team)),
            None => self.side_of(target.object_ref()).map(|team| team ^ 1),
        };
        if let Some(side) = side
            && side != self.side_of(target.object_ref()).unwrap_or(side ^ 1)
        {
            self.add_nearby(side, target, &mut shared)?;
            // A kill no unit made, with no one to share it, goes to every unit
            // of the killer's side that may take experience.
            if killer.is_none() && shared.is_empty() {
                let units = self
                    .actors
                    .values()
                    .filter(|actor| actor.placement.team == side)
                    .map(|actor| actor.placement.unit_id)
                    .collect::<Vec<_>>();
                for unit in units {
                    if let Some(formation) = self.may_gain(unit)
                        && !shared.contains(&formation)
                    {
                        shared.push(formation);
                    }
                }
            }
        }
        if shared.is_empty() {
            return Ok(());
        }
        let count = i64::try_from(shared.len()).map_err(|_| Error::new("too many sharers"))?;
        let share = q32_div(pool, count << 32);
        for formation in shared {
            self.gain(formation, share);
        }
        self.exp.attackers.remove(&target.object_ref());
        Ok(())
    }

    fn side_of(&self, object: ObjectRef) -> Option<u32> {
        match object.kind {
            ObjectKind::Unit => self
                .actors
                .get(&object.id)
                .map(|actor| actor.placement.team),
            ObjectKind::Building => self
                .buildings
                .iter()
                .find(|building| building.building_id == object.id)
                .map(|building| building.team_id),
            _ => None,
        }
    }

    /// `ExpSystem.AddRangeUnit`: every formation of the side with a unit its
    /// unit quadtree finds for a square `assistExpRange` wide around the
    /// target, and whose edge stands within `assistExpRange` of the target's
    /// centre by `FPoint.op_LessThanOrEqual`. The quadtree answers with whole
    /// nodes, so what it finds depends on how the side's tree has split; alive
    /// or dead is not asked, only whether the unit is still in the tree.
    fn add_nearby(&self, side: u32, target: FightActorRef, shared: &mut Vec<u64>) -> Result<()> {
        let (x, z) = match target {
            FightActorRef::Unit(id) => {
                let actor = &self.actors[&id];
                (actor.x_q32, actor.z_q32)
            }
            FightActorRef::Building(id) => self
                .buildings
                .iter()
                .find(|building| building.building_id == id)
                .map(|building| (building.position.x, building.position.z))
                .ok_or_else(|| Error::new("a killed building is absent"))?,
        };
        let range = self.exp.table.assist_exp_range << 32;
        let Some(tree) = self.mech_quadtrees.get(&side) else {
            return Ok(());
        };
        for found in tree.query_square(x, z, range) {
            let FightActorRef::Unit(id) = found else {
                continue;
            };
            let Some(actor) = self.actors.get(&id) else {
                continue;
            };
            // `ExpSystem.IsValidOwner`: a summon has no formation to take a
            // share, and does not thin the others'.
            if actor.summoned {
                continue;
            }
            let formation = actor.placement.formation_id;
            if shared.contains(&formation) {
                continue;
            }
            let distance =
                native_q32_magnitude(actor.x_q32.wrapping_sub(x), actor.z_q32.wrapping_sub(z));
            let edge = distance.wrapping_sub(space_to_q32(actor.rules.collision_radius()));
            if fpoint_less_or_equal(edge, range) {
                shared.push(formation);
            }
        }
        Ok(())
    }
}
