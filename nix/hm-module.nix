{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.programs.framr;
  tomlFormat = pkgs.formats.toml {};
in {
  options.programs.framr = {
    enable = lib.mkEnableOption "framr";
    package = lib.mkOption {
      type = lib.types.package;
      description = "The framr package to use.";
    };
    settings = lib.mkOption {
      type = tomlFormat.type;
      default = {};
      example = lib.literalExpression ''
        {
          default_action = "Save";
          default_capture = "Area";
          uploaders = [
            {
              name = "My Uploader";
              request_method = "POST";
              request_url = "https://example.com/upload";
              body_type = "FormData";
              file_form_name = "file";
              output_url = "{json:url}";
            }
          ];
        }
      '';
      description = ''
        Configuration written to {file}`$XDG_CONFIG_HOME/framr/default-config.toml`.
        See the project's documentation for more information on the available options.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [cfg.package];

    xdg.configFile."framr/default-config.toml" = lib.mkIf (cfg.settings != {}) {
      source = tomlFormat.generate "framr-config" cfg.settings;
    };

    xdg.mimeApps.defaultApplications = {
      "x-scheme-handler/framr" = ["framr-handler.desktop"];
    };
  };
}
