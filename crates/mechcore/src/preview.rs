//! `preview`: initialized attributes and predicted initial target relationships.
use crate::{
    cli::{Args, Failure, Outcome, Verdict, emit},
    kind::Kind,
};

pub(crate) fn run(mut args: Args) -> Outcome {
    let format = args.format()?;
    let seed = args.parsed::<i32>("--seed", "a signed 32-bit seed")?;
    let path = args.path("a layout or state document")?;
    args.finish()?;
    let (kind, bytes) = Kind::read(&path)?;
    kind.require("preview")?;
    let layout = match kind {
        Kind::Layout => bytes.clone(),
        Kind::State => {
            let layout =
                mechcore_document::project::read_state(&bytes).map_err(Failure::refused)?;
            mechcore_document::canonical_embedded_yaml(layout)
                .map_err(Failure::failed)?
                .into_bytes()
        }
        _ => unreachable!("kind table restricts preview input"),
    };
    let preview = mechcore_simulation::preview_document(&layout, seed)
        .map_err(|e| Failure::unsimulated(&e))?;
    let document: serde_yaml::Value =
        serde_yaml::from_slice(&bytes).map_err(|e| Failure::refused(e.to_string()))?;
    let mut answer = serde_json::to_value(preview).map_err(|e| Failure::failed(e.to_string()))?;
    answer["schema"] = serde_json::json!("mechcore.preview");
    answer["document"] =
        serde_json::to_value(document).map_err(|e| Failure::failed(e.to_string()))?;
    answer["coordinates"] = serde_json::json!("own_side_metres");
    emit(&answer, format)?;
    Ok(Verdict::Yes)
}
