//! The board a side deploys on: its three regions and the map's bound, as the
//! layout every standard 1v1 map shares lays them out.
//!
//! `config/maps.yaml` holds each map data's `MapLayout`: one `PlayerTerritory`
//! per seat, a main region and two flanks. The territory here is blue's, in
//! world metres, which is blue's own frame; red's is the same turned half a
//! turn, which reading the layout checks. The map's bound is `Map.Bound`, the
//! least rectangle holding every territory's regions (`Map.RefreshBound`), and
//! what a battle skill's map rule measures against
//! (`PlayerController.GetRegionForCommanderSkill`). `docs/rules/map.md` states
//! both.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

/// A rectangle in metres: its least corner and its greatest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}

impl Rect {
    /// The same rectangle in a frame turned half a turn.
    const fn turned(self) -> Self {
        Self {
            min_x: -self.max_x,
            min_y: -self.max_y,
            max_x: -self.min_x,
            max_y: -self.min_y,
        }
    }

    const fn union(self, other: Self) -> Self {
        Self {
            min_x: if self.min_x < other.min_x {
                self.min_x
            } else {
                other.min_x
            },
            min_y: if self.min_y < other.min_y {
                self.min_y
            } else {
                other.min_y
            },
            max_x: if self.max_x > other.max_x {
                self.max_x
            } else {
                other.max_x
            },
            max_y: if self.max_y > other.max_y {
                self.max_y
            } else {
                other.max_y
            },
        }
    }
}

/// A side's regions in its own frame, and the map's bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Territory {
    /// The main deployment region.
    pub main: Rect,
    /// The flank whose `x` is negative.
    pub left_flank: Rect,
    /// The flank whose `x` is positive.
    pub right_flank: Rect,
    /// `Map.Bound`.
    pub bound: Rect,
}

static TERRITORY: LazyLock<Territory> = LazyLock::new(|| {
    read(include_str!("../../../config/maps.yaml"))
        .unwrap_or_else(|error| panic!("config/maps.yaml lays out no territory: {error}"))
});

/// The territory of every standard 1v1 map.
#[must_use]
pub fn territory() -> &'static Territory {
    &TERRITORY
}

#[derive(Deserialize)]
struct MapsFile {
    maps: BTreeMap<i32, String>,
    layouts: BTreeMap<String, String>,
    map_layouts: BTreeMap<String, Vec<PlayerTerritory>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayerTerritory {
    main_region: i32,
    regions: Vec<MapRegion>,
}

/// A region's id and rectangle; its facing and opening round are the
/// layout's too, and nothing here reads them.
#[derive(Deserialize)]
struct MapRegion {
    id: i32,
    x_min: i64,
    y_min: i64,
    width: i64,
    height: i64,
}

impl MapRegion {
    const fn rect(&self) -> Rect {
        Rect {
            min_x: self.x_min,
            min_y: self.y_min,
            max_x: self.x_min + self.width,
            max_y: self.y_min + self.height,
        }
    }
}

fn read(yaml: &str) -> Result<Territory, String> {
    let maps: MapsFile = serde_yaml::from_str(yaml).map_err(|error| error.to_string())?;
    let mut names = maps.maps.values().map(|data| {
        maps.layouts
            .get(data)
            .ok_or_else(|| format!("map data {data} names no layout"))
    });
    let name = names.next().ok_or("no map")??;
    if names.any(|other| other != Ok(name)) {
        return Err("the standard maps lay out different territories".to_owned());
    }
    let seats = maps
        .map_layouts
        .get(name)
        .ok_or_else(|| format!("layout {name} is not in the config"))?;
    let [blue, red] = seats.as_slice() else {
        return Err(format!(
            "layout {name} holds {} territories, not 2",
            seats.len()
        ));
    };
    let regions = |seat: &PlayerTerritory| -> Result<(Rect, Vec<Rect>), String> {
        let main = seat
            .regions
            .iter()
            .find(|region| region.id == seat.main_region)
            .ok_or_else(|| format!("layout {name} has no region {}", seat.main_region))?
            .rect();
        let mut flanks = seat
            .regions
            .iter()
            .filter(|region| region.id != seat.main_region)
            .map(MapRegion::rect)
            .collect::<Vec<_>>();
        flanks.sort_by_key(|rect| rect.min_x);
        Ok((main, flanks))
    };
    let (main, flanks) = regions(blue)?;
    let [left_flank, right_flank] = flanks[..] else {
        return Err(format!(
            "layout {name} gives a side {} flanks, not 2",
            flanks.len()
        ));
    };
    let (red_main, red_flanks) = regions(red)?;
    let mut turned = flanks.iter().map(|rect| rect.turned()).collect::<Vec<_>>();
    turned.sort_by_key(|rect| rect.min_x);
    if red_main != main.turned() || red_flanks != turned {
        return Err(format!(
            "layout {name} does not give red blue's territory turned half a turn"
        ));
    }
    let bound = [main, left_flank, right_flank, red_main]
        .into_iter()
        .chain(red_flanks)
        .reduce(Rect::union)
        .expect("a territory has regions");
    Ok(Territory {
        main,
        left_flank,
        right_flank,
        bound,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The standard maps' board: a 600 by 300 metre main region and two 60 by
    /// 300 metre flanks beyond the middle, inside a 720 by 620 metre map.
    #[test]
    fn the_standard_board() {
        let territory = territory();
        assert_eq!(
            territory.main,
            Rect {
                min_x: -300,
                min_y: -310,
                max_x: 300,
                max_y: -10
            }
        );
        assert_eq!(
            territory.left_flank,
            Rect {
                min_x: -360,
                min_y: 10,
                max_x: -300,
                max_y: 310
            }
        );
        assert_eq!(territory.right_flank.min_x, 300);
        assert_eq!(
            territory.bound,
            Rect {
                min_x: -360,
                min_y: -310,
                max_x: 360,
                max_y: 310
            }
        );
    }
}
