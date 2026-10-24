{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.bone;
  json = pkgs.formats.json { };

  credDir = "@CREDENTIALS_DIRECTORY@";

  credentials =
    lib.concatLists (
      lib.mapAttrsToList (name: a: [
        "account-${name}-login:${toString a.loginFile}"
        "account-${name}-password:${toString a.passwordFile}"
      ]) cfg.accounts
    )
    ++ lib.mapAttrsToList (name: d: "device-${name}:${toString d.tokenFile}") cfg.devices;

  settings = {
    inherit (cfg) bind;
    data_dir = "/var/lib/bone";
    cache_dir = "/var/cache/bone";
    web_dir = if cfg.webDir == null then null else toString cfg.webDir;
    accounts = lib.mapAttrsToList (name: a: {
      inherit name;
      inherit (a) imap smtp;
      catch_all = a.catchAll;
      login_file = "${credDir}/account-${name}-login";
      password_file = "${credDir}/account-${name}-password";
    }) cfg.accounts;
    devices = lib.mapAttrsToList (name: _: {
      inherit name;
      token_file = "${credDir}/device-${name}";
    }) cfg.devices;
  };

  configFile = json.generate "bone.json" settings;

  start = pkgs.writeShellScript "bone-start" ''
    set -eu
    template=$(< ${configFile})
    printf '%s\n' "''${template//${credDir}/''${CREDENTIALS_DIRECTORY:-}}" > "$RUNTIME_DIRECTORY/server.json"
    export BONE_SERVER_CONFIG="$RUNTIME_DIRECTORY/server.json"
    exec ${lib.getExe cfg.package}
  '';
in
{
  options.services.bone = {
    enable = lib.mkEnableOption "the bone mail server";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalMD "the bone package from this repository";
      description = "The bone package to use.";
    };

    bind = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:8790";
      example = "100.64.0.1:8790";
      description = "Address and port the API listens on, as `host:port`.";
    };

    accounts = lib.mkOption {
      default = { };
      description = ''
        Mailboxes to sync, keyed by a short name used in logs.
        Clients see all of them as one mailbox.
      '';
      example = lib.literalExpression ''
        {
          main = {
            loginFile = "/run/agenix/bone-main-login";
            passwordFile = "/run/agenix/bone-main-password";
          };
        }
      '';
      type = lib.types.attrsOf (
        lib.types.submodule {
          options = {
            loginFile = lib.mkOption {
              type = lib.types.path;
              description = "File containing the mailbox address used to log in.";
            };

            passwordFile = lib.mkOption {
              type = lib.types.path;
              description = "File containing the mailbox password.";
            };

            imap = lib.mkOption {
              type = lib.types.str;
              default = "imap.migadu.com:993";
              description = "IMAP server as `host:port`, connected over TLS.";
            };

            smtp = lib.mkOption {
              type = lib.types.str;
              default = "smtp.migadu.com:465";
              description = "SMTP submission server as `host:port`, connected over TLS.";
            };

            catchAll = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = ''
                Whether this mailbox receives all mail for its login's domain.
                New addresses for services are made on that domain.
              '';
            };
          };
        }
      );
    };

    webDir = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = pkgs.callPackage ./web.nix { };
      defaultText = lib.literalMD "the web client from this repository";
      description = ''
        Directory of a built web client, served next to the API.
        When null, only the API is served.
      '';
    };

    devices = lib.mkOption {
      default = { };
      description = "Devices allowed to use the API, keyed by name, each with a fixed token.";
      example = lib.literalExpression ''
        { phone.tokenFile = "/run/agenix/bone-phone-token"; }
      '';
      type = lib.types.attrsOf (
        lib.types.submodule {
          options.tokenFile = lib.mkOption {
            type = lib.types.path;
            description = "File containing the token the device authenticates with.";
          };
        }
      );
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.bone = {
      isSystemUser = true;
      group = "bone";
    };
    users.groups.bone = { };

    systemd.services.bone = {
      description = "bone mail server";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];

      serviceConfig = {
        ExecStart = start;
        SyslogIdentifier = "bone";
        Restart = "on-failure";

        User = "bone";
        Group = "bone";
        StateDirectory = "bone";
        StateDirectoryMode = "0700";
        CacheDirectory = "bone";
        CacheDirectoryMode = "0700";
        RuntimeDirectory = "bone";
        RuntimeDirectoryMode = "0700";
        LoadCredential = credentials;
        UMask = "0077";

        CapabilityBoundingSet = "";
        NoNewPrivileges = true;
        PrivateDevices = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectProc = "invisible";
        ProcSubset = "pid";
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        RemoveIPC = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [
          "@system-service"
          "~@privileged"
        ];
      };
    };
  };
}
