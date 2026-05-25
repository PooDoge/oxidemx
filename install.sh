#!/bin/bash
#
# JuhRadial MX Universal Installer
# https://github.com/JuhLabs/juhradial-mx
#
# Usage: curl -fsSL https://raw.githubusercontent.com/JuhLabs/juhradial-mx/master/install.sh | bash
#
# This script will:
# 1. Detect your Linux distribution
# 2. Install required dependencies
# 3. Clone and build JuhRadial MX
# 4. Install and enable the systemd service
#

set -e

# ── Colors & Formatting ──────────────────────────────────────────────
BOLD='\033[1m'
DIM='\033[2m'
RESET='\033[0m'
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
WHITE='\033[1;37m'
GRAY='\033[0;90m'

# ── Configuration ────────────────────────────────────────────────────
REPO_URL="https://github.com/JuhLabs/juhradial-mx"
INSTALL_DIR="/opt/juhradial-mx"
BIN_DIR="/usr/local/bin"
SYSTEMD_USER_DIR="$HOME/.config/systemd/user"
CONFIG_DIR="$HOME/.config/juhradial"
DISTRO_FAMILY=""
IS_ATOMIC=false
# Paths for shared data — overridden to /usr/local/share on atomic/immutable
# systems where /usr is read-only (Bazzite, Silverblue, Kinoite, Bluefin…).
SHARE_DIR="/usr/share/juhradial"
APP_DIR="/usr/share/applications"
ICON_DIR="/usr/share/icons/hicolor/scalable/apps"
TOTAL_STEPS=6
CURRENT_STEP=0
INSTALL_MODE="install"  # "install" or "upgrade"

# ── Output helpers ───────────────────────────────────────────────────
print_banner() {
    local BCYAN='\033[1;96m'
    echo ""
    echo -e "${BCYAN}"
    cat << 'BANNER'
    ___       _    ______          _ _       _  ___  ____  __
   |_  |     | |   | ___ \        | (_)     | | |  \/  \ \ / /
     | |_   _| |__ | |_/ /__ _  __| |_  __ _| | | .  . |\ V /
     | | | | | '_ \|    // _` |/ _` | |/ _` | | | |\/| |/   \
 /\__/ | |_| | | | | |\ | (_| | (_| | | (_| | | | |  | / /^\ \
 \____/ \__,_|_| |_\_| \_\__,_|\__,_|_|\__,_|_| \_|  |_\/   \/
BANNER
    echo -e "${RESET}"
    echo -e "                       ${CYAN}· Installer${RESET}"
    echo -e "             ${DIM}Radial menu for MX Master on Linux${RESET}"
    echo ""
}

step() {
    CURRENT_STEP=$((CURRENT_STEP + 1))
    echo ""
    echo -e "  ${CYAN}${BOLD}[$CURRENT_STEP/$TOTAL_STEPS]${RESET} ${BOLD}$1${RESET}"
    echo -e "  ${GRAY}$(printf '%.0s─' {1..48})${RESET}"
}

log_info() {
    echo -e "  ${BLUE}→${RESET} $1"
}

log_success() {
    echo -e "  ${GREEN}✓${RESET} $1"
}

log_warning() {
    echo -e "  ${YELLOW}!${RESET} ${YELLOW}$1${RESET}"
}

log_error() {
    echo -e "  ${RED}✗${RESET} ${RED}$1${RESET}"
}

log_dim() {
    echo -e "  ${GRAY}  $1${RESET}"
}

# ── Pre-flight checks ───────────────────────────────────────────────
check_root() {
    if [[ $EUID -eq 0 ]]; then
        log_error "Do not run this script as root. It will ask for sudo when needed."
        exit 1
    fi
}

detect_distro() {
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        DISTRO=$ID
        DISTRO_LIKE=$ID_LIKE
        DISTRO_PRETTY="${PRETTY_NAME:-$ID}"
        VERSION=$VERSION_ID
    elif [ -f /etc/lsb-release ]; then
        . /etc/lsb-release
        DISTRO=$DISTRIB_ID
        DISTRO_PRETTY="$DISTRIB_ID $DISTRIB_RELEASE"
        VERSION=$DISTRIB_RELEASE
    else
        DISTRO=$(uname -s)
        DISTRO_PRETTY="$DISTRO"
    fi

    resolve_distro_family
}

resolve_distro_family() {
    DISTRO_FAMILY="$DISTRO"

    case "$DISTRO" in
        arch|manjaro|endeavouros|garuda|artix|cachyos|arcolinux|archcraft)
            DISTRO_FAMILY="arch"
            ;;
        fedora|rhel|centos|rocky|almalinux|nobara|ultramarine|bazzite|silverblue|kinoite|bluefin|aurora|fedora-asahi-remix)
            DISTRO_FAMILY="fedora"
            ;;
        debian|ubuntu|linuxmint|pop|elementary|kali|zorin|tuxedo|neon|mx)
            DISTRO_FAMILY="debian"
            ;;
        opensuse*|suse*|sles)
            DISTRO_FAMILY="opensuse"
            ;;
    esac

    # Fallback: check ID_LIKE for derivatives we didn't list
    if [ "$DISTRO_FAMILY" = "$DISTRO" ] && [ -n "$DISTRO_LIKE" ]; then
        if [[ "$DISTRO_LIKE" == *"arch"* ]]; then
            DISTRO_FAMILY="arch"
        elif [[ "$DISTRO_LIKE" == *"fedora"* ]] || [[ "$DISTRO_LIKE" == *"rhel"* ]]; then
            DISTRO_FAMILY="fedora"
        elif [[ "$DISTRO_LIKE" == *"debian"* ]] || [[ "$DISTRO_LIKE" == *"ubuntu"* ]]; then
            DISTRO_FAMILY="debian"
        elif [[ "$DISTRO_LIKE" == *"suse"* ]]; then
            DISTRO_FAMILY="opensuse"
        fi
    fi

    # Final fallback: detect by package manager
    if [ "$DISTRO_FAMILY" = "$DISTRO" ] || [ -z "$DISTRO_FAMILY" ]; then
        if command -v pacman &> /dev/null; then
            DISTRO_FAMILY="arch"
        elif command -v apt-get &> /dev/null; then
            DISTRO_FAMILY="debian"
        elif command -v dnf &> /dev/null; then
            DISTRO_FAMILY="fedora"
        elif command -v zypper &> /dev/null; then
            DISTRO_FAMILY="opensuse"
        fi
    fi
}

check_wayland() {
    if [ "$XDG_SESSION_TYPE" = "wayland" ]; then
        WAYLAND_OK=true
    else
        WAYLAND_OK=false
    fi
}

check_atomic() {
    # Detect immutable / atomic Linux distros. The script supports
    # rpm-ostree (Fedora atomic family) natively; openSUSE MicroOS and
    # NixOS are flagged so the user gets a clear error instead of a
    # mysterious dnf/zypper failure further down.
    ATOMIC_FLAVOR=""

    # rpm-ostree: Bazzite, Silverblue, Kinoite, Bluefin, Aurora, etc.
    if [ -f /run/ostree-booted ]; then
        IS_ATOMIC=true
        ATOMIC_FLAVOR="rpm-ostree"
    elif command -v rpm-ostree &> /dev/null && rpm-ostree status &> /dev/null; then
        IS_ATOMIC=true
        ATOMIC_FLAVOR="rpm-ostree"
    # openSUSE MicroOS / Aeon / Kalpa — transactional-update wraps zypper.
    elif command -v transactional-update &> /dev/null; then
        IS_ATOMIC=true
        ATOMIC_FLAVOR="transactional-update"
    # NixOS — fully declarative, configuration.nix flow.
    elif [ -f /etc/NIXOS ] || [ -d /run/current-system/sw/bin ]; then
        IS_ATOMIC=true
        ATOMIC_FLAVOR="nixos"
    fi

    if [ "$IS_ATOMIC" = true ]; then
        # /usr is read-only on atomic images; /usr/local is the canonical
        # writable overlay path on every flavor we support (rpm-ostree
        # symlinks it to /var/usrlocal; MicroOS keeps it native-writable;
        # NixOS puts everything under /run/current-system).
        SHARE_DIR="/usr/local/share/juhradial"
        APP_DIR="/usr/local/share/applications"
        ICON_DIR="/usr/local/share/icons/hicolor/scalable/apps"
    fi
}

