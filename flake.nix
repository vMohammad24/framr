{
  description = "Framr - A Wayland screenshot tool written in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  nixConfig = {
    extra-substituters = ["https://framr.cachix.org"];
    extra-trusted-public-keys = ["framr.cachix.org-1:Nn6BXpOrE0I1sO89xW8l2WVcf2FD4UqU6PD30sgRLZk="];
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    crane,
  }: let
    outputs = flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {inherit system;};
        craneLib = crane.mkLib pkgs;

        commonArgs = {
          src = craneLib.cleanCargoSource (craneLib.path ./.);
          strictDeps = true;

          buildInputs = with pkgs; [
            dbus
            wayland
            libxkbcommon
            cairo
            libxcursor
          ];

          nativeBuildInputs = with pkgs; [
            pkg-config
            rustPlatform.bindgenHook
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        framr = craneLib.buildPackage (commonArgs
          // {
            inherit cargoArtifacts;
          });
      in {
        packages.default = framr;

        devShells.default = pkgs.mkShell {
          inputsFrom = [framr];
          nativeBuildInputs = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
          ];
        };
      }
    );
  in
    outputs
    // {
      homeManagerModules.default = {
        pkgs,
        lib,
        ...
      }: {
        imports = [./nix/hm-module.nix];
        programs.framr.package = lib.mkDefault self.packages.${pkgs.system}.default;
      };
    };
}
