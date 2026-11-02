//! `mask`: geometric purchase positions on a layout or decision observation.

use crate::{
    cli::{Args, Failure, Outcome, Verdict, emit},
    kind::Kind,
    turn::Side,
};

pub(crate) fn run(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let side = arguments
        .value("--side")?
        .ok_or_else(|| Failure::usage("mask needs --side blue|red"))?;
    let side = Side::parse(&side)?;
    let unit = arguments
        .value("--unit")?
        .ok_or_else(|| Failure::usage("mask needs --unit <type>"))?;
    let path = arguments.path("a layout, state or match document")?;
    arguments.finish()?;
    let (kind, bytes) = Kind::read(&path)?;
    kind.require("mask")?;
    let layout = match kind {
        Kind::Layout => {
            let layout =
                mechcore_document::parse_embedded_yaml(&bytes).map_err(Failure::refused)?;
            crate::r#match::warn_build(&layout.game_build);
            layout
        }
        Kind::State => mechcore_document::project::read_state(&bytes).map_err(Failure::refused)?,
        Kind::Match => crate::r#match::placement_layout(&path, side)?,
        _ => unreachable!("kind table only admits layout, state and match"),
    };
    let mask =
        mechcore_document::mask::purchase(layout, side.red(), &unit).map_err(Failure::refused)?;
    emit(&mask, format)?;
    Ok(Verdict::Yes)
}
