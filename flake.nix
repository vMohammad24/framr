{
  description = "Framr - A Wayland screenshot tool written in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
        };

        commonBuildInputs = with pkgs; [
          dbus
          wayland
          libxkbcommon
          cairo
          libxcursor
          libgbm
        ];

        commonNativeBuildInputs = with pkgs; [
          pkg-config
          rustPlatform.bindgenHook
        ];
      in {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "framr";
          version = "0.1.0";
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          nativeBuildInputs = commonNativeBuildInputs;
          buildInputs = commonBuildInputs;
        };

        apps.default = flake-utils.lib.mkApp {
          drv = self.packages.${system}.default;
        };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs =
            commonNativeBuildInputs
            ++ (with pkgs; [
              cargo
              rustc
              rustfmt
              clippy
            ]);

          buildInputs = commonBuildInputs;

          inputsFrom = [self.packages.${system}.default];
        };
      }
    );
}