# Distrobox / Toolbox / Docker containers can't run rpm-ostree (the
# command may exist but talks to the host's read-only image store).
# Detect a container env and refuse — the user needs to exit to the host.
check_container_safety() {
    local in_container=""
    if [ -f /run/.containerenv ]; then
        in_container="podman/toolbox"
    elif [ -f /.dockerenv ]; then
        in_container="docker"
    elif [ -n "${container:-}" ]; then
        in_container="$container"
    elif [ -n "${TOOLBOX_NAME:-}" ] || [ -n "${DISTROBOX_HOST_HOME:-}" ]; then
        in_container="distrobox/toolbox"
    fi

    if [ -n "$in_container" ] && [ "$IS_ATOMIC" = true ]; then
        echo ""
        log_error "Detected: running inside a container ($in_container) on an atomic host."
        log_dim ""
        log_dim "  install.sh layers packages into the host with rpm-ostree (or"
        log_dim "  transactional-update), which can't reach the host from inside"
        log_dim "  a container. Exit to the host shell and re-run there:"
        log_dim ""
        log_dim "      # outside any toolbox / distrobox:"
        log_dim "      cd $(pwd)"
        log_dim "      ./install.sh"
        log_dim ""
        exit 1
    fi
}

check_desktop() {
    DESKTOP_TYPE="unknown"
    DESKTOP_LABEL="${XDG_CURRENT_DESKTOP:-unknown}"

    # Check compositors / desktop environments
    if [ -n "$HYPRLAND_INSTANCE_SIGNATURE" ]; then
        DESKTOP_TYPE="hyprland"
        DESKTOP_LABEL="Hyprland"
    elif [ -n "$SWAYSOCK" ] || [ "$XDG_CURRENT_DESKTOP" = "sway" ]; then
        DESKTOP_TYPE="sway"
        DESKTOP_LABEL="Sway"
    elif [[ "$XDG_CURRENT_DESKTOP" == *"KDE"* ]] || [ "$DESKTOP_SESSION" = "plasma" ] || [ "$DESKTOP_SESSION" = "plasmawayland" ]; then
        DESKTOP_TYPE="kde"
        DESKTOP_LABEL="KDE Plasma"
    elif [[ "$XDG_CURRENT_DESKTOP" == *"GNOME"* ]]; then
        DESKTOP_TYPE="gnome"
        DESKTOP_LABEL="GNOME"
    elif [[ "$XDG_CURRENT_DESKTOP" == *"COSMIC"* ]] || pgrep -x cosmic-comp &> /dev/null; then
        DESKTOP_TYPE="cosmic"
        DESKTOP_LABEL="COSMIC"
    elif [[ "$XDG_CURRENT_DESKTOP" == *"X-Cinnamon"* ]]; then
        DESKTOP_TYPE="cinnamon"
        DESKTOP_LABEL="Cinnamon"
    elif [[ "$XDG_CURRENT_DESKTOP" == *"XFCE"* ]]; then
        DESKTOP_TYPE="xfce"
        DESKTOP_LABEL="XFCE"
    elif [[ "$XDG_CURRENT_DESKTOP" == *"Budgie"* ]]; then
        DESKTOP_TYPE="budgie"
        DESKTOP_LABEL="Budgie"
    elif pgrep -x river &> /dev/null; then
        DESKTOP_TYPE="river"
        DESKTOP_LABEL="River"
    elif pgrep -x niri &> /dev/null; then
        DESKTOP_TYPE="niri"
        DESKTOP_LABEL="Niri"
    elif pgrep -x wayfire &> /dev/null; then
        DESKTOP_TYPE="wayfire"
        DESKTOP_LABEL="Wayfire"
    elif [ "$XDG_CURRENT_DESKTOP" = "i3" ] || pgrep -x i3 &> /dev/null; then
        DESKTOP_TYPE="i3"
        DESKTOP_LABEL="i3"
    fi
}

check_existing_install() {
    if [ -d "$INSTALL_DIR" ]; then
        INSTALL_MODE="upgrade"
        # Try to read current version from the installed copy
        if [ -f "$INSTALL_DIR/CHANGELOG.md" ]; then
            INSTALLED_VERSION=$(grep -m1 -oP '## \[?\K[0-9]+\.[0-9]+\.[0-9]+[^]\s]*' "$INSTALL_DIR/CHANGELOG.md" 2>/dev/null || echo "")
        fi
    fi
}

