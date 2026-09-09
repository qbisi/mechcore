use super::{Position, Terrain, TerrainType};
use std::collections::BTreeMap;

/// Terrain declarations reconstructed directly from one build-2259 GRBR round snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrbrRoundTerrains {
    pub blue: Vec<Terrain>,
    pub red: Vec<Terrain>,
}

/// Read retained Sticky Oil Bomb terrains from the BinaryFormatter-embedded
/// `BattleRecord` XML in a build-2259 GRBR.
///
/// This is deliberately independent from the Adapter's live `RangeItemSystem`
/// enumeration. A future `mechcore grbr layout` command can compose this narrow
/// parser with the other GRBR layout fields.
///
/// # Errors
///
/// Returns an error when the GRBR carries no embedded `BattleRecord` XML, when
/// that XML does not describe exactly two players, or when the requested round
/// or its terrain entries are missing or malformed.
#[allow(clippy::too_many_lines)]
pub fn terrains_from_grbr_round(grbr: &[u8], round: u32) -> Result<GrbrRoundTerrains, String> {
    let xml = embedded_battle_record_xml(grbr)?;
    let player_records = xml_element(xml, "playerRecords")?;
    let players = xml_elements(player_records, "PlayerRecord")?;
    if players.len() != 2 {
        return Err(format!(
            "GRBR terrain extraction requires exactly two PlayerRecord entries, got {}",
            players.len()
        ));
    }
    let mut sides: [Vec<Terrain>; 2] = [Vec::new(), Vec::new()];
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
                if xml_i32(item, "round")? <= 0 {
                    continue;
                }
                if id != 400_002 {
                    return Err(format!(
                        "GRBR round {round} contains unsupported retained commander-skill terrain {id}"
                    ));
                }
                let positions = xml_elements(xml_element(item, "positions")?, "Vector2Int")?
                    .into_iter()
                    .map(|position| {
                        Ok(Position {
                            x: xml_i32(position, "x")?,
                            y: xml_i32(position, "y")?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                if positions.len() != 2 {
                    return Err(format!(
                        "sticky-oil GRBR snapshot requires two line endpoints, got {}",
                        positions.len()
                    ));
                }
                let active = decode_grbr_byte_mask(xml_i32(item, "activeState")?)?;
                if active.len() != 7 {
                    return Err(format!(
                        "sticky-oil GRBR activeState has {} points, expected seven",
                        active.len()
                    ));
                }
                let point_count = active.len();
                let grid_values = xml_elements(xml_element(item, "gridInfo")?, "int")?
                    .into_iter()
                    .map(|value| parse_i32(value.trim(), "gridInfo int"))
                    .collect::<Result<Vec<_>, _>>()?;
                let grids = decode_grbr_grid_groups(&grid_values)?;
                let active_count = active.iter().filter(|&&value| value).count();
                if active_count == 0 {
                    continue;
                }
                if grids.len() != active_count {
                    return Err(format!(
                        "sticky-oil GRBR snapshot has {active_count} active points but {} grids",
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
                            .map_err(|_| "sticky-oil point index exceeds u32".to_owned())?,
                        rows,
                    );
                }
                let control_points = positions
                    .into_iter()
                    .map(|mut point| {
                        if team != 0 {
                            point.x = point.x.checked_neg().ok_or_else(|| {
                                "red GRBR terrain x cannot be converted to side-local space"
                                    .to_owned()
                            })?;
                            point.y = point.y.checked_neg().ok_or_else(|| {
                                "red GRBR terrain y cannot be converted to side-local space"
                                    .to_owned()
                            })?;
                        }
                        Ok(point)
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                if grid_rows.len() == point_count && grid_rows.values().all(Vec::is_empty) {
                    grid_rows.clear();
                }
                sides[team].push(Terrain {
                    terrain_type: TerrainType::Oil,
                    control_points,
                    grid_rows,
                });
            }
        }
    }
    let [blue, red] = sides;
    Ok(GrbrRoundTerrains { blue, red })
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

fn decode_grbr_byte_mask(value: i32) -> Result<Vec<bool>, String> {
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

fn decode_grbr_grid_groups(values: &[i32]) -> Result<Vec<Vec<u32>>, String> {
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

fn rotate_oil_grid_rows(rows: &[u32]) -> Vec<u32> {
    rows.iter()
        .rev()
        .map(|row| (row & 0x0fff).reverse_bits() >> 20)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_latest_tracked_grbr_round_two_oil() {
        let grbr =
            include_bytes!("../../../tests/grbr/2259_26-09-03__13-54-16-770_[crower]VS[电脑].grbr");
        let terrains = terrains_from_grbr_round(grbr, 2).unwrap();

        assert!(terrains.blue.is_empty());
        assert_eq!(terrains.red.len(), 1);
        assert_eq!(
            terrains.red[0].control_points,
            [Position { x: -24, y: 11 }, Position { x: 80, y: 1 }]
        );
        assert_eq!(terrains.red[0].terrain_type, TerrainType::Oil);
        assert_eq!(
            terrains.red[0]
                .grid_rows
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [0, 1, 5, 6]
        );
        assert_eq!(
            terrains.red[0].grid_rows[&0],
            [
                240, 1020, 2046, 2046, 4095, 4095, 1023, 511, 254, 126, 60, 48
            ]
        );

        let layout = super::super::parse_yaml(include_bytes!(
            "../../../tests/layouts/crower-computer-replay-round-2-terrain.yaml"
        ))
        .unwrap();
        assert_eq!(terrains.blue, layout.sides.blue.terrains);
        assert_eq!(terrains.red, layout.sides.red.terrains);
    }

    #[test]
    fn rejects_missing_grbr_xml() {
        assert_eq!(
            terrains_from_grbr_round(b"not a replay", 2).unwrap_err(),
            "GRBR has no embedded XML payload"
        );
    }
}
