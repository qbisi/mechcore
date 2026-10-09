//! `MechGrounpSystem`: the units a damage-share technology links into groups
//! that share every hit any of them takes.
//!
//! Each side's `TeamMechGroupManager` holds the units its technology reaches,
//! by unit type (`groupedMeches`), and the groups they form. As the fight
//! starts each type is one group (`RebuildGroup`), its members chained by
//! nearest ends squad by squad (`MechGroupInternal.Refresh`), and every update
//! (`DoRefresh`, which `Update` falls through to) each unit splits its group
//! into the parts still linked (`TrySplitGroup`), joins the groups it reaches
//! and takes in the ungrouped units it reaches (`UpdateGroupInfo`). Two units
//! are linked when the edges of their bodies are within the share distance,
//! by `FPoint.op_LessThanOrEqual`.
//!
//! A hit on a grouped unit is shared (`FightCalculator.CalculateGroupDamage`):
//! its damage, after the target's rate on damage taken, reduction and
//! stealth, divided by the group's count, dead members counted, is dealt to
//! each member alive in the group's order, a hit of its own that no rate,
//! reduction or further share touches ([`Simulation::strike`]). The group
//! writes nothing on its members. `docs/rules/technology_effects.md` states
//! the rule.

use super::*;

/// `FPoint.C1em5`, the tolerance of `FPoint.Approximately`.
const APPROXIMATELY: i64 = 0xA7C5;

/// `MechGrounpSystem`'s state, both sides'.
#[derive(Debug, Clone, Default)]
pub(in crate::fight) struct MechGroupSystem {
    /// `TeamMechGroupManager.damageShareDistance`: one static the last unit
    /// `PrepareAvaliableMechs` accepted wrote, whichever side's.
    share_distance_q32: i64,
    /// Each side's manager, by its team.
    teams: BTreeMap<u32, TeamGroups>,
    /// Each held unit's `MechGroupDistance`.
    distances: BTreeMap<u64, i64>,
    /// Each group's members, in its order, by an identity of the
    /// simulator's own.
    groups: BTreeMap<u64, Vec<u64>>,
    /// `FightMech.mechGroup`.
    member_of: BTreeMap<u64, u64>,
    next_group: u64,
}

/// One side's `TeamMechGroupManager`.
#[derive(Debug, Clone, Default)]
struct TeamGroups {
    /// `groupedMeches`: the units held, by unit type, keys ascending.
    grouped: BTreeMap<u32, Vec<u64>>,
    /// `groups`, in the manager's order.
    order: Vec<u64>,
}

/// `FPoint.Approximately`.
const fn approximately(left: i64, right: i64) -> bool {
    const SENTINEL: i64 = i64::MIN + 1;
    left != SENTINEL && right != SENTINEL && left.abs_diff(right) <= APPROXIMATELY as u64
}

impl Simulation {
    /// `FightActor.Distance2D`: edge to edge, never below zero.
    fn group_distance(&self, one: u64, two: u64) -> i64 {
        let (one, two) = (&self.actors[&one], &self.actors[&two]);
        math::native_q32_magnitude(
            two.x_q32.saturating_sub(one.x_q32),
            two.z_q32.saturating_sub(one.z_q32),
        )
        .saturating_sub(space_to_q32(one.rules.collision_radius()))
        .saturating_sub(space_to_q32(two.rules.collision_radius()))
        .max(0)
    }

    fn within_share(&self, one: u64, two: u64) -> bool {
        math::fpoint_less_or_equal(
            self.group_distance(one, two),
            self.mech_groups.share_distance_q32,
        )
    }

    /// `FightUtility.ActorComparer`: world `z`, unless `FPoint`'s tolerance
    /// finds the two equal, and `x` then.
    fn group_order(&self, one: u64, two: u64) -> Ordering {
        let (one, two) = (&self.actors[&one], &self.actors[&two]);
        if one.z_q32.abs_diff(two.z_q32) > 43 {
            one.z_q32.cmp(&two.z_q32)
        } else {
            one.x_q32.cmp(&two.x_q32)
        }
    }

    /// `List.Sort` by [`Self::group_order`], which is no total order: each
    /// unit is inserted after every unit it does not precede.
    fn sort_group(&self, units: &[u64]) -> Vec<u64> {
        let mut sorted: Vec<u64> = Vec::with_capacity(units.len());
        for &unit in units {
            let at = sorted
                .iter()
                .rposition(|&placed| self.group_order(placed, unit) != Ordering::Greater)
                .map_or(0, |index| index + 1);
            sorted.insert(at, unit);
        }
        sorted
    }