check_logitech_device() {
    LOGI_DEVICE_FOUND=false

    # Check for Logitech USB devices (vendor ID 046d)
    if command -v lsusb &> /dev/null; then
        if lsusb 2>/dev/null | grep -qi "046d:"; then
            LOGI_DEVICE_FOUND=true
        fi
    fi

    # Fallback: check HID subsystem (glob avoids the SC2010 ls|grep
    # antipattern and works with non-alphanumeric filenames if any
    # ever show up).
    if [ "$LOGI_DEVICE_FOUND" = false ] && [ -d /sys/bus/hid/devices/ ]; then
        for d in /sys/bus/hid/devices/*; do
            case "$(basename "$d")" in
                *046D:*|*046d:*) LOGI_DEVICE_FOUND=true; break ;;
            esac
        done
    fi
}

print_system_info() {
    echo ""
    echo -e "  ${BOLD}System${RESET}"
    echo -e "  ${GRAY}$(printf '%.0s─' {1..48})${RESET}"

    # Distro (use PRETTY_NAME for a nicer display)
    echo -e "  ${DIM}Distro${RESET}       ${WHITE}${DISTRO_PRETTY}${RESET} ${GRAY}(${DISTRO_FAMILY})${RESET}"

    # Kernel
    echo -e "  ${DIM}Kernel${RESET}       $(uname -r)"

    # Session
    if [ "$WAYLAND_OK" = true ]; then
        echo -e "  ${DIM}Session${RESET}      ${GREEN}Wayland${RESET}"
    else
        echo -e "  ${DIM}Session${RESET}      ${YELLOW}X11${RESET} ${GRAY}— some features may be limited${RESET}"
    fi

    # Desktop
    case "$DESKTOP_TYPE" in
        hyprland|kde|sway)
            echo -e "  ${DIM}Desktop${RESET}      ${GREEN}${DESKTOP_LABEL}${RESET}"
            ;;
        gnome|cosmic|cinnamon|xfce|budgie|river|niri|wayfire|i3)
            echo -e "  ${DIM}Desktop${RESET}      ${GREEN}${DESKTOP_LABEL}${RESET}"
            ;;
        *)
            echo -e "  ${DIM}Desktop${RESET}      ${YELLOW}${DESKTOP_LABEL}${RESET} ${GRAY}— works best on KDE/Hyprland${RESET}"
            ;;
    esac

    # Logitech device
    if [ "$LOGI_DEVICE_FOUND" = true ]; then
        echo -e "  ${DIM}Mouse${RESET}        ${GREEN}Logitech receiver detected${RESET}"
    else
        echo -e "  ${DIM}Mouse${RESET}        ${YELLOW}No Logitech receiver found${RESET} ${GRAY}— plug in to continue${RESET}"
    fi

    # Image type (atomic/immutable vs traditional). Wording deliberately
    # avoids "layering required" — on Bazzite/Silverblue with the Rust
    # workspace, the base image already has every runtime lib; we
    # install binaries to /usr/local/bin and the GNOME extensions to
    # ~/.local/share/ with zero rpm-ostree calls.
    if [ "$IS_ATOMIC" = true ]; then
        echo -e "  ${DIM}Image${RESET}        ${CYAN}Atomic${RESET} ${GRAY}(${ATOMIC_FLAVOR} — base image used directly, no layering by default)${RESET}"
    fi

    # Install mode
    if [ "$INSTALL_MODE" = "upgrade" ]; then
        local ver_info=""
        [ -n "$INSTALLED_VERSION" ] && ver_info=" ${GRAY}(${INSTALLED_VERSION})${RESET}"
        echo -e "  ${DIM}Mode${RESET}         ${CYAN}Upgrade${RESET}${ver_info}"
    else
        echo -e "  ${DIM}Mode${RESET}         ${WHITE}Fresh install${RESET}"
    fi

    echo ""
}

# ── Hyprland configuration ──────────────────────────────────────────
configure_hyprland() {
    if [ "$DESKTOP_TYPE" != "hyprland" ]; then
        return 0
    fi

    log_info "Configuring Hyprland window rules..."

    HYPR_CONFIG_DIR="$HOME/.config/hypr"
    RULES_CONTENT='
# ######## JuhRadial MX - Radial Menu Overlay ########
# These rules ensure the radial menu appears correctly as an overlay
windowrulev2 = float, title:^(JuhRadial MX)$
windowrulev2 = noblur, title:^(JuhRadial MX)$
windowrulev2 = noborder, title:^(JuhRadial MX)$
windowrulev2 = noshadow, title:^(JuhRadial MX)$
windowrulev2 = pin, title:^(JuhRadial MX)$
windowrulev2 = noanim, title:^(JuhRadial MX)$'

    # Check if rules already exist
    if grep -q "JuhRadial MX" "$HYPR_CONFIG_DIR"/*.conf "$HYPR_CONFIG_DIR"/**/*.conf 2>/dev/null; then
        log_dim "Hyprland rules already configured"
        return 0
    fi

    # Try dots-hyprland custom rules first (end-4/dots-hyprland structure)
    if [ -f "$HYPR_CONFIG_DIR/custom/rules.conf" ]; then
        echo "$RULES_CONTENT" >> "$HYPR_CONFIG_DIR/custom/rules.conf"
        log_success "Added rules to custom/rules.conf"
    # Try standard hyprland.conf
    elif [ -f "$HYPR_CONFIG_DIR/hyprland.conf" ]; then
        echo "$RULES_CONTENT" >> "$HYPR_CONFIG_DIR/hyprland.conf"
        log_success "Added rules to hyprland.conf"
    # Create a new rules file and source it
    else
        mkdir -p "$HYPR_CONFIG_DIR"
        echo "$RULES_CONTENT" > "$HYPR_CONFIG_DIR/juhradial-rules.conf"

        if [ -f "$HYPR_CONFIG_DIR/hyprland.conf" ]; then
            echo "source=juhradial-rules.conf" >> "$HYPR_CONFIG_DIR/hyprland.conf"
        fi
        log_success "Created juhradial-rules.conf"
    fi

    # Reload Hyprland config if possible
    if command -v hyprctl &> /dev/null; then
        hyprctl reload 2>/dev/null && log_dim "Hyprland config reloaded"
    fi
}

# ── Dependency installation ──────────────────────────────────────────

