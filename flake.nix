{
  description = "diffy's attic";

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

      flake.nixosModules.bone = ./bone/nix/server.nix;

      perSystem =
        {
          pkgs,
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
            bone = pkgs.callPackage ./bone/nix/package.nix { };
            bone-web = pkgs.callPackage ./bone/nix/web.nix { };
          };

          devShells.bone = pkgs.mkShell {
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

          devShells.bone-android = androidPkgs.callPackage ./bone/nix/android-shell.nix { };
        };
    };
}
