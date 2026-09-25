use super::{Position, Terrain, TerrainType};
use crate::catalog::terrain_type_from_skill;
use std::collections::BTreeMap;

/// Shield Airdrop, whose range item is one shield still standing on the board.
pub(crate) const SHIELD_AIRDROP_SKILL: i32 = 800_001;

/// The retained commander-skill objects one GRBR round snapshot holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrbrRoundRetained {
    pub blue: GrbrSideRetained,
    pub red: GrbrSideRetained,
}

/// One side's share of them, in that side's own frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrbrSideRetained {
    pub terrains: Vec<Terrain>,
    pub airdrop_shields: Vec<Position>,
}

/// Read the retained Sticky Oil Bomb terrains and Shield Airdrops from the
/// BinaryFormatter-embedded `BattleRecord` XML in a GRBR.
///
/// Both live in the same place, a panel skill's `rangeItems`, and the snapshot
/// is taken at the round's start, so an entry is an object that outlived the
/// round that made it. What the two kinds do with `round` differs: an oil
/// terrain counts its remaining lifetime down, while a shield is not
/// time-limited and its entry simply disappears once the object is gone.
///
/// This is deliberately independent from the Adapter's live `RangeItemSystem`
/// enumeration. A future `mechcore grbr layout` command can compose this narrow
/// parser with the other GRBR layout fields.
///
/// # Errors
///
/// Returns an error when the GRBR carries no embedded `BattleRecord` XML, when
/// that XML does not describe exactly two players, when the requested round is
/// missing, or when a retained entry is malformed or belongs to a skill this
/// reader has not been measured against.
pub fn retained_from_grbr_round(grbr: &[u8], round: u32) -> Result<GrbrRoundRetained, String> {
    let xml = embedded_battle_record_xml(grbr)?;
    let player_records = xml_element(xml, "playerRecords")?;
    let players = xml_elements(player_records, "PlayerRecord")?;
    if players.len() != 2 {
        return Err(format!(
            "GRBR retained-object extraction requires exactly two PlayerRecord entries, got {}",
            players.len()
        ));
    }
    let mut sides: [GrbrSideRetained; 2] = Default::default();
    for (team, player) in players.into_iter().enumerate() {
        let records = xml_element(player, "playerRoundRecords")?;
        let selected = xml_elements(records, "PlayerRoundRecord")?
            .into_iter()
            .find(|record| xml_u32(record, "round") == Ok(round))
            .ok_or_else(|| format!("GRBR has no player round snapshot {round} for team {team}"))?;
        let player_data = xml_element(selected, "playerData")?;
        let Some(skills) = xml_optional_element(player_data, "commanderSkills")? else {
            continue;
        };
        for skill in xml_elements(skills, "CommanderSkillData")? {
            let id = xml_i32(skill, "id")?;
            let Some(range_items) = xml_optional_element(skill, "rangeItems")? else {
                continue;
            };
            for item in xml_elements(range_items, "CommanderSkillRangeItemData")? {
                if id == SHIELD_AIRDROP_SKILL {
                    sides[team]
                        .airdrop_shields
                        .push(airdrop_shield_center(item, team)?);
                    continue;
                }
                // Any skill that leaves a battlefield area behind leaves one of
                // these, and which substance it is the catalogue says. Only one
                // of them lasts long enough to be snapshotted under standard
                // rules, and the reader does not need to know which.
                let Some(terrain_type) = terrain_type_from_skill(id) else {
                    return Err(format!(
                        "GRBR round {round} contains unsupported retained commander-skill object {id}"
                    ));
                };
                // An area counts its remaining lifetime down and is gone at
                // zero, unlike a shield, which carries none.
                if xml_i32(item, "round")? <= 0 {
                    continue;
                }
                if let Some(terrain) = range_item_terrain(item, terrain_type, team)? {
                    sides[team].terrains.push(terrain);
                }
            }
        }
    }
    let [blue, red] = sides;
    Ok(GrbrRoundRetained { blue, red })
}

/// One retained battlefield area, or nothing when no point of it is still
/// active.
fn range_item_terrain(
    item: &str,
    terrain_type: TerrainType,
    team: usize,
) -> Result<Option<Terrain>, String> {
    let positions = item_positions(item)?;
    if positions.len() != 2 {
        return Err(format!(
            "retained-terrain GRBR snapshot requires two line endpoints, got {}",
            positions.len()
        ));
    }
    let active = decode_grbr_byte_mask(xml_i32(item, "activeState")?)?;
    if active.len() != 7 {
        return Err(format!(
            "retained-terrain GRBR activeState has {} points, expected seven",
            active.len()
        ));
    }
    let point_count = active.len();
    let grids = decode_grbr_grid_groups(&item_grid_values(item)?)?;
    let active_count = active.iter().filter(|&&value| value).count();
    if active_count == 0 {
        return Ok(None);
    }
    if grids.len() != active_count {
        return Err(format!(
            "retained-terrain GRBR snapshot has {active_count} active points but {} grids",
            grids.len()
        ));
    }
    let mut grid_index = 0;
    let mut grid_rows = BTreeMap::new();
    for (point_index, is_active) in active.into_iter().enumerate() {
        if !is_active {
            continue;
        }
        let mut rows = grids[grid_index].clone();
        grid_index += 1;
        if team != 0 {
            rows = rotate_oil_grid_rows(&rows);
        }
        grid_rows.insert(
            u32::try_from(point_index)
                .map_err(|_| "retained-terrain point index exceeds u32".to_owned())?,
            rows,
        );
    }
    let control_points = positions
        .into_iter()
        .map(|point| side_local(point, team))
        .collect::<Result<Vec<_>, String>>()?;
    if grid_rows.len() == point_count && grid_rows.values().all(Vec::is_empty) {
        grid_rows.clear();
    }
    Ok(Some(Terrain {
        terrain_type,
        control_points,
        grid_rows,
    }))
}