    /// `TeamMechGroupManager.LinkMeches`: `target` joined to `source` by the
    /// two ends nearest each other, `FPoint.Min` of the four distances
    /// keeping an earlier one a later one is not 44 raw below.
    fn link_units(&self, source: &mut Vec<u64>, mut target: Vec<u64>) {
        let (Some(&source_first), Some(&target_first)) = (source.first(), target.first()) else {
            source.extend(target);
            return;
        };
        let source_last = *source.last().expect("a list with a first has a last");
        let target_last = *target.last().expect("a list with a first has a last");
        let a = self.group_distance(source_first, target_first);
        let b = self.group_distance(source_first, target_last);
        let c = self.group_distance(source_last, target_first);
        let d = self.group_distance(source_last, target_last);
        let nearest = [b, c, d].into_iter().fold(a, |nearest, distance| {
            if rvo::fpoint_less_than(distance, nearest) {
                distance
            } else {
                nearest
            }
        });
        if approximately(nearest, c) {
            source.extend(target);
        } else if approximately(nearest, b) {
            source.splice(0..0, target);
        } else if approximately(nearest, d) {
            target.reverse();
            source.extend(target);
        } else {
            if approximately(nearest, a) {
                source.reverse();
            }
            source.extend(target);
        }
    }

    /// `MechGrounpSystem.AddMech` and `TeamMechGroupManager.AddMech`: a
    /// unit's technology activates (`MechGrounpEffectProvider.DoActive`),
    /// writing its `MechGroupDistance`, and the side's manager refreshes.
    pub(in crate::fight) fn add_group_unit(&mut self, unit: u64) {
        let actor = &self.actors[&unit];
        let Some(distance) = actor.placement.share_distance else {
            return;
        };
        let (team, kind) = (actor.placement.team, actor.rules.unit_type_id);
        self.mech_groups.distances.insert(unit, distance);
        self.mech_groups
            .teams
            .entry(team)
            .or_default()
            .grouped
            .entry(kind)
            .or_default()
            .push(unit);
        self.refresh_groups(team);
    }

    /// `MechGrounpEffectProvider.DoDeactive` of a unit that died: its
    /// `MechGroupDistance` goes, and it leaves its group and its type's list
    /// (`TeamMechGroupManager.RemoveMech`), the side's manager refreshing.
    /// It no longer hears its side change (`MechGrounpSystem.RemoveMech`).
    pub(in crate::fight) fn remove_group_unit(&mut self, unit: u64) {
        if self.mech_groups.distances.remove(&unit).is_none() {
            return;
        }
        self.leave_side_groups(self.actors[&unit].placement.team, unit);
    }

    /// `MechGrounpSystem.ChangeMechGroup`, as a beam turns a held unit: it
    /// leaves its old side's manager and joins its new side's, its
    /// `MechGroupDistance` kept, each manager refreshing.
    pub(in crate::fight) fn change_group_side(&mut self, unit: u64, old_team: u32) {
        if !self.mech_groups.distances.contains_key(&unit) {
            return;
        }
        self.leave_side_groups(old_team, unit);
        let actor = &self.actors[&unit];
        let (team, kind) = (actor.placement.team, actor.rules.unit_type_id);
        self.mech_groups
            .teams
            .entry(team)
            .or_default()
            .grouped
            .entry(kind)
            .or_default()
            .push(unit);
        self.refresh_groups(team);
    }

    /// `TeamMechGroupManager.RemoveMech` on one side's manager.
    fn leave_side_groups(&mut self, team: u32, unit: u64) {
        let kind = self.actors[&unit].rules.unit_type_id;
        if let Some(group) = self.mech_groups.member_of.get(&unit).copied() {
            self.group_remove(group, unit);
            if self.mech_groups.groups[&group].is_empty() {
                self.destroy_group(team, group);
            }
        }
        if let Some(units) = self
            .mech_groups
            .teams
            .get_mut(&team)
            .and_then(|side| side.grouped.get_mut(&kind))
            && let Some(index) = units.iter().position(|&held| held == unit)
        {
            units.remove(index);
        }
        self.refresh_groups(team);
    }

