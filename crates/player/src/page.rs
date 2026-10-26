//! The page a timeline is played in: one HTML file carrying its style, its
//! script and the timeline itself, so it opens offline in any browser.

use crate::Timeline;

const SHELL: &str = include_str!("../web/index.html");
const STYLE: &str = include_str!("../web/player.css");
const HUD_FONT: &str = include_str!("../web/hud-font.css");
const HUD: &str = include_str!("../web/hud.js");
const SPRITES: &str = include_str!("../web/sprites.js");
const SNAPSHOT: &str = include_str!("../web/snapshot.js");
const TERRAIN: &str = include_str!("../web/terrain.js");
const SKILL_ICONS: &str = include_str!("../web/skill-icons.js");
const DEPLOYMENT: &str = include_str!("../web/deployment.js");
const SPAWN: &str = include_str!("../web/spawn.js");
const SETTLEMENT: &str = include_str!("../web/settlement.js");
const PLAYER: &str = include_str!("../web/player.js");

/// Writes the page that plays `timeline`, titled `title`.
///
/// # Errors
///
/// Returns an error when the timeline cannot be serialised.
pub fn page(timeline: &Timeline, title: &str) -> Result<String, serde_json::Error> {
    render(timeline, title)
}

/// Writes one offline page containing every round's timeline.
///
/// # Errors
///
/// Returns an error when the timelines cannot be serialised.
pub fn match_page(rounds: &[Timeline], title: &str) -> Result<String, serde_json::Error> {
    render(rounds, title)
}

fn render(
    data: &(impl serde::Serialize + ?Sized),
    title: &str,
) -> Result<String, serde_json::Error> {
    // Inside a script element only `</` can end the element early, and JSON
    // may write it as `<\/` instead.
    let data = serde_json::to_string(data)?.replace("</", "<\\/");
    Ok(SHELL
        .replace("{{title}}", &escape(title))
        .replace("/*{{style}}*/", STYLE)
        .replace("/*{{hud-font}}*/", HUD_FONT)
        .replace("/*{{hud}}*/", HUD)
        .replace("/*{{sprites}}*/", SPRITES)
        .replace("/*{{snapshot}}*/", SNAPSHOT)
        .replace("/*{{terrain}}*/", TERRAIN)
        .replace("/*{{skill-icons}}*/", SKILL_ICONS)
        .replace("/*{{deployment}}*/", DEPLOYMENT)
        .replace("/*{{spawn}}*/", SPAWN)
        .replace("/*{{settlement}}*/", SETTLEMENT)
        .replace("/*{{player}}*/", PLAYER)
        .replace("{{timeline}}", &data))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
