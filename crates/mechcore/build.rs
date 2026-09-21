use std::{env, fs, io, path::PathBuf};

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed=build.rs");
    // Without the `adapter` feature there is nothing to package: the binary
    // runs everything that needs no game, and says so when asked to launch one.
    if env::var_os("CARGO_FEATURE_ADAPTER").is_none() {
        return Ok(());
    }
    let artifact = PathBuf::from(
        env::var_os("CARGO_CDYLIB_FILE_MECHCORE_ADAPTER_mechcore_adapter")
            .expect("Cargo must supply the mechcore-adapter cdylib artifact"),
    );
    // Cargo's old and new build-directory layouts both place build-script
    // output beneath <target>/<profile>/build. Do not count path components:
    // nightly can separate the package name and hash into two directories.
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must set OUT_DIR"));
    let directory = out
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "build"))
        .and_then(|path| path.parent())
        .expect("Cargo profile directory above build-script output");
    let destination = directory.join(artifact.file_name().expect("adapter filename"));
    println!("cargo::rerun-if-changed={}", artifact.display());
    // Also repair a removed or replaced packaged copy on the next cargo run.
    println!("cargo::rerun-if-changed={}", destination.display());
    let bytes = fs::read(&artifact)?;
    if fs::read(&destination).ok().as_deref() != Some(bytes.as_slice()) {
        // A fresh inode avoids modifying a dylib mapped by an existing game.
        let temporary = tempfile::NamedTempFile::new_in(directory)?;
        fs::copy(&artifact, temporary.path())?;
        // Watching an output stamped after this build script started would
        // unnecessarily dirty the next build. Preserve the input timestamp.
        temporary
            .as_file()
            .set_times(fs::FileTimes::new().set_modified(fs::metadata(&artifact)?.modified()?))?;
        temporary
            .persist(&destination)
            .map_err(|error| error.error)?;
    }
    Ok(())
}