    /// `TeamMechGroupManager.OnFightStart` of every side: `RebuildGroup`,
    /// one group of every type's units, then `Refresh`.
    pub(in crate::fight) fn start_groups(&mut self) {
        let held = self
            .actors
            .iter()
            .filter(|(_, actor)| !actor.travelling && actor.placement.share_distance.is_some())
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        for unit in held {
            self.add_group_unit(unit);
        }
        for team in self.mech_groups.teams.keys().copied().collect::<Vec<_>>() {
            for group in self.mech_groups.teams[&team]
                .order
                .clone()
                .into_iter()
                .rev()
            {
                self.destroy_group(team, group);
            }
            self.sort_grouped(team);
            let side = self.mech_groups.teams.get_mut(&team).expect("a side held");
            side.grouped.retain(|_, units| !units.is_empty());
            for units in side.grouped.values().cloned().collect::<Vec<_>>() {
                self.create_group(team, units);
            }
            self.refresh_groups(team);
        }
    }

    /// `TeamMechGroupManager.Update`, which falls through to `DoRefresh`, of
    /// every side, as `MechGrounpSystem` updates: after
    /// `AdvancedEnergyShieldSystem` and before `RangeItemSystem`.
    pub(in crate::fight) fn step_groups(&mut self) {
        for team in self.mech_groups.teams.keys().copied().collect::<Vec<_>>() {
            self.update_groups(team);
        }
    }

    /// `TeamMechGroupManager.RefreshGroupedMeches`: each type's units by
    /// `ActorComparer`.
    fn sort_grouped(&mut self, team: u32) {
        let lists = self.mech_groups.teams[&team].grouped.clone();
        for (kind, units) in lists {
            let sorted = self.sort_group(&units);
            self.mech_groups
                .teams
                .get_mut(&team)
                .expect("a side held")
                .grouped
                .insert(kind, sorted);
        }
    }

    /// `TeamMechGroupManager.Refresh`: each group put back in order, or gone
    /// when empty, the groups by their first members, then `DoRefresh`.
    fn refresh_groups(&mut self, team: u32) {
        self.sort_grouped(team);
        for group in self.mech_groups.teams[&team].order.clone() {
            if self.mech_groups.groups[&group].is_empty() {
                self.destroy_group(team, group);
            } else {
                self.reorder_group(group);
            }
        }
        let order = self.mech_groups.teams[&team].order.clone();
        let firsts = order
            .iter()
            .map(|group| self.mech_groups.groups[group][0])
            .collect::<Vec<_>>();
        let sorted = self.sort_group(&firsts);
        let order = sorted
            .into_iter()
            .map(|first| {
                order
                    .iter()
                    .copied()
                    .find(|group| self.mech_groups.groups[group][0] == first)
                    .expect("each first is a group's")
            })
            .collect();
        self.mech_groups
            .teams
            .get_mut(&team)
            .expect("a side held")
            .order = order;
        self.update_groups(team);
    }

    /// `TeamMechGroupManager.DoRefresh`: `UpdateGroupInfo` of every unit
    /// held, type by type, each list read afresh as it goes.
    fn update_groups(&mut self, team: u32) {
        let kinds = self.mech_groups.teams[&team]
            .grouped
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for kind in kinds {
            let mut index = 0;
            while let Some(&unit) = self.mech_groups.teams[&team]
                .grouped
                .get(&kind)
                .and_then(|units| units.get(index))
            {
                self.update_group_info(team, kind, unit);
                index += 1;
            }
        }
    }

    /// `TeamMechGroupManager.PrepareAvaliableMechs`: the units of a type with
    /// a share distance, up to the first dead one, each writing the static
    /// distance as it is taken.
    fn available_group_units(&mut self, team: u32, kind: u32) -> Vec<u64> {
        let mut available = Vec::new();
        for unit in self.mech_groups.teams[&team].grouped[&kind].clone() {
            let distance = self.mech_groups.distances.get(&unit).copied().unwrap_or(0);
            if approximately(distance, 0) {
                continue;
            }
            if !self.actors[&unit].alive() {
                break;
            }
            self.mech_groups.share_distance_q32 = distance;
            available.push(unit);
        }
        available
    }

