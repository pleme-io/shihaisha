# Shihaisha home-manager module -- unified service management
#
# Namespace: blackmatter.components.shihaisha.*
#
# Declarative service definitions in Nix that auto-translate to shikumi YAML
# files consumed by shihaisha at runtime.  The same service definition works
# on both Darwin (launchd) and Linux (systemd) -- the backend is selected at
# runtime by shihaisha.
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

  # ── YAML format for pkgs.formats.yaml {} ────────────────────────────────
  settingsFormat = pkgs.formats.yaml {};

  # ── Health check submodule ──────────────────────────────────────────────
  # Mirrors shihaisha-core HealthCheckSpec (tagged enum: type + variant fields).
  healthCheckModule = types.submodule {
    options = {
      type = mkOption {
        type = types.enum ["http" "tcp" "command" "file"];
        description = "Health check type (http, tcp, command, file).";
      };

      endpoint = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "URL to probe (http type).";
        example = "http://localhost:8080/health";
      };

      address = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Address to connect to (tcp type).";
        example = "127.0.0.1:5432";
      };

      command = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Command to execute (command type).";
      };

      args = mkOption {
        type = types.listOf types.str;
        default = [];
        description = "Arguments for health check command.";
      };

      path = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "File path to check (file type).";
      };

      interval_secs = mkOption {
        type = types.int;
        default = 30;
        description = "Interval between checks in seconds.";
      };

      timeout_secs = mkOption {
        type = types.int;
        default = 5;
        description = "Timeout for each check in seconds (http type only).";
      };

      max_failures = mkOption {
        type = types.int;
        default = 3;
        description = "Consecutive failures before marking unhealthy.";
      };
    };
  };

  # ── Resource limits submodule ───────────────────────────────────────────
  # Mirrors shihaisha-core ResourceLimits.
  resourceLimitsModule = types.submodule {
    options = {
      memory_max = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Maximum memory (e.g. '512M', '1G'). Maps to MemoryMax=.";
      };

      memory_high = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Memory high watermark. Maps to MemoryHigh=.";
      };

      cpu_weight = mkOption {
        type = types.nullOr types.int;
        default = null;
        description = "CPU weight (1-10000). Maps to CPUWeight=.";
      };

      cpu_quota = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "CPU quota (e.g. '50%', '200%'). Maps to CPUQuota=.";
      };

      tasks_max = mkOption {
        type = types.nullOr types.int;
        default = null;
        description = "Maximum number of tasks/threads. Maps to TasksMax=.";
      };

      io_weight = mkOption {
        type = types.nullOr types.int;
        default = null;
        description = "I/O weight (1-10000). Maps to IOWeight=.";
      };

      nice = mkOption {
        type = types.nullOr types.int;
        default = null;
        description = "Nice value (-20 to 19). Maps to Nice=.";
      };
    };
  };

  # ── Socket spec submodule ───────────────────────────────────────────────
  # Mirrors shihaisha-core SocketSpec.
  socketSpecModule = types.submodule {
    options = {
      listen = mkOption {
        type = types.str;
        description = "Listen address (e.g. '127.0.0.1:8080' or '/run/myservice.sock').";
      };

      socket_type = mkOption {
        type = types.enum ["stream" "datagram" "sequential"];
        default = "stream";
        description = "Socket type.";
      };

      name = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "File descriptor name (LISTEN_FDNAMES).";
      };
    };
  };

  # ── Logging spec submodule ──────────────────────────────────────────────
  # Mirrors shihaisha-core LoggingSpec.  LogTarget is simplified to a string
  # for the Nix interface: "journal", "null", "inherit", or a file path.
  logTargetType = types.str;

  loggingModule = types.submodule {
    options = {
      stdout = mkOption {
        type = logTargetType;
        default = "journal";
        description = ''
          Where to send stdout: "journal", "null", "inherit", or a file path.
        '';
      };

      stderr = mkOption {
        type = logTargetType;
        default = "journal";
        description = ''
          Where to send stderr: "journal", "null", "inherit", or a file path.
        '';
      };
    };
  };

  # ── Convert a log target string to the YAML representation ─────────────
  # shihaisha-core uses serde tagged enums for LogTarget.  The YAML form is:
  #   journal  ->  journal
  #   null     ->  null
  #   inherit  ->  inherit
  #   /path    ->  !file /path  (serde_yaml_ng externally-tagged with data)
  #
  # Since pkgs.formats.yaml cannot emit YAML tags, we pass file targets as
  # an attrset that shihaisha also accepts: { file: "/path" }.
  logTargetToYaml = target:
    if target == "journal" || target == "null" || target == "inherit"
    then target
    else {file = target;};

  # ── Convert a single health check definition to YAML attrs ─────────────
  healthToYaml = h: let
    base = {
      inherit (h) type interval_secs max_failures;
    };
  in
    base
    // optionalAttrs (h.type == "http") {
      inherit (h) endpoint timeout_secs;
    }
    // optionalAttrs (h.type == "tcp") {
      inherit (h) address;
    }
    // optionalAttrs (h.type == "command") {
      inherit (h) command args;
    }
    // optionalAttrs (h.type == "file") {
      inherit (h) path;
    };

  # ── Convert resource limits to YAML attrs (omit null fields) ────────────
  resourcesToYaml = r:
    filterAttrs (_: v: v != null) {
      inherit (r) memory_max memory_high cpu_weight cpu_quota tasks_max io_weight nice;
    };

  # ── Convert a single service definition to a shihaisha YAML file ────────
  serviceToYaml = name: svc: let
    # Base attrs that are always present
    base = {
      inherit name;
      inherit (svc) description command args notify;
      service_type = svc.serviceType;
      timeout_start_sec = svc.timeoutStartSec;
      timeout_stop_sec = svc.timeoutStopSec;
      watchdog_sec = svc.watchdogSec;

      restart = {
        inherit (svc.restart) strategy max_retries reset_after_secs;
        delay_secs = svc.restart.delaySecs;
      };

      depends_on = {
        inherit (svc.dependsOn) after before requires wants conflicts;
      };

      environment = svc.environment;
    };

    # Logging
    loggingAttr = {
      logging = {
        stdout = logTargetToYaml svc.logging.stdout;
        stderr = logTargetToYaml svc.logging.stderr;
      };
    };

    # Optional attrs
    optional =
      optionalAttrs (svc.workingDirectory != null) {working_directory = svc.workingDirectory;}
      // optionalAttrs (svc.user != null) {user = svc.user;}
      // optionalAttrs (svc.group != null) {group = svc.group;}
      // optionalAttrs (svc.health != null) {health = healthToYaml svc.health;}
      // optionalAttrs (svc.sockets != []) {sockets = map (s: filterAttrs (_: v: v != null) {inherit (s) listen socket_type name;}) svc.sockets;}
      // optionalAttrs (svc.resources != null) {resources = resourcesToYaml svc.resources;};

    # Backend overrides
    overridesAttr =
      optionalAttrs (svc.overrides.systemd != {} || svc.overrides.launchd != {}) {
        overrides =
          {}
          // optionalAttrs (svc.overrides.systemd != {}) {inherit (svc.overrides) systemd;}
          // optionalAttrs (svc.overrides.launchd != {}) {inherit (svc.overrides) launchd;};
      };

    merged = base // loggingAttr // optional // overridesAttr;
  in
    settingsFormat.generate "${name}.yaml" merged;

  # ── Service submodule type ──────────────────────────────────────────────
  # Each service in `cfg.services` maps to one YAML file under
  # ~/.config/shihaisha/services/<name>.yaml
  serviceModule = types.submodule ({name, ...}: {
    options = {
      enable = mkEnableOption "this shihaisha service" // {default = true;};

      description = mkOption {
        type = types.str;
        default = "${name} managed by shihaisha";
        description = "Human-readable service description.";
      };

      command = mkOption {
        type = types.str;
        description = "Command to execute (program path).";
      };

      args = mkOption {
        type = types.listOf types.str;
        default = [];
        description = "Command arguments.";
      };

      serviceType = mkOption {
        type = types.enum ["simple" "oneshot" "notify" "forking" "timer" "socket"];
        default = "simple";
        description = "Service type (maps to systemd Type= / launchd KeepAlive).";
      };

      workingDirectory = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Working directory for the service process.";
      };

      user = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "User to run the service as.";
      };

      group = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Group to run the service as.";
      };

      environment = mkOption {
        type = types.attrsOf types.str;
        default = {};
        description = "Environment variables passed to the service.";
      };

      notify = mkOption {
        type = types.bool;
        default = false;
        description = "Enable sd_notify readiness protocol.";
      };

      watchdogSec = mkOption {
        type = types.int;
        default = 0;
        description = "Watchdog interval in seconds (0 = disabled).";
      };

      timeoutStartSec = mkOption {
        type = types.int;
        default = 90;
        description = "Timeout for starting the service in seconds.";
      };

      timeoutStopSec = mkOption {
        type = types.int;
        default = 90;
        description = "Timeout for stopping the service in seconds.";
      };

      # ── Restart policy ───────────────────────────────────────────────
      restart = {
        strategy = mkOption {
          type = types.enum ["always" "on-failure" "on-success" "never"];
          default = "on-failure";
          description = "Restart strategy (serialized as kebab-case).";
        };

        delaySecs = mkOption {
          type = types.int;
          default = 5;
          description = "Delay between restart attempts in seconds.";
        };

        maxRetries = mkOption {
          type = types.int;
          default = 0;
          description = "Maximum restart attempts (0 = unlimited).";
        };

        reset_after_secs = mkOption {
          type = types.int;
          default = 300;
          description = "Reset retry counter after this many seconds of successful running.";
        };
      };

      # ── Dependency ordering ──────────────────────────────────────────
      dependsOn = {
        after = mkOption {
          type = types.listOf types.str;
          default = [];
          description = "Services that must start before this one.";
        };

        before = mkOption {
          type = types.listOf types.str;
          default = [];
          description = "Services that must start after this one.";
        };

        requires = mkOption {
          type = types.listOf types.str;
          default = [];
          description = "Hard dependencies (pulled into the transaction).";
        };

        wants = mkOption {
          type = types.listOf types.str;
          default = [];
          description = "Soft dependencies (no failure on missing).";
        };

        conflicts = mkOption {
          type = types.listOf types.str;
          default = [];
          description = "Conflicting services (stopped when this starts).";
        };
      };

      # ── Health check ─────────────────────────────────────────────────
      health = mkOption {
        type = types.nullOr healthCheckModule;
        default = null;
        description = "Health check specification.";
        example = {
          type = "http";
          endpoint = "http://localhost:8080/health";
          interval_secs = 30;
        };
      };

      # ── Socket activation ────────────────────────────────────────────
      sockets = mkOption {
        type = types.listOf socketSpecModule;
        default = [];
        description = "Socket activation specifications.";
        example = [
          {
            listen = "127.0.0.1:8080";
            socket_type = "stream";
          }
        ];
      };

      # ── Resource limits ──────────────────────────────────────────────
      resources = mkOption {
        type = types.nullOr resourceLimitsModule;
        default = null;
        description = "Resource limits for the service.";
        example = {
          memory_max = "512M";
          cpu_weight = 100;
        };
      };

      # ── Logging ──────────────────────────────────────────────────────
      logging = mkOption {
        type = loggingModule;
        default = {};
        description = ''
          Logging configuration.  Targets: "journal", "null", "inherit",
          or a file path (e.g. "/var/log/myapp/stdout.log").
        '';
      };

      # ── Backend-specific overrides (escape hatch) ────────────────────
      overrides = {
        systemd = mkOption {
          type = types.attrsOf (types.attrsOf types.str);
          default = {};
          description = ''
            Raw systemd unit directives keyed by section.
            Injected into the backend-native config as-is.
          '';
          example = {
            Service = {
              LimitNOFILE = "65536";
              PrivateTmp = "true";
            };
          };
        };

        launchd = mkOption {
          type = types.attrsOf types.anything;
          default = {};
          description = ''
            Raw launchd plist keys.
            Injected into the backend-native config as-is.
          '';
          example = {
            ThrottleInterval = 10;
            LowPriorityIO = true;
          };
        };
      };
    };
  });

  # ── Global shihaisha settings → YAML ────────────────────────────────────
  globalSettingsAttr = let
    general =
      filterAttrs (_: v: v != null) {
        backend = cfg.backend;
        log_level = cfg.log_level;
        service_dir = cfg.service_dir;
      };
  in
    filterAttrs (_: v: v != {} && v != null) {
      inherit general;
    }
    // cfg.extraSettings;

  globalYamlConfig = settingsFormat.generate "shihaisha.yaml" globalSettingsAttr;

  # ── Enabled services filter ─────────────────────────────────────────────
  enabledServices = filterAttrs (_: svc: svc.enable) cfg.services;
