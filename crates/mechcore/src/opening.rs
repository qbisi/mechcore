//! Seed-only standard 1v1 opening prediction.

pub(crate) fn run(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let seed: i32 = arguments
        .next()
        .ok_or("usage: mechcore opening <seed> <map_id>")?
        .parse()
        .map_err(|error| format!("invalid seed: {error}"))?;
    let map_id: i32 = arguments
        .next()
        .ok_or("usage: mechcore opening <seed> <map_id>")?
        .parse()
        .map_err(|error| format!("invalid map ID: {error}"))?;
    if arguments.next().is_some() {
        return Err("usage: mechcore opening <seed> <map_id>".into());
    }
    let economy = mechcore_document::economy::Economy::embedded()?;
    let prediction = mechcore_document::opening::predict(&economy, seed, map_id)?;
    println!(
        "{}",
        serde_json::to_string(&prediction).map_err(|error| error.to_string())?
    );
    Ok(())
}
