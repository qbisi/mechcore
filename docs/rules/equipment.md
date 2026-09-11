# Equipment index

[中文](equipment.zh.md)

This index is pinned to game build 2227. It contains the 18 ordinary
reinforcement equipment items and the Lesser Amplifying Core embedded in the
Amplify Specialist opening. Names and effects are taken from that build's
`EquipmentGroupData`; percentages below are additive native modifiers unless
the effect says otherwise.

An equipment ID is also the ID of the card that grants it, so taking card
`13030001` adds equipment `13030001` and nothing has to be looked up in
between. The machine-readable table, pinned to build 2259, is
[`config/reinforce_items.yaml`](../../config/reinforce_items.yaml), which states
each item's `kind` and what taking it costs;
[`docs/reinforce_items.md`](reinforce_items.md) explains the card system that
deals it. The effect text below has no machine-readable counterpart yet.

| Equipment ID | Name | Effect |
| --- | --- | --- |
| 1305003 | Photon Coating | For the first 30 seconds of combat, reduces damage taken by 30% and grants immunity to EMP, ignition, acid, and degeneration beam effects |
| 1306001 | Tank Production Line | Giant units only; produces 2 Sledgehammers every 13 seconds, up to 7 times |
| 1306002 | Mustang Production Line | Giant units only; produces 4 Mustangs every 11 seconds, up to 8 times |
| 1306003 | Steel Ball Production Line | Giant units only; produces 2 Steel Balls every 16 seconds, up to 6 times |
| 1307001 | Barrier | Ground giant units only; creates a defensive barrier with 60000 HP that protects nearby units |
| 1308001 | Anti-Interference Module | Grants immunity to EMP, Hacker control, and paralysis caused by a core building explosion |
| 1309001 | Absorption Module | Increases HP by 30% and converts 90% of damage dealt into the equipped unit's HP |
| 13010001 | Portable Shield | Grants an energy shield with HP equal to the unit's HP; the shield blocks at least one instance of damage |
| 13020001 | Nano Repair Kit | Restores 4.5% of maximum HP per second |
| 13030001 | Laser Sight | Increases range by 20 m; restricted to units classified as ranged by the runtime |
| 13030002 | Heavy Armor | Increases HP by 75% |
| 13030003 | Improved Fire Control System | Increases attack by 65% |
| 13030004 | Enhancement Module | Increases attack by 25% and HP by 25%; reduces each upgrade's supply cost by 100 |
| 13030005 | Haste Module | Increases movement speed by 5 and attack by 35% |
| 13030006 | Super Heavy Armor | Increases HP by 150% |
| 13030007 | Amplifying Core | Increases attack by 50% and HP by 50% |
| 13030009 | Lesser Amplifying Core | Increases attack by 22% and HP by 22% |
| 13030010 | Dominion Core | Increases attack by 50% and HP by 100%, and grants 50 supply at the start of each round; if the equipped unit is destroyed in combat, all allied units are destroyed |
| 13040001 | Deployment Module | Allows the equipped unit to move freely during every deployment phase |

The layout schema stores the numeric Equipment ID in a unit's `equipment`
field. This index does not introduce another schema. The executor does not
charge the reinforcement-card acquisition cost: it creates one Training Ground
inventory object through `MAD_AddEquipment`, then the native `CanUseEquipment`
check enforces equipment capability, the single empty slot, ownership, and
effect-target restrictions.

In build 2227, War Factory, Abyss, and Mountain cannot equip any item. All 19
indexed items have an unlimited configured round duration; only Dominion Core
changes recurring supply.
