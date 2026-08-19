mod adapter;
mod mcp;

use std::process::ExitCode;

fn usage(program: &str) {
    eprintln!("usage: {program} mcp");
}

fn main() -> ExitCode {
    let mut arguments = std::env::args();
    let program = arguments.next().unwrap_or_else(|| "mechcore".into());
    if let (Some("mcp"), None) = (arguments.next().as_deref(), arguments.next()) {
        match mcp::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("mechcore mcp: {error}");
                ExitCode::FAILURE
            }
        }
    } else {
        usage(&program);
        ExitCode::from(2)
    }
}
