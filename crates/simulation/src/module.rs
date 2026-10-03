//! The fight's modules, and what each one is responsible for understanding.
//!
//! `docs/spec/simulation/architecture.md` is the contract. The build runs its
//! fight as 35 modules with one lifecycle, and this carries a module for each,
//! implemented or not. A module that is not implemented is still here: it
//! claims the layout fields it would understand and refuses them, which is what
//! makes the closure a property of this table rather than a hand-written list
//! of rejections.
//!
//! Adding a mechanism is filling in its module and listing the fields it now
//! understands. A module is not all or nothing: it can understand fewer
//! fields than it claims, and refuse the rest until their mechanism lands. The
//! loop that drives them is never edited for a mechanism.
//!
//! A unit's level is not a field here. It is part of the unit, as its type is:
//! `FightMech` is built with its `IMechLevelData`, and the level scales the
//! base numbers before any module's correction reaches them.
//!
//! A module that understands a field can still refuse one member of it, and
//! the refusal then names the thing rather than the field:
//! `FightConstructionSystem` places a Defensive Wall and a turret and refuses
//! a Magnetic Barrier, because where its two rows of objects stand has not
//! been measured.

use mechcore_document::SidePlan;

/// A layout field a fight has to understand before it can be fought.
///
/// Three of them sit on a unit rather than on the side, which is why a claim is
/// checked against the whole side plan rather than against a list of keys. A
/// layout's `blueprints` are not among them: compiling one applies each chain
/// as the officer it hands out, so a blueprint reaches the fight as an officer
/// and is refused as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Field {
    Officers,
    UnitTechnologies,
    EnergyTowerSkills,
    TowerStrengthenLevels,
    BattleSkills,
    Constructions,
    ShieldContraptions,
    InterceptorContraptions,
    MissileContraptions,
    StandingShields,
    StandingOil,
    UnitEquipment,
    Travelling,
}

impl Field {
    /// The document's own name for the field, so a refusal names what the
    /// caller wrote.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Officers => "officers",
            Self::UnitTechnologies => "unit technologies",
            Self::EnergyTowerSkills => "energy tower skills",
            Self::TowerStrengthenLevels => "tower strengthen levels",
            Self::BattleSkills => "battle skills",
            Self::Constructions => "constructions",
            Self::ShieldContraptions => "shield contraptions",
            Self::InterceptorContraptions => "interceptor contraptions",
            Self::MissileContraptions => "missile contraptions",
            Self::StandingShields => "standing shield_airdrop",
            Self::StandingOil => "standing sticky_oil_bomb",
            Self::UnitEquipment => "unit equipment",
            Self::Travelling => "travelling units",
        }
    }

    /// Whether a side plan carries this field.
    fn carried(self, side: &SidePlan) -> bool {
        match self {
            Self::Officers => !side.techs.officers.is_empty(),
            Self::UnitTechnologies => !side.techs.units.is_empty(),
            Self::EnergyTowerSkills => !side.energy_tower_skills.is_empty(),
            Self::TowerStrengthenLevels => {
                side.tower_strengthen_levels.iter().any(|level| *level != 0)
            }
            Self::BattleSkills => !side.battle_skills.is_empty(),
            Self::Constructions => !side.constructions.is_empty(),
            Self::ShieldContraptions => carries_contraption(side, "shield"),
            Self::InterceptorContraptions => carries_contraption(side, "interceptor"),
            Self::MissileContraptions => carries_contraption(side, "missile"),
            Self::StandingShields => !side.standing_shields.is_empty(),
            Self::StandingOil => !side.standing_oil.is_empty(),
            Self::UnitEquipment => side.units.iter().any(|unit| !unit.equipment.is_empty()),
            Self::Travelling => side.units.iter().any(|unit| unit.travelling),
        }
    }
}