# Dependency strategy for rpm-ostree atomic distros (Bazzite, Silverblue,
# Kinoite, Bluefin, Aurora, Universal Blue family).
#
# DEFAULT BEHAVIOUR: do NOT layer packages. The base image already ships
# every runtime shared library juhradiald links against (libdbus, libudev,
# libsystemd, libevdev, libhidapi) plus ydotool, python3, gtk4, libadwaita,
# python3-gobject — which is everything the running stack needs. Build deps
# (rust + *-devel headers) live in the user's distrobox / toolbox; they
# never need to touch the host image.
#
# Per the Bazzite docs (https://docs.bazzite.gg/Installing_and_Managing_Software/rpm-ostree/),
# rpm-ostree layering is the LAST resort because it:
#   - delays every future image update by re-layering on each rebase
#   - makes rollback messy (you re-apply layered packages after rollback)
#   - is unnecessary when an alternative install path exists (Flatpak,
#     Homebrew, distrobox, AppImage)
#
# This function detects what's actually missing on the host. If nothing's
# missing → skip layering entirely (the Bazzite default). If something IS
# missing → suggest Flatpak/Homebrew/distrobox alternatives FIRST, with
# rpm-ostree as the explicit opt-in path via JUHRADIAL_USE_RPM_OSTREE=1.
install_deps_fedora_atomic() {
    log_info "Atomic image detected — probing host for runtime libraries"

    # Runtime probe: shared libraries the daemon (and overlay/popup
    # binaries) actually dlopen at runtime. ldconfig is the canonical way
    # to check this — works regardless of whether the lib came from the
    # base image, a layered package, or a Flatpak runtime extension.
    local missing_runtime_libs=()
    for lib in libdbus-1.so.3 libudev.so.1 libsystemd.so.0 libevdev.so.2 libhidapi-hidraw.so.0; do
        if ! ldconfig -p 2>/dev/null | grep -qF "$lib"; then
            missing_runtime_libs+=("$lib")
        fi
    done

    # Runtime commands the daemon shells out to. Single-element loop
    # today (ydotool only); kept as a loop so adding more commands later
    # is one line. shellcheck disable=SC2043
    local missing_runtime_cmds=()
    local runtime_cmds=(ydotool)
    for cmd in "${runtime_cmds[@]}"; do
        if ! command -v "$cmd" &> /dev/null; then
            missing_runtime_cmds+=("$cmd")
        fi
    done

    # Build toolchain — only needed if we're going to compile here.
    # build_project() will use any of three sources, in order:
    #   1. host cargo (traditional install)
    #   2. distrobox container with cargo (the atomic-Fedora default)
    #   3. pre-built binaries in target/release/ (skip-build path)
    # So we only flag "cargo missing" if NONE of those is satisfiable.
    local missing_build_tools=()
    if ! command -v cargo &> /dev/null \
       && ! find_distrobox_container &> /dev/null \
       && ! { [ -x target/release/juhradiald ] || [ -x daemon/target/release/juhradiald ]; }; then
        missing_build_tools+=("cargo (rust toolchain — host OR distrobox container)")
    fi

    local total_missing=$((${#missing_runtime_libs[@]} + ${#missing_runtime_cmds[@]} + ${#missing_build_tools[@]}))

    # Happy path: nothing missing. This is the Bazzite default.
    if [ "$total_missing" -eq 0 ]; then
        log_success "All runtime dependencies present — no layering needed"
        log_dim "  Runtime libs: libdbus, libudev, libsystemd, libevdev, libhidapi — all in base image"
        log_dim "  Runtime cmds: ydotool present"
        if have_all_release_binaries; then
            log_dim "  Build: skipped — release binaries already in target/release/"
        elif command -v cargo &> /dev/null; then
            log_dim "  Build: cargo on host PATH"
        elif c="$(find_distrobox_container)"; then
            log_dim "  Build: will use distrobox container '$c' (no host cargo needed)"
        fi
        return 0
    fi

    # Something is missing — report + offer alternatives in order of
    # increasing intrusiveness.
    echo ""
    log_warning "Host is missing ${total_missing} dependenc${total_missing:+ies}:"
    if [ ${#missing_runtime_libs[@]} -gt 0 ]; then
        log_dim "  Runtime libraries: ${missing_runtime_libs[*]}"
    fi
    if [ ${#missing_runtime_cmds[@]} -gt 0 ]; then
        log_dim "  Runtime commands:  ${missing_runtime_cmds[*]}"
    fi
    if [ ${#missing_build_tools[@]} -gt 0 ]; then
        log_dim "  Build tools:       ${missing_build_tools[*]}"
    fi

    echo ""
    log_info "Atomic-Fedora-friendly install paths (least → most intrusive):"
    echo ""
    if [ ${#missing_build_tools[@]} -gt 0 ]; then
        log_dim "  ${BOLD}Build tools${RESET} — these never need to touch the host image."
        log_dim "    Inside your distrobox / toolbox:"
        log_dim "        sudo dnf install -y rust cargo dbus-devel systemd-devel \\"
        log_dim "             libevdev-devel hidapi-devel git make"
        log_dim "    Then build via:  ./dev.sh build all"
        log_dim ""
    fi
    if [ ${#missing_runtime_cmds[@]} -gt 0 ]; then
        log_dim "  ${BOLD}Runtime commands${RESET} — try Homebrew first (no host-image changes):"
        log_dim "        brew install ${missing_runtime_cmds[*]}"
        log_dim ""
    fi
    if [ ${#missing_runtime_libs[@]} -gt 0 ]; then
        log_dim "  ${BOLD}Runtime libraries${RESET} are part of the rpm-ostree base image."
        log_dim "    These would only be missing on a non-standard image; if you really"
        log_dim "    need them, layer JUST the missing packages (not the full dev set)."
        log_dim ""
    fi
    log_dim "  ${BOLD}rpm-ostree layering${RESET} (last resort — delays image updates):"
    log_dim "      JUHRADIAL_USE_RPM_OSTREE=1 ./install.sh"
    echo ""

    if [ "${JUHRADIAL_USE_RPM_OSTREE:-}" = "1" ]; then
        log_warning "JUHRADIAL_USE_RPM_OSTREE=1 set — falling through to rpm-ostree layering"
        install_deps_fedora_atomic_rpm_ostree
    else
        log_error "Stopping. Install the missing dependencies via the suggested paths and re-run."
        log_dim "Or set JUHRADIAL_USE_RPM_OSTREE=1 to layer with rpm-ostree anyway."
        exit 1
    fi
}

# Opt-in legacy path. Only reachable when JUHRADIAL_USE_RPM_OSTREE=1.
# Layers the FULL dependency set (build + runtime + python overlay deps)
# via rpm-ostree, prompts for the required reboot.
install_deps_fedora_atomic_rpm_ostree() {
    local packages=(
        rust cargo
        python3 python3-pip
        python3-pyqt6 qt6-qtsvg
        python3-gobject gtk4 libadwaita
        gtk4-layer-shell
        python3-cryptography
        dbus-devel systemd-devel
        libevdev-devel hidapi-devel
        ydotool
        git make
    )

    log_info "Using ${BOLD}rpm-ostree${RESET} to layer ${#packages[@]} packages on the host image."
    log_dim "Docs: https://docs.bazzite.gg/Installing_and_Managing_Software/rpm-ostree/"

    local to_install=()
    for pkg in "${packages[@]}"; do
        if ! rpm -q "$pkg" &> /dev/null; then
            to_install+=("$pkg")
        fi
    done

    if [ ${#to_install[@]} -eq 0 ]; then
        log_success "All packages already layered"
        return 0
    fi

    echo ""
    log_info "Packages to layer (${#to_install[@]}):"
    log_dim "  ${to_install[*]}"
    echo ""
    log_warning "rpm-ostree layering requires a REBOOT to activate packages."
    echo ""

    echo -e "  ${BOLD}Proceed with rpm-ostree install?${RESET} ${DIM}[Y/n]${RESET} \c"
    read -n 1 -r < /dev/tty
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]] && [[ -n $REPLY ]]; then
        echo ""
        log_info "Cancelled. To layer manually:"
        log_dim "  sudo rpm-ostree install ${to_install[*]}"
        exit 0
    fi

    echo ""
    if ! sudo rpm-ostree install --idempotent "${to_install[@]}"; then
        log_error "rpm-ostree install failed"
        exit 1
    fi

    echo ""
    log_success "Packages layered into the next deployment"
    echo ""
    echo -e "  ${YELLOW}${BOLD}════════════════════════════════════════════════${RESET}"
    echo -e "  ${YELLOW}${BOLD}  REBOOT REQUIRED — then re-run this installer${RESET}"
    echo -e "  ${YELLOW}${BOLD}════════════════════════════════════════════════${RESET}"
    echo ""
    echo -e "  ${BOLD}Reboot now?${RESET} ${DIM}[y/N]${RESET} \c"
    read -n 1 -r < /dev/tty
    echo ""
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        log_info "Rebooting in 3 seconds… (Ctrl+C to cancel)"
        sleep 3
        sudo systemctl reboot
    fi
    exit 0
}

install_deps_fedora() {
    sudo dnf install -y \
        rust cargo \
        python3 python3-pip \
        python3-pyqt6 qt6-qtsvg \
        python3-gobject gtk4 libadwaita \
        gtk4-layer-shell \
        python3-cryptography \
        dbus-devel systemd-devel \
        libevdev-devel hidapi-devel \
        ydotool \
        git make
}

install_deps_arch() {
    sudo pacman -S --noconfirm --needed \
        rust \
        python python-pip \
        python-pyqt6 qt6-svg \
        python-gobject gtk4 libadwaita \
        gtk4-layer-shell \
        python-cryptography \
        dbus systemd-libs \
        libevdev hidapi \
        ydotool \
        git make base-devel
}

install_deps_debian() {
    sudo apt-get update
    sudo apt-get install -y \
        rustc cargo \
        python3 python3-pip python3-venv \
        python3-pyqt6 python3-pyqt6.qtsvg \
        python3-gi gir1.2-gtk-4.0 gir1.2-adw-1 \
        python3-cryptography \
        libdbus-1-dev libsystemd-dev \
        libevdev-dev libhidapi-dev \
        ydotool \
        git make build-essential

    if apt-cache show libgtk4-layer-shell0 &> /dev/null; then
        sudo apt-get install -y libgtk4-layer-shell0
    fi
}

install_deps_opensuse() {
    sudo zypper install -y \
        rust cargo \
        python3 python3-pip \
        python3-qt6 python3-qt6-svg \
        python3-gobject gtk4 libadwaita-devel \
        python3-cryptography \
        dbus-1-devel systemd-devel \
        libevdev-devel libhidapi-devel \
        ydotool \
        git make

    if ! sudo zypper install -y gtk4-layer-shell; then
        log_warning "gtk4-layer-shell not available on this repo"
    fi
}

install_dependencies() {
    step "Installing dependencies"

    if [ "$IS_ATOMIC" = true ]; then
        log_info "Package manager: ${BOLD}${ATOMIC_FLAVOR}${RESET} ${GRAY}(atomic ${DISTRO_FAMILY})${RESET}"
    else
        log_info "Package manager: ${BOLD}${DISTRO_FAMILY}${RESET}"
    fi

    # Atomic flavors that AREN'T rpm-ostree have very different package
    # flows. Bail with a clear message instead of falling through to
    # zypper/dnf and erroring opaquely.
    case "$ATOMIC_FLAVOR" in
        transactional-update)
            log_error "openSUSE MicroOS / Aeon / Kalpa detected (transactional-update)."
            log_dim "Automated layering for MicroOS isn't wired yet. Install manually:"
            log_dim "  sudo transactional-update pkg install rust cargo python3 python3-qt6 \\"
            log_dim "       python3-gobject gtk4 libadwaita-devel python3-cryptography \\"
            log_dim "       libevdev-devel libhidapi-devel ydotool git make"
            log_dim "Then reboot and re-run this installer with JUHRADIAL_SKIP_DEPS=1."
            exit 1
            ;;
        nixos)
            log_error "NixOS detected. Imperative package install doesn't fit NixOS's model."
            log_dim "Add a juhradial-mx derivation to your configuration.nix / flake instead."
            log_dim "Hand-roll a derivation from packaging/arch/PKGBUILD as a template."
            log_dim "Then re-run with JUHRADIAL_SKIP_DEPS=1 to run only build + install steps."
            exit 1
            ;;
    esac

    # JUHRADIAL_SKIP_DEPS=1 short-circuits the package install — useful
    # when the user manages deps via Nix / Guix / hand-built tooling.
    if [ "${JUHRADIAL_SKIP_DEPS:-}" = "1" ]; then
        log_warning "JUHRADIAL_SKIP_DEPS=1 set — assuming dependencies are already present"
        return 0
    fi

    case $DISTRO_FAMILY in
        fedora)
            if [ "$IS_ATOMIC" = true ]; then
                install_deps_fedora_atomic
            else
                install_deps_fedora
            fi
            ;;
        arch)
            install_deps_arch
            ;;
        debian)
            install_deps_debian
            ;;
        opensuse)
            install_deps_opensuse
            ;;
        *)
            log_error "Unsupported distribution: $DISTRO"
            log_dim "Please install dependencies manually. See CONTRIBUTING.md"
            log_dim "Or run with JUHRADIAL_SKIP_DEPS=1 to skip this step."
            exit 1
            ;;
    esac
    log_success "Dependencies ready"
}

# ── Repository ───────────────────────────────────────────────────────

# Detect "the script is being executed from inside an existing checkout"
# vs "user downloaded install.sh into /tmp and wants us to clone for them".
# Marker: a sibling Cargo.toml + .git/ at the script's directory.
script_dir_is_a_juhradial_clone() {
    local d
    d="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    [ -f "$d/Cargo.toml" ] && [ -d "$d/.git" ] && \
        grep -q "juhradial" "$d/Cargo.toml" 2>/dev/null
}

clone_repo() {
    step "Fetching source"

    # If the user ran ./install.sh from inside their own clone, prefer
    # that — don't clobber their working tree with a fresh remote
    # clone to /opt/. Honors $JUHRADIAL_INSTALL_DIR for explicit overrides.
    if [ -n "${JUHRADIAL_INSTALL_DIR:-}" ]; then
        INSTALL_DIR="$JUHRADIAL_INSTALL_DIR"
        log_info "Using \$JUHRADIAL_INSTALL_DIR override: $INSTALL_DIR"
    elif script_dir_is_a_juhradial_clone; then
        INSTALL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
        log_info "Running from inside a clone — using $INSTALL_DIR (no remote clone)"
    fi

    if [ -d "$INSTALL_DIR/.git" ]; then
        log_info "Updating existing checkout at $INSTALL_DIR..."
        # Only chown if we own it (otherwise it's the user's working tree
        # and reset --hard would destroy their uncommitted work).
        if [ -O "$INSTALL_DIR" ]; then
            git -C "$INSTALL_DIR" fetch origin || log_warning "git fetch failed (offline?) — using cached state"
            log_dim "Skipping reset --hard to preserve your working tree."
        else
            sudo chown -R "$USER:$USER" "$INSTALL_DIR"
            git -C "$INSTALL_DIR" fetch origin
            git -C "$INSTALL_DIR" reset --hard origin/master
            git -C "$INSTALL_DIR" clean -fd
        fi
    elif [ -d "$INSTALL_DIR" ]; then
        log_warning "$INSTALL_DIR exists but isn't a git checkout — skipping clone"
    else
        log_info "Cloning repository into $INSTALL_DIR..."
        sudo git clone "$REPO_URL" "$INSTALL_DIR"
        sudo chown -R "$USER:$USER" "$INSTALL_DIR"
    fi

    cd "$INSTALL_DIR"
    log_success "Source ready ($INSTALL_DIR)"
}

# ── Build ────────────────────────────────────────────────────────────

# Check whether all 4 release binaries are already on disk. Used to
# decide whether to skip the build step entirely.
have_all_release_binaries() {
    [ -x target/release/juhradiald ] || [ -x daemon/target/release/juhradiald ] || return 1
    # popup/overlay/settings are optional in the strict sense, but if
    # any of them ARE missing on an atomic system and we don't have
    # cargo, the user is going to hit pick_binary warnings later — so
    # require all-four-present here for the skip-build path.
    [ -x target/release/juhradial-popup ]      || [ -x popup-rs/target/release/juhradial-popup ]      || return 1
    [ -x target/release/juhradial-overlay-rs ] || [ -x overlay-rs/target/release/juhradial-overlay-rs ] || return 1
    [ -x target/release/juhradial-settings ]   || [ -x settings-rs/target/release/juhradial-settings ] || return 1
    return 0
}

# Find an available distrobox container that can run cargo. Honors
# $JUHRADIAL_DISTROBOX, then probes common names. Returns the name on
# stdout + exit 0, or exit 1 if no usable container found.
find_distrobox_container() {
    if ! command -v distrobox &> /dev/null; then
        return 1
    fi
    local candidates=("${JUHRADIAL_DISTROBOX:-}" claude_development juhradial-dev dev rust fedora-toolbox)
    for name in "${candidates[@]}"; do
        [ -z "$name" ] && continue
        if distrobox list 2>/dev/null | awk '{print $3}' | grep -qx "$name"; then
            echo "$name"
            return 0
        fi
    done
    return 1
}

build_project() {
    step "Building Rust workspace"
    cd "$INSTALL_DIR"

    # Escape hatch: user already has pre-built binaries (e.g., copied from
    # a release tarball or a CI artifact). Skip the build entirely.
    if [ "${JUHRADIAL_SKIP_BUILD:-}" = "1" ]; then
        if have_all_release_binaries; then
            log_success "JUHRADIAL_SKIP_BUILD=1 — using existing release binaries"
            return 0
        else
            log_error "JUHRADIAL_SKIP_BUILD=1 but not all release binaries present"
            log_dim "  Need: target/release/{juhradiald,juhradial-popup,juhradial-overlay-rs,juhradial-settings}"
            log_dim "  Or per-crate: <crate>/target/release/<bin>"
            exit 1
        fi
    fi

    # Build strategy: prefer host cargo if available; fall back to
    # distrobox-managed cargo (the atomic-Fedora default); fall back to
    # "binaries already present, skip" if all four exist.
    local build_mode=""
    if command -v cargo &> /dev/null; then
        build_mode="host"
    elif container="$(find_distrobox_container)"; then
        build_mode="distrobox:$container"
    elif have_all_release_binaries; then
        build_mode="skip"
    fi

    case "$build_mode" in
        host)
            log_info "Compiling on host with cargo: $(command -v cargo)"
            do_workspace_build_host
            ;;
        distrobox:*)
            local container="${build_mode#distrobox:}"
            log_info "Compiling inside distrobox container: ${BOLD}${container}${RESET}"
            log_dim "  (cargo not on host PATH — this avoids layering rust via rpm-ostree)"
            distrobox enter "$container" -- bash -c \
                "cd '$INSTALL_DIR' && cargo build --release \
                 -p juhradiald -p juhradial-overlay-rs \
                 -p juhradial-popup-rs -p juhradial-settings-rs" || {
                log_error "distrobox build failed."
                log_dim "  If '$container' is missing the rust/devel deps, install them inside it:"
                log_dim "    distrobox enter $container -- sudo dnf install -y rust cargo \\"
                log_dim "         dbus-devel systemd-devel libevdev-devel hidapi-devel git"
                exit 1
            }
            ;;
        skip)
            log_warning "cargo not on host AND no distrobox found — but release binaries"
            log_warning "are already on disk. Skipping build."
            ;;
        *)
            log_error "No way to build: cargo not on host PATH, no distrobox container available."
            log_dim ""
            log_dim "  Options:"
            log_dim "    1. Set up a distrobox with rust:"
            log_dim "         distrobox-create --name juhradial-dev --image registry.fedoraproject.org/fedora-toolbox:latest"
            log_dim "         distrobox enter juhradial-dev -- sudo dnf install -y rust cargo \\"
            log_dim "              dbus-devel systemd-devel libevdev-devel hidapi-devel git"
            log_dim "         ./install.sh"
            log_dim ""
            log_dim "    2. Build elsewhere and copy target/release/* in, then re-run with:"
            log_dim "         JUHRADIAL_SKIP_BUILD=1 ./install.sh"
            log_dim ""
            log_dim "    3. (Last resort) layer rust via rpm-ostree:"
            log_dim "         JUHRADIAL_USE_RPM_OSTREE=1 ./install.sh"
            exit 1
            ;;
    esac

    log_success "Build complete"
}

# Host-cargo build, factored out for clarity. Same workspace-aware
# fallback as before.
do_workspace_build_host() {
    if [ -f Cargo.toml ] && grep -q '^\[workspace\]' Cargo.toml; then
        cargo build --release \
            -p juhradiald \
            -p juhradial-overlay-rs \
            -p juhradial-popup-rs \
            -p juhradial-settings-rs
    else
        log_warning "Workspace Cargo.toml not detected; building per-crate"
        ( cd daemon && cargo build --release )
        [ -d popup-rs ]    && ( cd popup-rs    && cargo build --release )
        [ -d overlay-rs ]  && ( cd overlay-rs  && cargo build --release )
        [ -d settings-rs ] && ( cd settings-rs && cargo build --release )
    fi
}

# ── Install files ────────────────────────────────────────────────────
install_files() {
    step "Installing files"

    # Cargo workspace puts binaries at target/release/<bin>; per-crate builds
    # put them at <crate>/target/release/<bin>. Helper picks the first
    # existing path so the install works in both modes. First arg is the
    # human-readable binary name (used in error messages); remaining args
    # are candidate paths to probe in order.
    pick_binary() {
        local name="$1"; shift
        for candidate in "$@"; do
            if [ -x "$candidate" ]; then
                echo "$candidate"
                return 0
            fi
        done
        log_warning "$name binary not found in any of: $*"
        return 1
    }

    # Install daemon binary (required)
    daemon_bin="$(pick_binary juhradiald target/release/juhradiald daemon/target/release/juhradiald)" || {
        log_error "juhradiald not built — run ./dev.sh build daemon or cargo build --release -p juhradiald"
        exit 1
    }
    sudo install -Dm755 "$daemon_bin" "$BIN_DIR/juhradiald"
    log_success "Daemon binary ($daemon_bin)"

    # Install indicator popup binary (popup-rs) — optional, skip on partial builds
    if popup_bin="$(pick_binary juhradial-popup target/release/juhradial-popup popup-rs/target/release/juhradial-popup)"; then
        sudo install -Dm755 "$popup_bin" "$BIN_DIR/juhradial-popup"
        log_success "Indicator popup binary"
    fi

    # Install overlay binary (overlay-rs) — optional
    if overlay_bin="$(pick_binary juhradial-overlay-rs target/release/juhradial-overlay-rs overlay-rs/target/release/juhradial-overlay-rs)"; then
        sudo install -Dm755 "$overlay_bin" "$BIN_DIR/juhradial-overlay-rs"
        log_success "Overlay binary"
    fi

    # Install settings binary (settings-rs) — optional
    if settings_bin="$(pick_binary juhradial-settings target/release/juhradial-settings settings-rs/target/release/juhradial-settings)"; then
        sudo install -Dm755 "$settings_bin" "$BIN_DIR/juhradial-settings"
        log_success "Settings binary"
    fi

    # On atomic images /usr is read-only, so shared data goes to /usr/local/share
    # (SHARE_DIR / APP_DIR / ICON_DIR are set by check_atomic()).
    if [ "$IS_ATOMIC" = true ]; then
        log_dim "Installing shared files to ${SHARE_DIR} (atomic image)"
    fi

    # Install overlay scripts
    sudo mkdir -p "$SHARE_DIR"
    sudo cp -r overlay/*.py "$SHARE_DIR/"
    log_success "Overlay scripts"

    # Install flow module (subdirectory)
    sudo rm -rf "$SHARE_DIR/flow"
    sudo cp -r overlay/flow "$SHARE_DIR/flow"
    log_success "Flow module"

    # Install locale files
    if [ -d overlay/locales ]; then
        sudo mkdir -p "$SHARE_DIR/locales"
        sudo cp -r overlay/locales/* "$SHARE_DIR/locales/"
    fi

    # Install 3D radial wheel images
    sudo mkdir -p "$SHARE_DIR/assets/radial-wheels"
    sudo cp -r assets/radial-wheels/*.png "$SHARE_DIR/assets/radial-wheels/"
    log_success "Theme assets"

    # Install device images (mouse illustrations for settings)
    if [ -d assets/devices ]; then
        sudo mkdir -p "$SHARE_DIR/assets/devices"
        sudo cp assets/devices/*.png assets/devices/*.svg "$SHARE_DIR/assets/devices/" 2>/dev/null || true
    fi

    # Install AI assistant icons
    sudo cp assets/ai-*.svg "$SHARE_DIR/assets/" 2>/dev/null || true

    # Install OS icons (used by Flow easy-switch and device display)
    sudo cp assets/os-*.svg "$SHARE_DIR/assets/" 2>/dev/null || true

    # Install Flow indicator image
    sudo cp assets/flow-indicator.png "$SHARE_DIR/assets/" 2>/dev/null || true

    # Install generic mouse icon
    sudo cp assets/genericmouse.png "$SHARE_DIR/assets/" 2>/dev/null || true

    # Install sidebar navigation icons
    sudo cp assets/nav-*.png "$SHARE_DIR/assets/" 2>/dev/null || true

    # Install generated settings artwork
    if [ -d assets/settings-generated ]; then
        sudo mkdir -p "$SHARE_DIR/assets/settings-generated"
        sudo cp assets/settings-generated/control-ring.png "$SHARE_DIR/assets/settings-generated/" 2>/dev/null || true
        sudo cp assets/settings-generated/easyswitch.png "$SHARE_DIR/assets/settings-generated/" 2>/dev/null || true
        sudo cp assets/settings-generated/haptics.png "$SHARE_DIR/assets/settings-generated/" 2>/dev/null || true
    fi

    # Install launcher scripts. NOTE: the Rust juhradial-settings binary
    # installed above (when present) wins on $PATH; the shell launcher
    # remains as a fallback for installs that built only the Python
    # overlay tree.
    sudo install -Dm755 scripts/juhradial-mx.sh "$BIN_DIR/juhradial-mx"
    if [ -z "${settings_bin:-}" ]; then
        sudo install -Dm755 scripts/juhradial-settings.sh "$BIN_DIR/juhradial-settings"
    fi

    # Install desktop files
    sudo install -Dm644 packaging/juhradial-mx.desktop "$APP_DIR/juhradial-mx.desktop"
    sudo install -Dm644 packaging/org.juhradial.settings.desktop "$APP_DIR/org.juhradial.settings.desktop"

    # Install icons
    sudo install -Dm644 assets/juhradial-mx.svg "$ICON_DIR/juhradial-mx.svg"
    log_success "Desktop integration"

    # Install systemd service
    mkdir -p "$SYSTEMD_USER_DIR"
    cp packaging/systemd/juhradialmx-daemon.service "$SYSTEMD_USER_DIR/"

    # Autostart the overlay at login. The systemd service runs the daemon,
    # but the overlay is a per-session GUI process that needs the user's
    # graphical/D-Bus session — so we install it as an XDG autostart entry
    # rather than a second systemd unit. Without this, the daemon captures
    # button presses but there's no overlay listening to draw the menu, so
    # users have to manually launch the app every login.
    AUTOSTART_DIR="$HOME/.config/autostart"
    mkdir -p "$AUTOSTART_DIR"
    cat > "$AUTOSTART_DIR/juhradial-overlay.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=JuhRadial MX Overlay
Comment=Radial menu overlay for Logitech MX Master mice
Exec=python3 $SHARE_DIR/juhradial-overlay.py
Icon=juhradial-mx
Terminal=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
EOF
    log_success "Overlay autostart configured"

    # Install/update udev rules (always update to fix security issues in older versions)
    if [ -f packaging/udev/99-juhradialmx.rules ]; then
        sudo install -Dm644 packaging/udev/99-juhradialmx.rules /etc/udev/rules.d/
        [ -f /etc/udev/rules.d/99-logitech-hidpp.rules ] && sudo rm -f /etc/udev/rules.d/99-logitech-hidpp.rules
        sudo udevadm control --reload-rules
        sudo udevadm trigger
        log_success "udev rules"

        # Ensure 'input' group exists and user is a member (required for hidraw/evdev access)
        if ! getent group input &> /dev/null; then
            sudo groupadd input
            log_info "Created 'input' group"
        fi
        if ! id -nG "$USER" | grep -qw input; then
            sudo usermod -aG input "$USER"
            log_warning "Added $USER to 'input' group - log out and back in for device access"
        fi
    fi

    # Create config directory
    mkdir -p "$CONFIG_DIR"
}

# ── Systemd service ─────────────────────────────────────────────────
enable_service() {
    step "Enabling service"

    if ! command -v systemctl &> /dev/null; then
        log_warning "systemctl not available — skipping"
        return 0
    fi

    systemctl --user daemon-reload || log_warning "Failed to reload user systemd"
    systemctl --user enable juhradialmx-daemon || log_warning "Failed to enable service"

    # Restart on upgrade, start on fresh install
    if [ "$INSTALL_MODE" = "upgrade" ]; then
        systemctl --user restart juhradialmx-daemon || log_warning "Failed to restart service"
        log_success "Service restarted"
    else
        systemctl --user start juhradialmx-daemon || log_warning "Failed to start service"
        log_success "Service enabled and started"
    fi
}

# ── GNOME extensions ─────────────────────────────────────────────────

# Compile TypeScript sources for both extensions into .js (the runtime
# format Shell loads). Idempotent — exits early if no .ts source exists,
# bootstraps node_modules via npm install on first run.
compile_gnome_extensions_ts() {
    local ext_root="$INSTALL_DIR/gnome-extension"

    # No TS source = nothing to compile. The extensions can still be
    # installed if they have pre-built .js (older snapshots / vendored).
    if ! find "$ext_root" -maxdepth 3 -name '*.ts' -print -quit 2>/dev/null | grep -q .; then
        return 0
    fi

    if ! command -v npx &> /dev/null; then
        log_warning "npx not on PATH — skipping TypeScript compile."
        log_dim "Install Node.js (inside a distrobox on atomic systems) and re-run for full TS support."
        log_dim "  distrobox enter <name> -- bash -c 'cd $ext_root && npm install && npm run build'"
        return 0
    fi

    if [ ! -d "$ext_root/node_modules" ]; then
        log_info "Bootstrapping TypeScript toolchain (npm install)..."
        ( cd "$ext_root" && npm install --no-audit --no-fund ) || {
            log_warning "npm install failed — TS sources won't be compiled."
            return 0
        }
    fi
    log_info "Compiling TypeScript extensions..."
    ( cd "$ext_root" && npx tsc -p tsconfig.json ) || {
        log_warning "tsc failed — falling back to whatever .js is already present."
        return 0
    }
    log_success "TypeScript extensions compiled"
}

# Install one GNOME shell extension by UUID. Handles: copy source files,
# compile gschema (if a schemas/ dir exists), enable, report state. The
# caller controls whether the extension is "primary" (cursor helper) or
# "indicator" — both are installed identically; the only per-extension
# logic is whether glib-compile-schemas runs against schemas/.
install_gnome_extension() {
    local EXT_UUID="$1"
    local EXT_SRC="$INSTALL_DIR/gnome-extension/$EXT_UUID"
    local EXT_DEST="$HOME/.local/share/gnome-shell/extensions/$EXT_UUID"

    if [ ! -d "$EXT_SRC" ]; then
        log_warning "$EXT_UUID source not found at $EXT_SRC — skipping"
        return 0
    fi

    mkdir -p "$EXT_DEST"
    # Copy every payload artifact: metadata, compiled .js, lib/, schemas/,
    # icons/, stylesheet. Use rsync-style cp -a so timestamps + perms
    # carry over (-r alone trips on some src layouts).
    cp -af "$EXT_SRC/metadata.json" "$EXT_DEST/"
    # Compiled .js (extension entry + prefs + lib/*); lib/ may not exist
    # on the cursor extension, which is fine.
    cp -af "$EXT_SRC"/*.js "$EXT_DEST/" 2>/dev/null || true
    [ -d "$EXT_SRC/lib" ]      && cp -af "$EXT_SRC/lib"      "$EXT_DEST/"
    [ -d "$EXT_SRC/icons" ]    && cp -af "$EXT_SRC/icons"    "$EXT_DEST/"
    [ -f "$EXT_SRC/stylesheet.css" ] && cp -af "$EXT_SRC/stylesheet.css" "$EXT_DEST/"

    # GSettings schema. Compile in-place so Shell + popup-rs find the
    # compiled gschemas.compiled at the extension's own schemas/ dir.
    if [ -d "$EXT_SRC/schemas" ]; then
        mkdir -p "$EXT_DEST/schemas"
        cp -af "$EXT_SRC/schemas"/*.xml "$EXT_DEST/schemas/" 2>/dev/null || true
        if command -v glib-compile-schemas &> /dev/null; then
            glib-compile-schemas "$EXT_DEST/schemas/" 2>/dev/null \
                && log_dim "  ↳ gschema compiled" \
                || log_warning "  ↳ glib-compile-schemas failed for $EXT_UUID"
        else
            log_warning "  ↳ glib-compile-schemas missing — install glib2-devel"
        fi
    fi

    log_success "$EXT_UUID installed"

    # Enable + report state. Hot-reload works for the cursor extension's
    # D-Bus surface; the indicator's PanelMenu.Button needs a real
    # Shell restart (full re-login on Wayland — there's no Alt-F2 r).
    if command -v gnome-extensions &> /dev/null; then
        if gnome-extensions enable "$EXT_UUID" 2>/dev/null; then
            log_dim "  ↳ enabled"
        else
            log_dim "  ↳ enable deferred (Shell not running, or needs re-login)"
        fi
    fi
}

configure_gnome() {
    if [ "$DESKTOP_TYPE" != "gnome" ]; then
        return 0
    fi

    log_info "Installing GNOME Shell extensions..."

    compile_gnome_extensions_ts
    install_gnome_extension "juhradial-cursor@dev.juhlabs.com"
    install_gnome_extension "juhradial-indicator@dev.juhlabs.com"

    # Per-extension state check — report the WORSE of the two states so
    # the user knows whether they need to log out.
    local needs_restart=false
    for uuid in "juhradial-cursor@dev.juhlabs.com" "juhradial-indicator@dev.juhlabs.com"; do
        local ext_state
        ext_state=$(gnome-extensions info "$uuid" 2>/dev/null | grep -oP '(?<=State: )\S+' || true)
        if [ "$ext_state" != "ACTIVE" ] && [ "$ext_state" != "ENABLED" ]; then
            needs_restart=true
        fi
    done

    if [ "$needs_restart" = true ]; then
        log_warning "Log out and back in for the extensions to load (Wayland requires session restart)."
    else
        log_success "Both extensions active — no restart needed."
    fi
}

# ── Desktop environment ─────────────────────────────────────────────
configure_desktop() {
    step "Desktop integration"

    configure_hyprland
    configure_gnome

    if [ "$DESKTOP_TYPE" = "hyprland" ]; then
        log_success "Hyprland window rules configured"
    elif [ "$DESKTOP_TYPE" = "gnome" ]; then
        log_success "GNOME cursor helper extension installed"
    elif [ "$DESKTOP_TYPE" = "cosmic" ]; then
        log_success "COSMIC detected — cursor position via XWayland"
    elif [ "$DESKTOP_TYPE" = "kde" ] || [ "$DESKTOP_TYPE" = "sway" ]; then
        log_success "No extra configuration needed for ${DESKTOP_LABEL}"
    else
        log_dim "No desktop-specific configuration applied"
    fi
}

# ── Completion ───────────────────────────────────────────────────────
print_success() {
    # Read new version from the freshly fetched source
    local new_version=""
    if [ -f "$INSTALL_DIR/CHANGELOG.md" ]; then
        new_version=$(grep -m1 -oP '## \[?\K[0-9]+\.[0-9]+\.[0-9]+[^]\s]*' "$INSTALL_DIR/CHANGELOG.md" 2>/dev/null || echo "")
    fi

    local version_display=""
    if [ -n "$new_version" ]; then
        if [ "$INSTALL_MODE" = "upgrade" ] && [ -n "$INSTALLED_VERSION" ] && [ "$INSTALLED_VERSION" != "$new_version" ]; then
            version_display=" ${GRAY}${INSTALLED_VERSION} → ${RESET}${WHITE}${new_version}${RESET}"
        else
            version_display=" ${WHITE}${new_version}${RESET}"
        fi
    fi

    echo ""
    echo -e "  ${GREEN}${BOLD}╭──────────────────────────────────────────╮${RESET}"
    echo -e "  ${GREEN}${BOLD}│                                          │${RESET}"
    if [ "$INSTALL_MODE" = "upgrade" ]; then
        echo -e "  ${GREEN}${BOLD}│   ✓  JuhRadial MX updated!               │${RESET}"
    else
        echo -e "  ${GREEN}${BOLD}│   ✓  JuhRadial MX installed!             │${RESET}"
    fi
    echo -e "  ${GREEN}${BOLD}│                                          │${RESET}"
    echo -e "  ${GREEN}${BOLD}╰──────────────────────────────────────────╯${RESET}"
    [ -n "$version_display" ] && echo -e "  ${DIM}Version${RESET}${version_display}"
    echo ""
    echo -e "  ${BOLD}Getting started${RESET}"
    echo -e "  ${GRAY}$(printf '%.0s─' {1..48})${RESET}"
    echo -e "  ${WHITE}1.${RESET}  Run ${CYAN}juhradial-mx${RESET} or find it in your app menu"
    echo -e "  ${WHITE}2.${RESET}  Hold the ${BOLD}thumb button${RESET} on your MX Master"
    echo -e "  ${WHITE}3.${RESET}  Right-click the tray icon for ${BOLD}Settings${RESET}"
    echo ""
    echo -e "  ${BOLD}Useful commands${RESET}"
    echo -e "  ${GRAY}$(printf '%.0s─' {1..48})${RESET}"
    echo -e "  ${DIM}Status${RESET}   systemctl --user status juhradialmx-daemon"
    echo -e "  ${DIM}Logs${RESET}     journalctl --user -u juhradialmx-daemon -f"
    echo ""
    echo -e "  ${GRAY}github.com/JuhLabs/juhradial-mx${RESET}"
    echo ""
    echo -e "  ${CYAN}Enjoying JuhRadial MX?${RESET} Leave a ${YELLOW}★${RESET} on GitHub!"
    echo -e "  ${DIM}Found a bug? Open an issue - we'd love to hear from you.${RESET}"
    echo ""

    # First-time GNOME installers need a session restart for the extension
    if [ "$INSTALL_MODE" = "install" ] && [ "$DESKTOP_TYPE" = "gnome" ]; then
        echo -e "  ${YELLOW}${BOLD}════════════════════════════════════════════════${RESET}"
        echo -e "  ${YELLOW}${BOLD}  FIRST TIME INSTALLERS: LOG OUT AND BACK IN${RESET}"
        echo -e "  ${YELLOW}${BOLD}  (or restart) to activate the GNOME extension${RESET}"
        echo -e "  ${YELLOW}${BOLD}════════════════════════════════════════════════${RESET}"
        echo ""
    fi
}

# ── Main ─────────────────────────────────────────────────────────────
main() {
    print_banner
    check_root
    detect_distro
    check_atomic
    check_container_safety
    check_wayland
    check_desktop
    check_existing_install
    check_logitech_device
    print_system_info

    if [ "$INSTALL_MODE" = "upgrade" ]; then
        echo -e "  ${BOLD}Proceed with upgrade?${RESET} ${DIM}[Y/n]${RESET} \c"
    else
        echo -e "  ${BOLD}Proceed with installation?${RESET} ${DIM}[Y/n]${RESET} \c"
    fi
    read -n 1 -r < /dev/tty
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]] && [[ ! -z $REPLY ]]; then
        echo ""
        log_info "Cancelled."
        exit 0
    fi

    install_dependencies
    clone_repo
    build_project
    install_files
    configure_desktop
    enable_service
    print_success
}

main "$@"
