//! The fight's modules, and what each one is responsible for understanding.
//!
//! `docs/spec/simulation/architecture.md` is the contract. Build 2259 runs its
//! fight as 35 modules with one lifecycle, and this carries a module for each,
//! implemented or not. A module that is not implemented is still here: it
//! claims the layout fields it would understand and refuses them, which is what
//! makes the closure a property of this table rather than a hand-written list
//! of rejections.
//!
//! Adding a mechanism is filling in its module and setting `implemented`. The
//! loop that drives them is never edited for a mechanism.

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
    Contraptions,
    AirdropShields,
    Terrains,
    UnitLevel,
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
            Self::Contraptions => "contraptions",
            Self::AirdropShields => "airdrop shields",
            Self::Terrains => "terrains",
            Self::UnitLevel => "units above level one",
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
            Self::Contraptions => !side.contraptions.is_empty(),
            Self::AirdropShields => !side.airdrop_shields.is_empty(),
            Self::Terrains => !side.terrains.is_empty(),
            Self::UnitLevel => side.units.iter().any(|unit| unit.level != Some(1)),
            Self::UnitEquipment => side.units.iter().any(|unit| unit.equipment.is_some()),
            Self::Travelling => side.units.iter().any(|unit| unit.travelling),
        }
    }
}

/// One module of the fight.
pub(crate) struct Module {
    /// The build's name for it, which is what a refusal says and what
    /// `scripts/fight-structure.py` lists.
    pub(crate) native: &'static str,
    /// The layout fields this module is responsible for understanding.
    pub(crate) claims: &'static [Field],
    /// Whether this build of the simulator implements it.
    pub(crate) implemented: bool,
}

/// Build 2259's fight modules, and the one step that is not one of them.
///
/// Which module a field is claimed by is this simulator's arrangement; the
/// names are the build's. Two of the arrangements are the build's too:
/// `RangeItemSystem` owns terrain by `docs/rules/terrain.md`, and
/// `SuperDeploymentSystem` owns a travelling unit because
/// `FightCoreSystem.PreCalculate` asks it `IsTravelling`.
///
/// `Loadout` is the exception and is deliberately not a module of the build:
/// officers, technologies, equipment and levels are applied to a unit before
/// the fight rather than inside it — the build's own
/// `TechnologySystem.AddTechnologyEffect` takes a `PlayerController` and is
/// called from the deployment's `MAP_AddUnit` — so the simulator applies them
/// as the fight is built, in one step of its own.
pub(crate) static MODULES: &[Module] = &[
    Module {
        native: "Loadout",
        claims: &[
            Field::Officers,
            Field::UnitTechnologies,
            Field::UnitEquipment,
            Field::UnitLevel,
        ],
        implemented: false,
    },
    Module {
        native: "AdvancedEnergyShieldSystem",
        claims: &[Field::AirdropShields],
        implemented: false,
    },
    Module {
        native: "AutoRecoverySystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "BuffSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "BuildingSystem",
        claims: &[Field::EnergyTowerSkills, Field::TowerStrengthenLevels],
        implemented: false,
    },
    Module {
        native: "BurrowSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "ClearRangeItemSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "CloakSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "CommanderSkillSystem",
        claims: &[Field::BattleSkills],
        implemented: false,
    },
    Module {
        native: "DeadEffectSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "ExpSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "ExtraSkillSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "FightConstructionSystem",
        claims: &[Field::Constructions],
        implemented: false,
    },
    // The only one that is implemented: units, their movement, their targets,
    // their attacks and what those do.
    Module {
        native: "FightCoreSystem",
        claims: &[],
        implemented: true,
    },
    Module {
        native: "FightEffectSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "FightGroupSystem",
        claims: &[],
        implemented: true,
    },
    Module {
        native: "FightTeamSystem",
        claims: &[],
        implemented: true,
    },
    Module {
        native: "InterceptSystem",
        claims: &[Field::Contraptions],
        implemented: false,
    },
    Module {
        native: "IterationEffectSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "LifeChangeEffectSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "MechGrounpSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "MineSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "MoveAbilityRangeItemSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "MoveAbilitySummonSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "ProjectileSystem",
        claims: &[],
        implemented: true,
    },
    Module {
        native: "RangeItemSystem",
        claims: &[Field::Terrains],
        implemented: false,
    },
    Module {
        native: "ReactiveArmorSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "RecoveryEffectSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "SiegeModeEffectSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "StealthTechSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "SummonSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "SuperDeploymentSystem",
        claims: &[Field::Travelling],
        implemented: false,
    },
    Module {
        native: "SupportUnitSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "TeamTranslationSystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "TechnologySystem",
        claims: &[],
        implemented: false,
    },
    Module {
        native: "WreckageRecoverySystem",
        claims: &[],
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
        .filter(|module| !module.implemented)
        .flat_map(|module| {
            module
                .claims
                .iter()
                .filter(|field| field.carried(side))
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
/// `scripts/fight-coverage.py` counts.
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
            Field::Contraptions,
            Field::AirdropShields,
            Field::Terrains,
            Field::UnitLevel,
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

    /// The build's own module list, which `scripts/fight-structure.py` reads
    /// out of the decompilation index. One of ours is not the build's, and
    /// [`MODULES`] says why.
    #[test]
    fn the_modules_are_the_builds_thirty_five_and_one_of_our_own() {
        assert_eq!(MODULES.len(), 36);
        assert_eq!(MODULES[0].native, "Loadout");
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

    /// An empty side is inside the closure, and a side that carries something
    /// unimplemented is refused for every field at once rather than the first.
    #[test]
    fn a_side_is_refused_for_everything_it_carries() {
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
             units: [{name: marksman, index: 0, position: {x: 0, y: -50}, level: 3}]\nred:\n  \
             units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n",
        );
        let missing = unsupported(&loaded.blue);
        assert_eq!(
            missing
                .iter()
                .map(|(field, module)| (field.name(), *module))
                .collect::<Vec<_>>(),
            [
                ("officers", "Loadout"),
                ("constructions", "FightConstructionSystem"),
                ("units above level one", "Loadout"),
            ]
        );
    }
}
