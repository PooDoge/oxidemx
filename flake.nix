{
  description = "OxideMX - Radial menu and device manager for Logitech MX Master (and any mouse) on Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};

          gtkRuntimeLibs = with pkgs; [
            glib
            gtk4
            libadwaita
            gtk4-layer-shell
            (lib.getLib pango)
            gdk-pixbuf
            graphene
            gobject-introspection
            harfbuzz
          ];

          # Python environment with both PyQt6 (overlay) and PyGObject (settings)
          pythonEnv = pkgs.python3.withPackages (ps: with ps; [
            pyqt6
            pygobject3
          ]);

          # Rust daemon - handles evdev input and D-Bus signaling
          oxidemxd = pkgs.rustPlatform.buildRustPackage {
            pname = "oxidemxd";
            version = "0.3.2";

            src = ./.;
            cargoRoot = "daemon";
            buildAndTestSubdir = "daemon";

            cargoLock.lockFile = ./daemon/Cargo.lock;

            nativeBuildInputs = with pkgs; [ pkg-config ];
            buildInputs = with pkgs; [ dbus systemd ];

            meta = with pkgs.lib; {
              description = "OxideMX daemon - HID++/evdev listener and D-Bus bridge";
              license = licenses.gpl3Only;
              platforms = platforms.linux;
            };
          };

        in
        {
          inherit oxidemxd;

          default = pkgs.stdenv.mkDerivation {
            pname = "oxidemx";
            version = "0.3.2";
            src = ./.;

            nativeBuildInputs = with pkgs; [
              makeWrapper
              gobject-introspection
            ];

            buildInputs = gtkRuntimeLibs ++ (with pkgs; [
              qt6.qtbase
              qt6.qtsvg
            ]);

            dontBuild = true;
            dontWrapQtApps = true;
            dontWrapGApps = true;

            installPhase = ''
              runHook preInstall

              # Daemon binary
              install -Dm755 ${oxidemxd}/bin/oxidemxd $out/bin/oxidemxd

              # Python overlay and settings scripts
              mkdir -p $out/share/oxidemx
              cp overlay/*.py $out/share/oxidemx/

              # Flow module
              cp -r overlay/flow $out/share/oxidemx/

              # Locale files
              cp -r overlay/locales $out/share/oxidemx/

              # Assets - radial wheel images, device illustrations, AI icons
              mkdir -p $out/share/oxidemx/assets/radial-wheels
              cp assets/radial-wheels/*.png $out/share/oxidemx/assets/radial-wheels/
              if [ -d assets/devices ]; then
                mkdir -p $out/share/oxidemx/assets/devices
                cp assets/devices/*.png assets/devices/*.svg $out/share/oxidemx/assets/devices/ 2>/dev/null || true
              fi
              if [ -d assets/settings-generated ]; then
                mkdir -p $out/share/oxidemx/assets/settings-generated
                cp assets/settings-generated/*.png $out/share/oxidemx/assets/settings-generated/
              fi
              cp assets/ai-*.svg $out/share/oxidemx/assets/ 2>/dev/null || true

              # Symlink so ../assets/ relative paths from overlay scripts resolve correctly
              # (overlay_actions.py, oxidemx-overlay.py use os.path.dirname(__file__)/../assets/)
              ln -s $out/share/oxidemx/assets $out/share/assets

              # App icon
              install -Dm644 assets/oxidemx.svg $out/share/icons/hicolor/scalable/apps/oxidemx.svg
              install -Dm644 assets/oxidemx.svg $out/share/oxidemx/assets/oxidemx.svg

              # Desktop entries
              install -Dm644 packaging/oxidemx.desktop $out/share/applications/oxidemx.desktop
              install -Dm644 packaging/org.oxidemx.settings.desktop $out/share/applications/org.oxidemx.settings.desktop

              # Launcher scripts - write Nix-aware versions
              cat > $out/bin/oxidemx <<LAUNCHER
              #!/bin/bash
              # OxideMX Launcher (Nix)
              pkill -f "oxidemxd" 2>/dev/null
              pkill -f "oxidemx-overlay" 2>/dev/null
              sleep 0.3
              ${pythonEnv}/bin/python3 $out/share/oxidemx/oxidemx-overlay.py &
              OVERLAY_PID=\$!
              $out/bin/oxidemxd &
              DAEMON_PID=\$!
              echo "OxideMX started"
              echo "  Overlay PID: \$OVERLAY_PID"
              echo "  Daemon PID: \$DAEMON_PID"
              wait \$DAEMON_PID
              LAUNCHER
              chmod 755 $out/bin/oxidemx

              cat > $out/bin/oxidemx-settings <<LAUNCHER
              #!/bin/bash
              # OxideMX Settings (Nix)
              exec ${pythonEnv}/bin/python3 $out/share/oxidemx/settings_dashboard.py "\$@"
              LAUNCHER
              chmod 755 $out/bin/oxidemx-settings

              # systemd user service
              install -Dm644 packaging/systemd/oxidemx-daemon.service $out/lib/systemd/user/oxidemx-daemon.service
              substituteInPlace $out/lib/systemd/user/oxidemx-daemon.service \
                --replace-fail "/usr/local/bin/oxidemxd" "$out/bin/oxidemxd"

              # udev rules (for NixOS module)
              install -Dm644 packaging/udev/99-oxidemx.rules $out/etc/udev/rules.d/99-oxidemx.rules

              runHook postInstall
            '';

            # Wrap launcher scripts with GTK/Qt environment variables
            postFixup = let
              typelibPath = pkgs.lib.makeSearchPath "lib/girepository-1.0" gtkRuntimeLibs;
              qtPluginPath = pkgs.lib.makeSearchPath "lib/qt-6/plugins" [ pkgs.qt6.qtbase pkgs.qt6.qtsvg ];
            in ''
              wrapProgram $out/bin/oxidemx \
                --set GI_TYPELIB_PATH "${typelibPath}" \
                --set QT_PLUGIN_PATH "${qtPluginPath}" \
                --prefix PYTHONPATH : "$out/share/oxidemx"

              wrapProgram $out/bin/oxidemx-settings \
                --set GI_TYPELIB_PATH "${typelibPath}" \
                --prefix PYTHONPATH : "$out/share/oxidemx"
            '';

            meta = with pkgs.lib; {
              description = "Radial menu and device manager for Logitech MX Master (and any mouse) on Linux";
              homepage = "https://github.com/PooDoge/oxidemx";
              license = licenses.gpl3Only;
              platforms = platforms.linux;
              maintainers = [ ];
              mainProgram = "oxidemx";
            };
          };
        }
      );

      # NixOS module - enables systemd service + udev rules declaratively
      nixosModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.services.oxidemx;
        in
        {
          options.services.oxidemx = {
            enable = lib.mkEnableOption "OxideMX radial menu for Logitech MX Master";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.system}.default;
              defaultText = lib.literalExpression "oxidemx.packages.\${pkgs.system}.default";
              description = "The OxideMX package to use.";
            };
          };

          config = lib.mkIf cfg.enable {
            # Install the package system-wide
            environment.systemPackages = [ cfg.package ];

            # udev rules for non-root Logitech device access
            services.udev.extraRules =
              builtins.readFile (cfg.package + "/etc/udev/rules.d/99-oxidemx.rules");

            # Ensure 'input' group exists for device permissions
            users.groups.input = { };
          };
        };

      # Development shell for contributors
      devShells = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              rustc cargo pkg-config
              dbus systemd
              (python3.withPackages (ps: with ps; [ pyqt6 pygobject3 ]))
              gtk4 libadwaita gtk4-layer-shell graphene harfbuzz
              qt6.qtbase qt6.qtsvg
              gobject-introspection
            ];
          };
        }
      );
    };
}
