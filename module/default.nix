# Shihaisha home-manager module -- unified service management
#
# Namespace: blackmatter.components.shihaisha.*
#
# Generates YAML config from typed Nix options, loaded by shikumi at runtime.
#
# Module factory: receives { hmHelpers } from flake.nix, returns HM module.
{hmHelpers}: {
  lib,
  config,
  pkgs,
  ...
}:
with lib; let
  inherit (hmHelpers) mkLaunchdService mkSystemdService;
  cfg = config.blackmatter.components.shihaisha;
  isDarwin = pkgs.stdenv.isDarwin;

  logDir =
    if isDarwin
    then "${config.home.homeDirectory}/Library/Logs"
    else "${config.home.homeDirectory}/.local/share/shihaisha/logs";

  # -- YAML config generation ------------------------------------------------
  settingsAttr = let
    general =
      filterAttrs (_: v: v != null) {
        inherit (cfg) backend log_level;
        service_dir = cfg.service_dir;
      };
  in
    filterAttrs (_: v: v != {} && v != null) {
      inherit general;
    }
    // cfg.extraSettings;

  yamlConfig =
    pkgs.writeText "shihaisha.yaml"
    (lib.generators.toYAML {} settingsAttr);
in {
  options.blackmatter.components.shihaisha = {
    enable = mkEnableOption "shihaisha unified service management";

    package = mkOption {
      type = types.package;
      default = pkgs.shihaisha;
      description = "The shihaisha package to use.";
    };

    # -- General settings ----------------------------------------------------
    backend = mkOption {
      type = types.str;
      default = "auto";
      description = "Backend to use: auto, systemd, launchd, native.";
    };

    service_dir = mkOption {
      type = types.str;
      default = "~/.config/shihaisha/services";
      description = "Directory for service spec YAML files.";
    };

    log_level = mkOption {
      type = types.str;
      default = "info";
      description = "Log level for shihaisha daemon.";
    };

    # -- Daemon settings -----------------------------------------------------
    daemon = {
      enable = mkOption {
        type = types.bool;
        default = false;
        description = "Run shihaisha as a background daemon (watch config directory for changes).";
      };
    };

    # -- Escape hatch --------------------------------------------------------
    extraSettings = mkOption {
      type = types.attrs;
      default = {};
      description = ''
        Additional raw settings merged on top of typed options.
        Values are serialized directly to YAML.
      '';
    };
  };

  config = mkIf cfg.enable (mkMerge [
    # Install the package
    {
      home.packages = [cfg.package];
    }

    # YAML configuration
    {
      xdg.configFile."shihaisha/shihaisha.yaml".source = yamlConfig;
    }

    # Darwin: launchd agent (daemon mode)
    (mkIf (cfg.daemon.enable && isDarwin)
      (mkLaunchdService {
        name = "shihaisha";
        label = "io.pleme.shihaisha";
        command = "${cfg.package}/bin/shihaisha";
        args = ["daemon"];
        logDir = logDir;
        processType = "Background";
        keepAlive = true;
      }))

    # Linux: systemd user service (daemon mode)
    (mkIf (cfg.daemon.enable && !isDarwin)
      (mkSystemdService {
        name = "shihaisha";
        description = "shihaisha unified service management daemon";
        command = "${cfg.package}/bin/shihaisha";
        args = ["daemon"];
      }))
  ]);
}