    /// `TeamMechGroupManager.UpdateGroupInfo`.
    fn update_group_info(&mut self, team: u32, kind: u32, source: u64) {
        // `IsIgnoredMech`.
        if approximately(
            self.mech_groups
                .distances
                .get(&source)
                .copied()
                .unwrap_or(0),
            0,
        ) {
            return;
        }
        let available = self.available_group_units(team, kind);
        if let Some(group) = self.mech_groups.member_of.get(&source).copied() {
            self.split_group(team, group);
        }
        let own = self.mech_groups.member_of.get(&source).copied();
        let linked = self.mech_groups.teams[&team]
            .order
            .iter()
            .copied()
            .filter(|&group| Some(group) != own && self.connects(source, group))
            .collect::<Vec<_>>();
        let near = available
            .into_iter()
            .filter(|&unit| {
                unit != source
                    && !self.mech_groups.member_of.contains_key(&unit)
                    && self.within_share(unit, source)
            })
            .collect::<Vec<_>>();
        if linked.is_empty() && near.is_empty() {
            return;
        }
        if own.is_none()
            && let Some(&first) = linked.first()
        {
            self.group_add(first, source);
        }
        for group in linked {
            let own = self.mech_groups.member_of[&source];
            if own != group {
                self.group_link(own, group);
                self.destroy_group(team, group);
            }
        }
        for unit in near {
            match self.mech_groups.member_of.get(&source).copied() {
                None => self.create_group(team, vec![source, unit]),
                Some(own) => self.group_add(own, unit),
            }
        }
    }

    /// `TeamMechGroupManager.CheckConnection`: a group of the unit's type with
    /// a member within the share distance of it.
    fn connects(&self, unit: u64, group: u64) -> bool {
        let members = &self.mech_groups.groups[&group];
        let Some(&first) = members.first() else {
            return false;
        };
        self.actors[&first].rules.unit_type_id == self.actors[&unit].rules.unit_type_id
            && members
                .iter()
                .any(|&member| self.within_share(unit, member))
    }

    /// `TeamMechGroupManager.TrySplitGroup`: the group's members in the parts
    /// still linked (`GenerateSimpleGroups`, `SimpleGroup.CombineGroups`). A
    /// member alone leaves; the group keeps the first part and every other
    /// becomes a group of its own.
    fn split_group(&mut self, team: u32, group: u64) {
        let mut parts: Vec<Vec<u64>> = Vec::new();
        for unit in self.mech_groups.groups[&group].clone() {
            // `SimpleGroup.TryAdd` and `DoAdd`.
            let joined = parts
                .iter()
                .position(|part| part.iter().any(|&member| self.within_share(member, unit)));
            match joined {
                Some(index) => {
                    let part = &parts[index];
                    let (first, last) = (part[0], part[part.len() - 1]);
                    if part.len() < 2
                        || math::fpoint_less_or_equal(
                            self.group_distance(unit, last),
                            self.group_distance(unit, first),
                        )
                    {
                        parts[index].push(unit);
                    } else {
                        parts[index].insert(0, unit);
                    }
                }
                None => parts.push(vec![unit]),
            }
        }
        // `SimpleGroup.CombineGroups`: one pass, each part taking in every
        // other it reaches.
        for one in 0..parts.len() {
            if parts[one].is_empty() {
                continue;
            }
            for two in 0..parts.len() {
                if one == two || parts[two].is_empty() {
                    continue;
                }
                let reaches = parts[one]
                    .iter()
                    .any(|&a| parts[two].iter().any(|&b| self.within_share(a, b)));
                if reaches {
                    let taken = std::mem::take(&mut parts[two]);
                    let mut joined = std::mem::take(&mut parts[one]);
                    self.link_units(&mut joined, taken);
                    parts[one] = joined;
                }
            }
        }
        parts.retain(|part| !part.is_empty());
        for index in (0..parts.len()).rev() {
            if parts[index].len() == 1 {
                if !self.mech_groups.groups[&group].is_empty() {
                    self.group_remove(group, parts[index][0]);
                }
                parts.remove(index);
            }
        }
        if parts.is_empty() {
            self.destroy_group(team, group);
        } else if parts.len() >= 2 {
            let mut parts = parts.into_iter();
            let first = parts.next().expect("two parts");
            self.group_set(group, first);
            for part in parts {
                self.create_group(team, part);
            }
        }
    }

