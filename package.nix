{
  lib,
  stdenv,
  rustPlatform,
  withAdapter ? false,
}:

assert lib.assertMsg (
  !withAdapter || stdenv.hostPlatform.isDarwin
) "mechcore's adapter requires macOS";

rustPlatform.buildRustPackage {
  pname = "mechcore";
  version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./GAME_VERSION
      ./crates
      ./config
      ./docs
    ];
  };

  cargoLock.lockFile = ./Cargo.lock;
  cargoBuildFlags = [
    "-p"
    "mechcore"
  ]
  ++ lib.optionals withAdapter [
    "-p"
    "mechcore-adapter"
  ];
  # Package-local unit tests need neither a running game nor recordings.
  # Repository CI owns the workspace integration and fixture regression suites.
  cargoTestFlags = [
    "-p"
    "mechcore"
    "--bin"
    "mechcore"
  ];

  meta = {
    description = "Deterministic Mechabellum simulator, recording toolkit, and battle player";
    homepage = "https://github.com/qbisi/mechcore";
    mainProgram = "mechcore";
    platforms =
      if withAdapter then
        [ "aarch64-darwin" ]
      else
        [
          "x86_64-linux"
          "aarch64-linux"
          "aarch64-darwin"
        ];
  };
}
