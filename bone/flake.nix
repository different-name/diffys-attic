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
        { pkgs, self', ... }:
        {
          packages = {
            bone = pkgs.callPackage ./nix/package.nix { };
            default = self'.packages.bone;
          };

          devShells.default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              clippy
              rustfmt
              rust-analyzer
              pkg-config
              openssl
              sqlite
            ];
          };
        };
    };
}
