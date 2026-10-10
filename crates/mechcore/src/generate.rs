//! `generate`: draws layouts that cover every pair of decisions a fight tells
//! apart.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use crate::cli::{Args, Failure, Outcome, Verdict};

#[derive(Serialize)]
struct Written {
    index: usize,
    path: String,
    new_pairs: usize,
}

#[derive(Serialize)]
struct Uncovered {
    pair: [String; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    refusal: Option<String>,
}

#[derive(Serialize)]
struct Report {
    schema: &'static str,
    seed: u64,
    count: usize,
    pairs: usize,
    covered: usize,
    layouts: Vec<Written>,
    uncovered: Vec<Uncovered>,
}

/// Draws the first `--count` layouts of the batch `--seed` starts, and writes
/// each into `<out>` as `<seed>-<index>.yaml`; or, with `--index`, writes that
/// one layout of the batch on standard output.
///
/// # Errors
///
/// Returns a usage failure for a command the contract does not define, a
/// refusal when the batch cannot be drawn, and a failure when a file cannot
/// be written.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let force = arguments.flag("--force")?;
    let seed = arguments
        .parsed::<u64>("--seed", "an unsigned 64-bit integer")?
        .ok_or_else(|| Failure::usage("generate needs --seed <u64>"))?;
    let index = arguments.parsed::<usize>("--index", "a layout's place in the batch")?;
    let count = arguments.parsed::<usize>("--count", "a number of layouts")?;
    let out = if arguments.is_empty() {
        None
    } else {
        Some(arguments.path("the directory to write")?)
    };
    arguments.finish()?;
    match (index, count, out) {
        (Some(index), None, None) => {
            let batch =
                mechcore_document::generate::generate(seed, index + 1).map_err(Failure::refused)?;
            let layout = batch
                .layouts
                .into_iter()
                .last()
                .map(|generated| generated.layout);
            let yaml = layout
                .ok_or_else(|| Failure::refused("the batch drew no layout"))
                .and_then(|layout| {
                    mechcore_document::canonical_yaml(layout).map_err(Failure::failed)
                })?;
            print!("{yaml}");
            Ok(Verdict::Yes)
        }
        (None, Some(count), Some(out)) => write_batch(seed, count, &out, force, format),
        _ => Err(Failure::usage(
            "expected --seed <u64> --count <n> <dir>, or --seed <u64> --index <i>",
        )),
    }
}

fn write_batch(
    seed: u64,
    count: usize,
    out: &PathBuf,
    force: bool,
    format: crate::cli::Format,
) -> Outcome {
    let batch = mechcore_document::generate::generate(seed, count).map_err(Failure::refused)?;
    fs::create_dir_all(out)
        .map_err(|error| Failure::failed(format!("cannot create {}: {error}", out.display())))?;
    let mut layouts = Vec::with_capacity(batch.layouts.len());
    for generated in batch.layouts {
        let path = out.join(format!("{seed}-{:04}.yaml", generated.index));
        if path.exists() && !force {
            return Err(Failure::refused(format!(
                "{} exists; --force replaces it",
                path.display()
            )));
        }
        let yaml = mechcore_document::canonical_yaml(generated.layout).map_err(Failure::failed)?;
        fs::write(&path, yaml).map_err(|error| {
            Failure::failed(format!("cannot write {}: {error}", path.display()))
        })?;
        layouts.push(Written {
            index: generated.index,
            path: path.display().to_string(),
            new_pairs: generated.new_pairs,
        });
    }
    crate::cli::emit(
        &Report {
            schema: "mechcore.generate-result",
            seed,
            count,
            pairs: batch.pairs,
            covered: batch.covered,
            layouts,
            uncovered: batch
                .uncovered
                .into_iter()
                .map(|uncovered| Uncovered {
                    pair: [uncovered.pair.0, uncovered.pair.1],
                    refusal: uncovered.refusal,
                })
                .collect(),
        },
        format,
    )?;
    Ok(Verdict::Yes)
}