    /// `TeamMechGroupManager.CreateGroup`: a new group, last in the side's.
    fn create_group(&mut self, team: u32, members: Vec<u64>) {
        let group = self.mech_groups.next_group;
        self.mech_groups.next_group += 1;
        self.mech_groups
            .teams
            .get_mut(&team)
            .expect("a side held")
            .order
            .push(group);
        self.mech_groups.groups.insert(group, Vec::new());
        self.group_set(group, members);
    }

    /// `TeamMechGroupManager.DestroyGroup`: out of the side's groups. Its
    /// members are not told.
    fn destroy_group(&mut self, team: u32, group: u64) {
        if let Some(side) = self.mech_groups.teams.get_mut(&team) {
            side.order.retain(|&held| held != group);
        }
    }

    /// `MechGroupInternal.Init` and `SetGroupElement`.
    fn group_set(&mut self, group: u64, members: Vec<u64>) {
        for &unit in &members {
            self.mech_groups.member_of.insert(unit, group);
        }
        self.mech_groups.groups.insert(group, members);
    }

    /// `MechGroupInternal.Add`: at the end nearer the unit.
    fn group_add(&mut self, group: u64, unit: u64) {
        let members = &self.mech_groups.groups[&group];
        let (first, last) = (members[0], members[members.len() - 1]);
        let to_first = self.group_distance(unit, first);
        let to_last = self.group_distance(unit, last);
        let members = self
            .mech_groups
            .groups
            .get_mut(&group)
            .expect("a group held");
        if rvo::fpoint_less_than(to_first, to_last) {
            members.insert(0, unit);
        } else {
            members.push(unit);
        }
        self.mech_groups.member_of.insert(unit, group);
    }

    /// `MechGroupInternal.Remove`: the members before and after it chained
    /// again by their nearest ends, and the group emptied once fewer than two
    /// are left.
    fn group_remove(&mut self, group: u64, unit: u64) {
        self.mech_groups.member_of.remove(&unit);
        let mut members = std::mem::take(
            self.mech_groups
                .groups
                .get_mut(&group)
                .expect("a group held"),
        );
        if let Some(index) = members.iter().position(|&member| member == unit) {
            let tail = members.split_off(index + 1);
            members.truncate(index);
            self.link_units(&mut members, tail);
        }
        if members.len() < 2 {
            for member in members.drain(..) {
                self.mech_groups.member_of.remove(&member);
            }
        }
        self.mech_groups.groups.insert(group, members);
    }

    /// `MechGroupInternal.LinkGroup`: another group's members chained onto
    /// this one's.
    fn group_link(&mut self, group: u64, other: u64) {
        let taken = std::mem::take(
            self.mech_groups
                .groups
                .get_mut(&other)
                .expect("a group held"),
        );
        for &unit in &taken {
            self.mech_groups.member_of.insert(unit, group);
        }
        let mut members = std::mem::take(
            self.mech_groups
                .groups
                .get_mut(&group)
                .expect("a group held"),
        );
        self.link_units(&mut members, taken);
        self.mech_groups.groups.insert(group, members);
    }

    /// `MechGroupInternal.Refresh`: the members by `ActorComparer`, squad by
    /// squad in the order each squad first comes, each squad chained on by
    /// its nearest ends. A unit a beam turned is a squad of its own.
    fn reorder_group(&mut self, group: u64) {
        let sorted = self.sort_group(&self.mech_groups.groups[&group]);
        let mut squads: Vec<(Option<u64>, u64, Vec<u64>)> = Vec::new();
        for unit in sorted {
            let actor = &self.actors[&unit];
            let key = (actor.placement.team == actor.original_team)
                .then_some(actor.placement.formation_id);
            match squads
                .iter_mut()
                .find(|(squad, alone, _)| *squad == key && (key.is_some() || *alone == unit))
            {
                Some((_, _, members)) => members.push(unit),
                None => squads.push((key, unit, vec![unit])),
            }
        }
        let mut members = Vec::new();
        for (_, _, squad) in squads {
            self.link_units(&mut members, squad);
        }
        self.mech_groups.groups.insert(group, members);
    }

    /// The members a hit on a unit is shared among, in the group's order,
    /// dead ones too: `FightCalculator.CalculateGroupDamage`'s, when the unit
    /// is grouped.
    pub(in crate::fight) fn sharing_group(&self, unit: u64) -> Option<Vec<u64>> {
        self.mech_groups
            .member_of
            .get(&unit)
            .map(|group| self.mech_groups.groups[group].clone())
            .filter(|members| !members.is_empty())
    }
}
