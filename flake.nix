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

        cargoSrc = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = craneLib.filterCargoSources;
          name = "framr-cargo-source";
        };

        commonArgs = {
          src = cargoSrc;
          strictDeps = true;

          buildInputs = with pkgs; [
            dbus
            wayland
            libxkbcommon
            cairo
            pango
            libxcursor
            mesa
            libgbm
            libdrm
            gst_all_1.gstreamer.dev
            gst_all_1.gst-plugins-base.dev
            gst_all_1.gst-plugins-good
            gst_all_1.gst-plugins-ugly
            gst_all_1.gst-plugins-bad
            gst_all_1.gst-plugins-rs
            gst_all_1.gst-vaapi
            pipewire
          ];

          nativeBuildInputs = with pkgs; [
            pkg-config
            rustPlatform.bindgenHook
            makeWrapper
            gst_all_1.gstreamer.dev
            gst_all_1.gst-plugins-base.dev
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        framr = craneLib.buildPackage (commonArgs
          // {
            inherit cargoArtifacts;

            src = pkgs.lib.cleanSourceWith {
              src = ./.;
              filter = path: type:
                (builtins.match ".*/assets/.*" path != null) || (craneLib.filterCargoSources path type);
              name = "framr-full-source";
            };

            postInstall = ''
              ls -lha assets
              install -Dm644 assets/framr-handler.desktop -t $out/share/applications

              wrapProgram $out/bin/framr \
                --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "$GST_PLUGIN_SYSTEM_PATH_1_0"
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