/// The centre of one retained Shield Airdrop.
///
/// A shield carries no lifetime and no grid, so the three fields an oil terrain
/// uses to say how much of itself is left are checked to be their empty forms
/// rather than read. Presence in the snapshot is the whole statement: the object
/// stands, at full energy, since a retained airdrop resets between rounds.
fn airdrop_shield_center(item: &str, team: usize) -> Result<Position, String> {
    let round = xml_i32(item, "round")?;
    if round != 0 {
        return Err(format!(
            "shield-airdrop GRBR snapshot carries lifetime {round}, and the object has none"
        ));
    }
    let positions = item_positions(item)?;
    let [center] = <[Position; 1]>::try_from(positions).map_err(|positions| {
        format!(
            "shield-airdrop GRBR snapshot requires one centre, got {}",
            positions.len()
        )
    })?;
    let active = decode_grbr_byte_mask(xml_i32(item, "activeState")?)?;
    if active != [true] {
        return Err(format!(
            "shield-airdrop GRBR activeState decodes to {active:?}, expected one active point"
        ));
    }
    let grids = decode_grbr_grid_groups(&item_grid_values(item)?)?;
    if !matches!(grids.as_slice(), [grid] if grid.is_empty()) {
        return Err(format!(
            "shield-airdrop GRBR snapshot carries {} grids, and the object has none",
            grids.len()
        ));
    }
    side_local(center, team)
}

fn item_positions(item: &str) -> Result<Vec<Position>, String> {
    xml_elements(xml_element(item, "positions")?, "Vector2Int")?
        .into_iter()
        .map(|position| {
            Ok(Position {
                x: xml_i32(position, "x")?,
                y: xml_i32(position, "y")?,
            })
        })
        .collect()
}

fn item_grid_values(item: &str) -> Result<Vec<i32>, String> {
    xml_elements(xml_element(item, "gridInfo")?, "int")?
        .into_iter()
        .map(|value| parse_i32(value.trim(), "gridInfo int"))
        .collect()
}

/// Red's recorded coordinates are the blue frame turned half a turn.
fn side_local(position: Position, team: usize) -> Result<Position, String> {
    if team == 0 {
        return Ok(position);
    }
    let turn = |value: i32| {
        value
            .checked_neg()
            .ok_or_else(|| "red GRBR coordinate cannot be converted to side-local space".to_owned())
    };
    Ok(Position {
        x: turn(position.x)?,
        y: turn(position.y)?,
    })
}

fn embedded_battle_record_xml(grbr: &[u8]) -> Result<&str, String> {
    const START: &[u8] = b"<?xml version=\"1.0\" encoding=\"utf-8\"?>";
    const END: &[u8] = b"</BattleRecord>";
    let start =
        find_bytes(grbr, START).ok_or_else(|| "GRBR has no embedded XML payload".to_owned())?;
    if find_bytes(&grbr[start + START.len()..], START).is_some() {
        return Err("GRBR contains more than one embedded XML payload".into());
    }
    let end_offset = find_bytes(&grbr[start..], END)
        .ok_or_else(|| "GRBR embedded XML has no BattleRecord terminator".to_owned())?;
    let end = start + end_offset + END.len();
    std::str::from_utf8(&grbr[start..end])
        .map_err(|error| format!("GRBR embedded XML is not UTF-8: {error}"))
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn xml_element<'a>(xml: &'a str, tag: &str) -> Result<&'a str, String> {
    xml_optional_element(xml, tag)?.ok_or_else(|| format!("XML element <{tag}> is absent"))
}

fn xml_optional_element<'a>(xml: &'a str, tag: &str) -> Result<Option<&'a str>, String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let Some(start) = xml.find(&open) else {
        return Ok(None);
    };
    let content_start = start + open.len();
    let end = xml[content_start..]
        .find(&close)
        .map(|offset| content_start + offset)
        .ok_or_else(|| format!("XML element <{tag}> is unterminated"))?;
    Ok(Some(&xml[content_start..end]))
}

