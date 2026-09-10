use std::{fs, path::PathBuf};

/// How many ledger failures to name before counting the rest.
const FAILURES_SHOWN: usize = 5;

pub(crate) fn run(mut arguments: impl Iterator<Item = String>) -> Result<bool, String> {
    match arguments.next().as_deref() {
        Some("battle") => battle(arguments).map(|()| true),
        _ => Err("expected `battle <replay.grbr> <battle.yaml>`".into()),
    }
}

/// Converts one locally recorded replay into a battle document.
///
/// The replay is read, never written, and the destination is refused when it
/// already exists: a conversion that silently replaced a file would make the
/// document's provenance unrecoverable.
fn battle(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let source = required_path(&mut arguments, "expected replay.grbr after `battle`")?;
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
    let ledger = mechcore_document::ledger::check(&battle, &economy)?;
    let yaml = mechcore_document::battle::canonical_yaml(&battle)?;
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
    println!(
        "  supply ledger: {} of {} round transitions close, \
         {} paid by the fight, {} unpriced",
        ledger.closed,
        ledger.checked(),
        ledger.fight_pays,
        ledger.unpriced
    );
    for failure in ledger.failures.iter().take(FAILURES_SHOWN) {
        println!(
            "    round {} {} holds {} where the ledger expects {}",
            failure.round, failure.side, failure.actual, failure.expected
        );
    }
    if ledger.failures.len() > FAILURES_SHOWN {
        println!("    and {} more", ledger.failures.len() - FAILURES_SHOWN);
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