in {
  options.blackmatter.components.shihaisha = {
    enable = mkEnableOption "shihaisha unified service management";

    package = mkOption {
      type = types.package;
      default = pkgs.shihaisha;
      description = "The shihaisha package to use.";
    };

    # ── Global settings ──────────────────────────────────────────────────
    backend = mkOption {
      type = types.enum ["auto" "systemd" "launchd" "native"];
      default = "auto";
      description = "Backend to use for all services (auto-detected at runtime when 'auto').";
    };

    service_dir = mkOption {
      type = types.str;
      default = "~/.config/shihaisha/services";
      description = "Directory where shihaisha looks for service spec YAML files.";
    };

    log_level = mkOption {
      type = types.enum ["trace" "debug" "info" "warn" "error"];
      default = "info";
      description = "Log level for the shihaisha daemon.";
    };

    # ── Declarative service definitions ──────────────────────────────────
    services = mkOption {
      type = types.attrsOf serviceModule;
      default = {};
      description = ''
        Declarative service definitions.  Each entry produces a YAML spec file
        under ~/.config/shihaisha/services/<name>.yaml that shihaisha reads
        and translates to the native backend format at runtime.
      '';
      example = {
        my-api = {
          command = "/usr/bin/my-api";
          args = ["--port" "8080"];
          restart.strategy = "always";
          health = {
            type = "http";
            endpoint = "http://localhost:8080/health";
          };
          environment.RUST_LOG = "info";
        };
      };
    };

    # ── Daemon settings ──────────────────────────────────────────────────
    daemon = {
      enable = mkOption {
        type = types.bool;
        default = false;
        description = "Run shihaisha as a background daemon (watches config directory for changes).";
      };
    };

    # ── Escape hatch ─────────────────────────────────────────────────────
    extraSettings = mkOption {
      type = types.attrs;
      default = {};
      description = ''
        Additional raw settings merged on top of typed options in the global
        shihaisha.yaml config.  Values are serialized directly to YAML.
      '';
    };
  };

  config = mkIf cfg.enable (mkMerge [
    # Install the package
    {
      home.packages = [cfg.package];
    }

    # Global YAML configuration
    {
      xdg.configFile."shihaisha/shihaisha.yaml".source = globalYamlConfig;
    }

    # Per-service YAML spec files
    {
      xdg.configFile = mapAttrs' (name: svc:
        nameValuePair "shihaisha/services/${name}.yaml" {
          source = serviceToYaml name svc;
        })
      enabledServices;
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
