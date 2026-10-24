{
  description = "bone, a personal mail server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      flake.nixosModules.server = ./nix/server.nix;

      perSystem =
        {
          pkgs,
          self',
          system,
          ...
        }:
        let
          androidPkgs = import inputs.nixpkgs {
            inherit system;
            config = {
              allowUnfree = true;
              android_sdk.accept_license = true;
            };
          };
        in
        {
          packages = {
            bone = pkgs.callPackage ./nix/package.nix { };
            bone-web = pkgs.callPackage ./nix/web.nix { };
            default = self'.packages.bone;
          };

          devShells.default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              clippy
              rustfmt
              rust-analyzer
              nodejs
              pnpm
              pkg-config
              openssl
              sqlite
            ];
          };

          devShells.android = androidPkgs.callPackage ./nix/android-shell.nix { };
        };
    };
}