/// Whether a side releases a contraption of this kind. The three kinds are
/// three mechanisms of the build: `CRC_EnergyShield.Perform` makes a shield
/// through `AdvancedEnergyShieldSystem.Create`, `CRC_Interceptor.Perform` an
/// interceptor through `InterceptSystem.DoCreateFightInterceptor`, and
/// `CRC_Mine.Perform` a missile through `MineSystem.Create`.
fn carries_contraption(side: &SidePlan, kind: &str) -> bool {
    side.contraptions
        .iter()
        .any(|contraption| contraption.type_name == kind)
}

/// One module of the fight.
pub(crate) struct Module {
    /// The build's name for it, which is what a refusal says and what
    /// `scripts/decomp/fight-structure.py` lists.
    pub(crate) native: &'static str,
    /// The layout fields this module is responsible for understanding.
    pub(crate) claims: &'static [Field],
    /// Which of its claims this build understands.
    ///
    /// A module is not all or nothing: it can understand fewer fields than it
    /// claims. A field left out of this list is refused exactly as an
    /// unimplemented module's claim is, so a side carrying it is still outside
    /// the closure.
    pub(crate) understood: &'static [Field],
    /// Whether this build of the simulator implements the module at all.
    pub(crate) implemented: bool,
}

/// The build's fight modules, and the one step that is not one of them.
///
/// Which module a field is claimed by is this simulator's arrangement; the
/// names are the build's. Two of the arrangements are the build's too:
/// `RangeItemSystem` owns terrain by `docs/rules/terrain.md`, and
/// `SuperDeploymentSystem` owns a travelling unit because
/// `FightCoreSystem.PreCalculate` asks it `IsTravelling`.
///
/// `Modifier` is the exception and is deliberately not a module of the build:
/// officers, technologies and equipment are applied to a unit before
/// the fight rather than inside it — the build's own
/// `TechnologySystem.AddTechnologyEffect` takes a `PlayerController` and is
/// called from the deployment's `MAP_AddUnit` — so the simulator applies them
/// as the fight is built, in one step of its own. It is named for what the
/// build calls the thing it writes: `Officer.AddData` reaches
/// `MechDataModifer.AddData` and `SkillDataModifier.AddData`, and every entry
/// carries the `IDataModifier` that put it there.
pub(crate) static MODULES: &[Module] = &[
    Module {
        native: "Modifier",
        claims: &[
            Field::Officers,
            Field::UnitTechnologies,
            Field::UnitEquipment,
        ],
        understood: &[
            Field::Officers,
            Field::UnitTechnologies,
            Field::UnitEquipment,
        ],
        implemented: true,
    },
    Module {
        native: "AdvancedEnergyShieldSystem",
        claims: &[Field::ShieldContraptions, Field::StandingShields],
        understood: &[Field::ShieldContraptions, Field::StandingShields],
        implemented: true,
    },
    Module {
        native: "AutoRecoverySystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "BuffSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "BuildingSystem",
        claims: &[Field::EnergyTowerSkills, Field::TowerStrengthenLevels],
        understood: &[Field::EnergyTowerSkills, Field::TowerStrengthenLevels],
        implemented: true,
    },
    Module {
        native: "BurrowSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "ClearRangeItemSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "CloakSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "CommanderSkillSystem",
        claims: &[Field::BattleSkills],
        understood: &[Field::BattleSkills],
        implemented: true,
    },
    Module {
        native: "DeadEffectSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "ExpSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "ExtraSkillSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "FightConstructionSystem",
        claims: &[Field::Constructions],
        understood: &[Field::Constructions],
        implemented: true,
    },
    // The only one that is implemented: units, their movement, their targets,
    // their attacks and what those do.
    Module {
        native: "FightCoreSystem",
        claims: &[],
        understood: &[],
        implemented: true,
    },
    Module {
        native: "FightEffectSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "FightGroupSystem",
        claims: &[],
        understood: &[],
        implemented: true,
    },
    Module {
        native: "FightTeamSystem",
        claims: &[],
        understood: &[],
        implemented: true,
    },
    Module {
        native: "InterceptSystem",
        claims: &[Field::InterceptorContraptions],
        understood: &[Field::InterceptorContraptions],
        implemented: true,
    },
    Module {
        native: "IterationEffectSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "LifeChangeEffectSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "MechGrounpSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "MineSystem",
        claims: &[Field::MissileContraptions],
        understood: &[Field::MissileContraptions],
        implemented: true,
    },
    Module {
        native: "MoveAbilityRangeItemSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "MoveAbilitySummonSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "ProjectileSystem",
        claims: &[],
        understood: &[],
        implemented: true,
    },
    Module {
        native: "RangeItemSystem",
        claims: &[Field::StandingOil],
        understood: &[Field::StandingOil],
        implemented: true,
    },
    Module {
        native: "ReactiveArmorSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "RecoveryEffectSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "SiegeModeEffectSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "StealthTechSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "SummonSystem",
        claims: &[],
        understood: &[],
        implemented: true,
    },
    Module {
        native: "SuperDeploymentSystem",
        claims: &[Field::Travelling],
        understood: &[Field::Travelling],
        implemented: true,
    },
    Module {
        native: "SupportUnitSystem",
        claims: &[],
        understood: &[],
        implemented: true,
    },
    Module {
        native: "TeamTranslationSystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "TechnologySystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
    Module {
        native: "WreckageRecoverySystem",
        claims: &[],
        understood: &[],
        implemented: false,
    },
];

/// Every field this side carries that no implemented module understands, with
/// the module that owes it.
///
/// A side inside the closure answers an empty list, which is the only thing a
/// caller has to test.
pub(crate) fn unsupported(side: &SidePlan) -> Vec<(Field, &'static str)> {
    let mut missing: Vec<(Field, &'static str)> = MODULES
        .iter()
        .flat_map(|module| {
            module
                .claims
                .iter()
                .filter(|field| {
                    let understood = module.implemented && module.understood.contains(field);
                    !understood && field.carried(side)
                })
                .map(|field| (*field, module.native))
        })
        .collect();
    missing.sort_unstable();
    missing
}

/// What a refusal says: every field at once, each with the module that owes it.
///
/// Naming all of them rather than the first is what lets a caller see how far a
/// deployment is from being fought, and what
/// `scripts/corpus/fight-coverage.py` counts.
pub(crate) fn refusal(side_name: &str, missing: &[(Field, &'static str)]) -> String {
    let listed = missing
        .iter()
        .map(|(field, module)| format!("{} ({module})", field.name()))
        .collect::<Vec<_>>()
        .join(", ");
    format!("side {side_name} needs modules this build has not implemented: {listed}")
}

#[cfg(test)]
mod tests {
    use super::{Field, MODULES, unsupported};

    /// Every field is claimed by exactly one module, and every claim is a
    /// field. A mechanism nobody owns would be refused by nothing.
    #[test]
    fn every_field_is_claimed_once() {
        let every = [
            Field::Officers,
            Field::UnitTechnologies,
            Field::EnergyTowerSkills,
            Field::TowerStrengthenLevels,
            Field::BattleSkills,
            Field::Constructions,
            Field::ShieldContraptions,
            Field::InterceptorContraptions,
            Field::MissileContraptions,
            Field::StandingShields,
            Field::StandingOil,
            Field::UnitEquipment,
            Field::Travelling,
        ];
        for field in every {
            let owners: Vec<_> = MODULES
                .iter()
                .filter(|module| module.claims.contains(&field))
                .map(|module| module.native)
                .collect();
            assert_eq!(owners.len(), 1, "{} is claimed by {owners:?}", field.name());
        }
        let claimed: usize = MODULES.iter().map(|module| module.claims.len()).sum();
        assert_eq!(claimed, every.len());
    }

    /// The build's own module list, which `scripts/decomp/fight-structure.py` reads
    /// out of the decompilation index. One of ours is not the build's, and
    /// [`MODULES`] says why.
    #[test]
    fn the_modules_are_the_builds_thirty_five_and_one_of_our_own() {
        assert_eq!(MODULES.len(), 36);
        assert_eq!(MODULES[0].native, "Modifier");
        let mut native: Vec<&str> = MODULES[1..].iter().map(|module| module.native).collect();
        let sorted = {
            let mut sorted = native.clone();
            sorted.sort_unstable();
            sorted
        };
        assert_eq!(
            native, sorted,
            "the build's modules are listed in name order"
        );
        native.dedup();
        assert_eq!(native.len(), 35);
    }

    /// An empty side is inside the closure, and so is one that carries a
    /// field of every kind the closure once lacked.
    #[test]
    fn every_field_a_side_carries_is_understood() {
        let plan = |yaml: &str| {
            let layout = mechcore_document::parse_yaml(yaml.as_bytes()).unwrap();
            mechcore_document::compile_layout(layout).unwrap()
        };
        let bare = plan(
            "kind: layout\nround: 1\nblue:\n  units: \
             [{name: marksman, index: 0, position: {x: 0, y: -50}}]\nred:\n  units: \
             [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n",
        );
        assert!(unsupported(&bare.blue).is_empty());

        let loaded = plan(
            "kind: layout\nround: 1\nblue:\n  officers: [supply_specialist]\n  \
             constructions: [{name: defensive_wall, index: 0, position: {x: 140, y: -105}}]\n  \
             battle_skills: [{name: missile_strike, positions: [{x: 0, y: 40}]}, \
             {name: sticky_oil_bomb, standing: {control_points: [{x: -60, y: 40}, {x: 60, y: 40}]}}]\n  \
             units: [{name: marksman, index: 0, position: {x: 0, y: -50}, level: 3}]\nred:\n  \
             units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n",
        );
        assert!(
            unsupported(&loaded.blue).is_empty(),
            "officers, a unit's level, the construction, the released battle skill \
             and the oil standing from an earlier round are all understood"
        );
    }

    /// Each kind of contraption is owed by the module that makes it, and all
    /// three are understood, as is a Shield Airdrop an earlier round left
    /// standing, which the shield module owes too.
    #[test]
    fn a_contraption_is_owed_by_the_module_of_its_kind() {
        let layout = mechcore_document::parse_yaml(
            "kind: layout\nround: 1\nblue:\n  units: \
             [{name: marksman, index: 0, position: {x: 0, y: -50}}]\n  contraptions: \
             [{name: interceptor, index: 0, position: {x: 5, y: -95}}, \
             {name: missile, index: 1, position: {x: 1, y: -11}}, \
             {name: shield, index: 2, position: {x: 1, y: -81}}]\n  battle_skills: \
             [{name: shield_airdrop, standing: {position: {x: 0, y: -60}}}]\nred:\n  units: \
             [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n"
                .as_bytes(),
        )
        .unwrap();
        let plan = mechcore_document::compile_layout(layout).unwrap();
        assert_eq!(
            unsupported(&plan.blue)
                .iter()
                .map(|(field, module)| (field.name(), *module))
                .collect::<Vec<_>>(),
            [] as [(&str, &str); 0]
        );
    }

    /// A module understands only what it claims, and only if it is
    /// implemented at all. Either would otherwise let a field through that
    /// nothing applies.
    #[test]
    fn what_a_module_understands_is_a_subset_of_what_it_claims() {
        for module in MODULES {
            for field in module.understood {
                assert!(
                    module.claims.contains(field),
                    "{} understands {} without claiming it",
                    module.native,
                    field.name()
                );
                assert!(
                    module.implemented,
                    "{} understands {} while unimplemented",
                    module.native,
                    field.name()
                );
            }
        }
    }
}
