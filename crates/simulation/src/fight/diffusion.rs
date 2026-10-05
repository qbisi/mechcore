//! `DamagePerformer.PerformDiffusionRangeEffect`: a splash that grows from
//! where it lands a step at a time, on a `GRTimer` of its own.
//!
//! The timer fires every `diffusionInteval`, from the update after the blow;
//! the `n`th firing reaches `n` times `diffusionSpeed`, no further than the
//! skill's splash, and strikes what it reaches that no earlier firing struck
//! (`DiffusionIntevalCallBack`, `PrepareRangeTargetsInDiffusion`). It fires
//! once more than the steps the splash takes, the last firing striking
//! nothing (`DiffusionCompleteCallBack`). Disintegration's wave reaches 30
//! metres further every half second until its 280: a Rhino whose edge stands
//! 85 metres from the Abyss takes it at the third firing, a second and a half
//! after the blow.

use super::damage::{DamageHit, Reach};
use super::*;

/// One splash under way, its performer's `m_diffusionTimer` and
/// `diffusionDamagedTargets`.
#[derive(Debug, Clone)]
pub(in crate::fight) struct Diffusion {
    /// The skill whose blow it is.
    skill_ref: SkillRef,
    /// The blow, as it landed: where, for how much, of which side.
    hit: DamageHit,
    /// The skill's whole splash, and how much further each firing reaches,
    /// in space units.
    splash_radius: i64,
    step_radius: i64,
    /// `GRTimer.Interval` and `RepeatCount`.
    interval: u64,
    repeat: u64,
    /// `GRTimer.CurrentTime` and `CurrentCount`.
    time: u64,
    count: u64,
    /// What it has struck, in the order it struck them.
    struck: Vec<FightActorRef>,
}

impl Simulation {
    /// `PerformDiffusionRangeEffect`: the timer of a blow whose splash
    /// diffuses, started as the blow lands. Its steps are the splash over
    /// the speed, rounded down, and one more for what that leaves; it fires
    /// once more than that.
    pub(in crate::fight) fn start_diffusion(
        &mut self,
        skill_ref: SkillRef,
        hit: DamageHit,
        diffusion: crate::rules::Diffusion,
    ) -> Result<()> {
        if !self.shield.standing.is_empty() {
            return Err(Error::new(
                "a diffusing splash in a fight with a battlefield shield is not measured",
            ));
        }
        let targets = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("a diffusing skill's owner is absent"))?
            .targets;
        let step_radius = diffusion.step_radius();
        if step_radius <= 0 {
            return Err(Error::new("a diffusing splash that does not grow"));
        }
        let steps =
            hit.splash_radius / step_radius + i64::from(hit.splash_radius % step_radius != 0);
        self.diffusions.push(Diffusion {
            skill_ref,
            // `PrepareRangeTargetsInDiffusion` takes what the splash reaches
            // alone, of the domains the skill attacks
            // (`SkillDamageProvider.GetTargetType` of a skill that diffuses).
            hit: DamageHit {
                aimed: None,
                hits_aimed: false,
                reach: Reach::Targets(targets),
                ..hit
            },
            splash_radius: hit.splash_radius,
            step_radius,
            interval: seconds_q32_to_steps(crate::rules::metres_q32(diffusion.interval)),
            repeat: u64::try_from(steps).unwrap_or(0) + 1,
            time: 0,
            count: 0,
            struck: Vec::new(),
        });
        Ok(())
    }

    /// `GRTimerManager.Update` of every splash under way, in the order they
    /// started: each timer a tick on, and its callback where it is due.
    pub(in crate::fight) fn update_diffusions(&mut self, events: &mut Vec<Event>) -> Result<()> {
        let mut index = 0;
        while index < self.diffusions.len() {
            let diffusion = &mut self.diffusions[index];
            diffusion.time += 1;
            if diffusion.time < diffusion.interval {
                index += 1;
                continue;
            }
            diffusion.count += 1;
            if diffusion.count >= diffusion.repeat {
                self.diffusions.remove(index);
                continue;
            }
            diffusion.time -= diffusion.interval;
            self.diffuse(index, events)?;
            index += 1;
        }
        Ok(())
    }

    /// `DiffusionIntevalCallBack`: nothing while the skill's unit is dead;
    /// otherwise the splash this far, on what it reaches that it has not
    /// struck, which then takes the skill's hit effects
    /// (`DispatchHitDamageEvent`): Disintegration's buff.
    fn diffuse(&mut self, index: usize, events: &mut Vec<Event>) -> Result<()> {
        let diffusion = &self.diffusions[index];
        let (skill_ref, struck) = (diffusion.skill_ref, &diffusion.struck);
        let owner_alive = skill_ref
            .owner
            .unit_id()
            .and_then(|id| self.actors.get(&id))
            .is_some_and(|actor| actor.life > 0);
        if !owner_alive {
            return Ok(());
        }
        let count = i64::try_from(diffusion.count).unwrap_or(i64::MAX);
        let hit = DamageHit {
            splash_radius: count
                .saturating_mul(diffusion.step_radius)
                .min(diffusion.splash_radius),
            ..diffusion.hit
        };
        let targets = self
            .damage_targets(&hit)?
            .into_iter()
            .filter(|target| !struck.contains(target))
            .collect::<Vec<_>>();
        self.diffusions[index].struck.extend(&targets);
        let struck = self.strike_targets(&hit, targets, events)?;
        let center = (hit.center_q32.0, hit.center_y_q32, hit.center_q32.1);
        self.extra_hit_effect(skill_ref, &struck.targets, center, events)?;
        self.record_ends(struck.ends, events);
        Ok(())
    }
}
