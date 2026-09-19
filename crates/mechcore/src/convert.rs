use std::{fs, path::PathBuf};

/// How many unequal leaves to name before counting the rest.
const FAILURES_SHOWN: usize = 5;

/// Converts one locally recorded replay into a battle document.
///
/// A replay is the only source this command reads and a battle document the
/// only thing it writes, so the direction is the command rather than an
/// argument of it. The replay is read, never written, and the destination is
/// refused when it already exists: a conversion that silently replaced a file
/// would make the document's provenance unrecoverable.
pub(crate) fn run(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let source = required_path(&mut arguments, "expected <replay.grbr> <battle.yaml>")?;
    let destination = required_path(&mut arguments, "expected battle.yaml after replay.grbr")?;
    let force = match arguments.next().as_deref() {
        None => false,
        Some("--force") => true,
        Some(extra) => return Err(format!("unexpected argument {extra:?}")),
    };
    reject_extra(&mut arguments)?;
    if !force && destination.exists() {
        return Err(format!(
            "{} already exists; pass --force to replace it",
            destination.display()
        ));
    }

    let grbr =
        fs::read(&source).map_err(|error| format!("cannot read {}: {error}", source.display()))?;
    let battle = mechcore_document::convert::battle_from_grbr(&grbr)?;
    let economy = mechcore_document::economy::Economy::embedded()?;
    let yaml = mechcore_document::battle::canonical_yaml(&battle)?;
    // The document is measured as it was written, the way `verify` reads it.
    let stated = mechcore_document::opening::stated(yaml.as_bytes())?
        .ok_or("the converted document does not read back as a battle")?;
    let deal = mechcore_document::opening::verify(&economy, &stated).and_then(|opening| {
        mechcore_document::reinforcement::verify(&economy, &stated, &opening)
    });
    let coverage = mechcore_document::coverage::measure(
        &economy,
        &stated,
        deal.as_ref().map_err(String::as_str),
    );
    fs::write(&destination, &yaml)
        .map_err(|error| format!("cannot write {}: {error}", destination.display()))?;

    let actions: usize = battle
        .turns
        .iter()
        .map(|turn| turn.actions.blue.len() + turn.actions.red.len())
        .sum();
    println!(
        "{} map {} seed {} rounds {} actions {}",
        destination.display(),
        battle.map_id,
        battle.seed,
        battle.turns.len(),
        actions
    );
    let total = coverage.total;
    println!(
        "  transitions: {} leaves equal, {} unequal, {} unimplemented, {} the fight's",
        total.equal, total.unequal, total.unimplemented, total.fight
    );
    if let Err(error) = &deal {
        println!("    reinforcement deal: {error}");
    }
    for difference in coverage.unequal.iter().take(FAILURES_SHOWN) {
        println!(
            "    round {} {} {}: predicted {} and holds {}",
            difference.round,
            difference.side,
            difference.path,
            difference.predicted.as_deref().unwrap_or("nothing"),
            difference.recorded.as_deref().unwrap_or("nothing")
        );
    }
    if coverage.unequal.len() > FAILURES_SHOWN {
        println!("    and {} more", coverage.unequal.len() - FAILURES_SHOWN);
    }
    Ok(())
}

fn required_path(
    arguments: &mut impl Iterator<Item = String>,
    message: &str,
) -> Result<PathBuf, String> {
    arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| message.into())
}

fn reject_extra(arguments: &mut impl Iterator<Item = String>) -> Result<(), String> {
    if let Some(extra) = arguments.next() {
        Err(format!("unexpected argument {extra:?}"))
    } else {
        Ok(())
    }
}
