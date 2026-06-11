# Fedora RPM Spec for OxideMX
# Build: rpmbuild -ba oxidemx.spec

Name:           oxidemx
Version:        0.3.2
Release:        1%{?dist}
Summary:        Radial menu, DPI control and haptics for Logitech MX Master mice on Linux

License:        GPL-3.0-or-later
URL:            https://github.com/PooDoge/oxidemx
Source0:        %{url}/archive/v%{version}/%{name}-%{version}.tar.gz

BuildRequires:  rust
BuildRequires:  cargo
BuildRequires:  gtk4-devel
BuildRequires:  gtk4-layer-shell-devel
BuildRequires:  libadwaita-devel
BuildRequires:  dbus-devel
BuildRequires:  systemd-devel
BuildRequires:  libevdev-devel
BuildRequires:  hidapi-devel

Requires:       gtk4
Requires:       gtk4-layer-shell
Requires:       libadwaita
Requires:       dbus

Recommends:     ydotool

Obsoletes:      juhradial-mx < 0.3.3

%description
OxideMX brings a Logi Options+ inspired experience to Linux, written
entirely in Rust. Hold the gesture button on your MX Master mouse to open a
radial menu overlay, then move to select actions.

Features:
- Radial menu overlay with smooth animations (Rust + iced + layer-shell)
- Per-application profiles for context-aware actions
- Real-time battery status monitoring via HID++ protocol
- Visual DPI control with presets (400-8000 DPI)
- SmartShift scroll wheel and haptics configuration
- GNOME Shell battery indicator extension
- Native Wayland integration (GNOME, KDE Plasma, Hyprland)

OxideMX began as a fork of JuhRadial MX by Julian Hermstad (JuhLabs).

%prep
%autosetup -n %{name}-%{version}

%build
# Build the whole Rust workspace (daemon, overlay, settings, popup)
cargo build --release --workspace --exclude spike-iced --exclude iced_gtk_themer

%install
# Install binaries
install -Dm755 target/release/oxidemxd %{buildroot}%{_bindir}/oxidemxd
install -Dm755 target/release/oxidemx-overlay %{buildroot}%{_bindir}/oxidemx-overlay
install -Dm755 target/release/oxidemx-settings %{buildroot}%{_bindir}/oxidemx-settings
install -Dm755 target/release/oxidemx-popup %{buildroot}%{_bindir}/oxidemx-popup

# Install launcher script
install -Dm755 scripts/oxidemx.sh %{buildroot}%{_bindir}/oxidemx

# Install assets
install -dm755 %{buildroot}%{_datadir}/oxidemx/assets
cp -r assets/* %{buildroot}%{_datadir}/oxidemx/assets/

# Install desktop files
install -Dm644 packaging/oxidemx.desktop %{buildroot}%{_datadir}/applications/oxidemx.desktop
install -Dm644 packaging/org.oxidemx.settings.desktop %{buildroot}%{_datadir}/applications/org.oxidemx.settings.desktop

# Install icon
install -Dm644 assets/oxidemx.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/oxidemx.svg

# Install systemd user service
install -Dm644 packaging/systemd/oxidemx-daemon.service %{buildroot}%{_userunitdir}/oxidemx-daemon.service

# Install udev rules
install -Dm644 packaging/udev/99-oxidemx.rules %{buildroot}%{_udevrulesdir}/99-oxidemx.rules
install -Dm644 packaging/udev/70-oxidemx-haptic-pad.rules %{buildroot}%{_udevrulesdir}/70-oxidemx-haptic-pad.rules
install -Dm644 packaging/udev/60-ydotool-uinput.rules %{buildroot}%{_udevrulesdir}/60-ydotool-uinput.rules

%post
# Update icon cache
/usr/bin/gtk-update-icon-cache %{_datadir}/icons/hicolor &>/dev/null || :

# Reload udev rules
/usr/bin/udevadm control --reload-rules &>/dev/null || :
/usr/bin/udevadm trigger &>/dev/null || :

%postun
# Update icon cache
/usr/bin/gtk-update-icon-cache %{_datadir}/icons/hicolor &>/dev/null || :

%files
%license LICENSE
%doc README.md CONTRIBUTING.md
%{_bindir}/oxidemxd
%{_bindir}/oxidemx-overlay
%{_bindir}/oxidemx-settings
%{_bindir}/oxidemx-popup
%{_bindir}/oxidemx
%{_datadir}/oxidemx/
%{_datadir}/applications/oxidemx.desktop
%{_datadir}/applications/org.oxidemx.settings.desktop
%{_datadir}/icons/hicolor/scalable/apps/oxidemx.svg
%{_userunitdir}/oxidemx-daemon.service
%{_udevrulesdir}/99-oxidemx.rules
%{_udevrulesdir}/70-oxidemx-haptic-pad.rules
%{_udevrulesdir}/60-ydotool-uinput.rules
%changelog
* Thu Jun 11 2026 PooDoge <dev@chewyswap.dog> - 0.3.2-1
- Rebrand to OxideMX under new maintainership
- All-Rust stack: daemon, overlay, settings, popup (Python overlay removed)
- Credit to JuhLabs (Julian Hermstad) for the original JuhRadial MX
