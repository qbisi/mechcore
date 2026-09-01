use std::{fs, path::PathBuf};

pub(crate) fn run(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    if arguments.next().as_deref() != Some("verify") {
        return Err("expected `verify <layout.yaml>`".into());
    }
    let path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "expected layout.yaml after `verify`".to_owned())?;
    if let Some(extra) = arguments.next() {
        return Err(format!("unexpected argument {extra:?}"));
    }
    let bytes =
        fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let layout: mechcore_layout::Layout =
        serde_yaml::from_slice(&bytes).map_err(|error| format!("invalid layout YAML: {error}"))?;
    let plan = mechcore_layout::compile_layout(layout)?;
    let report = serde_json::json!({
        "valid": true,
        "layout": path,
        "seed": plan.seed,
        "round": plan.round,
        "formation_count": plan.formation_count(),
        "contraption_count": plan.contraption_count(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("cannot serialize verification report: {error}"))?
    );
    Ok(())
}
