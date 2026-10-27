{
  description = "Deterministic Mechabellum simulator, recording toolkit, and battle player";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      overlays.default = nixpkgs.lib.composeManyExtensions [
        rust-overlay.overlays.default
        (final: _prev: {
          mechcore =
            let
              rust = final.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
            in
            final.callPackage ./package.nix {
              rustPlatform = final.makeRustPlatform {
                cargo = rust;
                rustc = rust;
              };
            };
        })
      ];

      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ self.overlays.default ];
          };
        in
        {
          inherit (pkgs) mechcore;
          default = pkgs.mechcore;
        }
        // nixpkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
          mechcore-with-adapter = pkgs.mechcore.override { withAdapter = true; };
        }
      );

      checks = forAllSystems (
        system:
        {
          inherit (self.packages.${system}) mechcore;
        }
        // nixpkgs.lib.optionalAttrs (nixpkgs.lib.hasSuffix "-darwin" system) {
          inherit (self.packages.${system}) mechcore-with-adapter;
        }
      );

      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt-tree);
    };
}
