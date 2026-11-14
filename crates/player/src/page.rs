//! The page a timeline is played in: one HTML file carrying its style, its
//! script and the timeline itself, so it opens offline in any browser.

use crate::Timeline;

const SHELL: &str = include_str!("../web/index.html");
const STYLE: &str = include_str!("../web/player.css");
const HUD_FONT: &str = include_str!("../web/hud-font.css");
const HUD: &str = include_str!("../web/hud.js");
const ROSTER: &str = include_str!("../web/roster.js");
const ARSENAL: &str = include_str!("../web/arsenal.js");
const TOWER_ASSETS: &str = include_str!("../web/tower-cards.json");
const SPRITES: &str = include_str!("../web/sprites.js");
const SNAPSHOT: &str = include_str!("../web/snapshot.js");
const TERRAIN: &str = include_str!("../web/terrain.js");
const SKILL_ICONS: &str = include_str!("../web/skill-icons.js");
const DEPLOYMENT: &str = include_str!("../web/deployment.js");
const LEAD_IN: &str = include_str!("../web/lead-in.js");
const SPAWN: &str = include_str!("../web/spawn.js");
const SETTLEMENT: &str = include_str!("../web/settlement.js");
const PLAYER: &str = include_str!("../web/player.js");

/// One playable round listed in a live player page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct LiveRoundMeta {
    pub index: usize,
    pub round: i32,
}

/// Writes the page that plays `timeline`, titled `title`.
///
/// # Errors
///
/// Returns an error when the timeline cannot be serialised.
pub fn page(timeline: &Timeline, title: &str) -> Result<String, serde_json::Error> {
    let assets = crate::roster::assets(timeline.rosters.iter().flatten())?;
    let arsenal = crate::arsenal::assets(timeline.arsenals.iter())?;
    render(timeline, title, &assets, &arsenal)
}

/// Writes one offline page containing every round's timeline.
///
/// # Errors
///
/// Returns an error when the timelines cannot be serialised.
pub fn match_page(rounds: &[Timeline], title: &str) -> Result<String, serde_json::Error> {
    let assets = crate::roster::assets(rounds.iter().flat_map(|r| r.rosters.iter().flatten()))?;
    let arsenal = crate::arsenal::assets(rounds.iter().flat_map(|r| r.arsenals.iter()))?;
    render(rounds, title, &assets, &arsenal)
}

/// Writes a live player page embedding the initial round, round metadata, and
/// pre-collected panel assets across all rounds.
///
/// # Errors
///
/// Returns an error when the timeline, metadata, or asset catalogs cannot be
/// serialised.
pub fn live_page(
    first: &Timeline,
    rounds: &[LiveRoundMeta],
    rosters: &[crate::RosterEntry],
    arsenals: &[crate::Arsenal],
    title: &str,
) -> Result<String, serde_json::Error> {
    let first_json = serde_json::to_string(first)?;
    let assets = crate::roster::assets(first.rosters.iter().flatten().chain(rosters.iter()))?;
    let arsenal = crate::arsenal::assets(first.arsenals.iter().chain(arsenals.iter()))?;
    live_render(&first_json, rounds, title, &assets, &arsenal)
}

/// Writes a live player page from an already-serialised initial round
/// timeline and full asset slices across all rounds.
///
/// # Errors
///
/// Returns an error when metadata or asset catalogs cannot be serialised.
pub fn live_page_json(
    first_json: &str,
    rounds: &[LiveRoundMeta],
    rosters: &[crate::RosterEntry],
    arsenals: &[crate::Arsenal],
    title: &str,
) -> Result<String, serde_json::Error> {
    let assets = crate::roster::assets(rosters.iter())?;
    let arsenal = crate::arsenal::assets(arsenals.iter())?;
    live_render(first_json, rounds, title, &assets, &arsenal)
}

fn live_render(
    first_json: &str,
    rounds: &[LiveRoundMeta],
    title: &str,
    assets: &crate::roster::Catalog,
    arsenal: &crate::arsenal::Catalog,
) -> Result<String, serde_json::Error> {
    let rounds_json = serde_json::to_string(rounds)?;
    let data = format!("{{\"live\":true,\"rounds\":{rounds_json},\"initial\":{first_json}}}");
    render_raw(&data, title, assets, arsenal)
}

fn render(
    data: &(impl serde::Serialize + ?Sized),
    title: &str,
    assets: &crate::roster::Catalog,
    arsenal: &crate::arsenal::Catalog,
) -> Result<String, serde_json::Error> {
    render_raw(&serde_json::to_string(data)?, title, assets, arsenal)
}

fn render_raw(
    data_json: &str,
    title: &str,
    assets: &crate::roster::Catalog,
    arsenal: &crate::arsenal::Catalog,
) -> Result<String, serde_json::Error> {
    // Inside a script element only `</` can end the element early, and JSON
    // may write it as `<\/` instead.
    let data = data_json.replace("</", "<\\/");
    let assets = serde_json::to_string(assets)?.replace("</", "<\\/");
    let arsenal = serde_json::to_string(arsenal)?.replace("</", "<\\/");
    let towers: serde_json::Value = serde_json::from_str(TOWER_ASSETS)?;
    let towers = serde_json::to_string(&towers)?.replace("</", "<\\/");
    Ok(SHELL
        .replace("{{title}}", &escape(title))
        .replace("/*{{style}}*/", STYLE)
        .replace("/*{{hud-font}}*/", HUD_FONT)
        .replace("/*{{hud}}*/", HUD)
        .replace("/*{{roster}}*/", ROSTER)
        .replace("/*{{arsenal}}*/", ARSENAL)
        .replace("/*{{sprites}}*/", SPRITES)
        .replace("/*{{snapshot}}*/", SNAPSHOT)
        .replace("/*{{terrain}}*/", TERRAIN)
        .replace("/*{{skill-icons}}*/", SKILL_ICONS)
        .replace("/*{{deployment}}*/", DEPLOYMENT)
        .replace("/*{{lead-in}}*/", LEAD_IN)
        .replace("/*{{spawn}}*/", SPAWN)
        .replace("/*{{settlement}}*/", SETTLEMENT)
        .replace("/*{{player}}*/", PLAYER)
        .replace("{{timeline}}", &data)
        .replace("{{roster-assets}}", &assets)
        .replace("{{tower-assets}}", &towers)
        .replace("{{arsenal-assets}}", &arsenal))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