fn xml_elements<'a>(xml: &'a str, tag: &str) -> Result<Vec<&'a str>, String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut remaining = xml;
    let mut values = Vec::new();
    while let Some(start) = remaining.find(&open) {
        let content_start = start + open.len();
        let end = remaining[content_start..]
            .find(&close)
            .map(|offset| content_start + offset)
            .ok_or_else(|| format!("XML element <{tag}> is unterminated"))?;
        values.push(&remaining[content_start..end]);
        remaining = &remaining[end + close.len()..];
    }
    Ok(values)
}

fn xml_i32(xml: &str, tag: &str) -> Result<i32, String> {
    parse_i32(xml_element(xml, tag)?.trim(), tag)
}

fn xml_u32(xml: &str, tag: &str) -> Result<u32, String> {
    xml_element(xml, tag)?
        .trim()
        .parse::<u32>()
        .map_err(|error| format!("XML <{tag}> is not u32: {error}"))
}

fn parse_i32(value: &str, context: &str) -> Result<i32, String> {
    value
        .parse::<i32>()
        .map_err(|error| format!("{context} is not i32: {error}"))
}

pub(crate) fn decode_grbr_byte_mask(value: i32) -> Result<Vec<bool>, String> {
    if value == 0 {
        return Err("serialized ByteMask value is zero".into());
    }
    let raw = value.cast_unsigned();
    let length = if value < 0 {
        31
    } else {
        usize::try_from(u32::BITS - 1 - raw.leading_zeros())
            .map_err(|_| "ByteMask length exceeds usize".to_owned())?
    };
    Ok((0..length)
        .rev()
        .map(|bit| raw & (1_u32 << bit) != 0)
        .collect())
}

pub(crate) fn decode_grbr_grid_groups(values: &[i32]) -> Result<Vec<Vec<u32>>, String> {
    let mut offset = 0;
    let mut grids = Vec::new();
    while offset < values.len() {
        let count = usize::try_from(values[offset])
            .map_err(|_| "GRBR gridInfo contains a negative mask count".to_owned())?;
        offset += 1;
        let end = offset
            .checked_add(count)
            .filter(|&end| end <= values.len())
            .ok_or_else(|| "GRBR gridInfo mask group exceeds its payload".to_owned())?;
        if count == 0 {
            grids.push(Vec::new());
            continue;
        }
        let bits = values[offset..end]
            .iter()
            .copied()
            .map(decode_grbr_byte_mask)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        offset = end;
        if bits.len() != 12 * 12 {
            return Err(format!(
                "GRBR oil grid decodes to {} bits, expected 144",
                bits.len()
            ));
        }
        let mut rows = vec![0_u32; 12];
        for (bit_index, active) in bits.into_iter().enumerate() {
            if active {
                rows[bit_index % 12] |= 1_u32 << (bit_index / 12);
            }
        }
        grids.push(rows);
    }
    Ok(grids)
}

pub(crate) fn rotate_oil_grid_rows(rows: &[u32]) -> Vec<u32> {
    rows.iter()
        .rev()
        .map(|row| (row & 0x0fff).reverse_bits() >> 20)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One well-formed Shield Airdrop range item, as the tracked set writes it.
    fn shield_item(x: i32, y: i32) -> String {
        format!(
            "<positions><Vector2Int><x>{x}</x><y>{y}</y></Vector2Int></positions>\
             <activeState>3</activeState><gridInfo><int>0</int></gridInfo><round>0</round>"
        )
    }

    #[test]
    fn a_red_shield_center_turns_half_a_turn() {
        let item = shield_item(235, 74);
        assert_eq!(
            airdrop_shield_center(&item, 0).unwrap(),
            Position { x: 235, y: 74 }
        );
        assert_eq!(
            airdrop_shield_center(&item, 1).unwrap(),
            Position { x: -235, y: -74 }
        );
    }

    /// The three fields that say how much of an oil terrain is left are the
    /// three a shield has no use for, so each is required to be its empty form
    /// rather than read past.
    #[test]
    fn a_shield_carrying_an_oil_terrains_fields_is_refused() {
        let lifetime = shield_item(1, 2).replace("<round>0</round>", "<round>1</round>");
        assert!(
            airdrop_shield_center(&lifetime, 0)
                .unwrap_err()
                .contains("carries lifetime 1")
        );
        let pair = shield_item(1, 2).replace(
            "</positions>",
            "<Vector2Int><x>3</x><y>4</y></Vector2Int></positions>",
        );
        assert!(
            airdrop_shield_center(&pair, 0)
                .unwrap_err()
                .contains("requires one centre, got 2")
        );
        let points = shield_item(1, 2).replace(
            "<activeState>3</activeState>",
            "<activeState>7</activeState>",
        );
        assert!(
            airdrop_shield_center(&points, 0)
                .unwrap_err()
                .contains("expected one active point")
        );
        let grid = shield_item(1, 2).replace("<int>0</int>", "<int>1</int><int>255</int>");
        assert!(airdrop_shield_center(&grid, 0).is_err());
    }

    #[test]
    fn rejects_missing_grbr_xml() {
        assert_eq!(
            retained_from_grbr_round(b"not a replay", 2).unwrap_err(),
            "GRBR has no embedded XML payload"
        );
    }
}
