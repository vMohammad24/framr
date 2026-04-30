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
          src = let
            cargoFilter = craneLib.filterCargoSources;
            assetsFilter = path: type:
              (builtins.match ".*/assets/.*" path != null) || (cargoFilter path type);
          in
            pkgs.lib.cleanSourceWith {
              src = ./.;
              filter = assetsFilter;
              name = "framr-source";
            };
          strictDeps = true;

          buildInputs = with pkgs; [
            dbus
            wayland
            libxkbcommon
            cairo
            pango
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
            postInstall = ''
              ls -lha assets
              install -Dm644 assets/framr-handler.desktop -t $out/share/applications
            '';
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
        programs.framr.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      };
    };
}
