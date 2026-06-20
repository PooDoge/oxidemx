# Idea — Install GUI front-end for install.sh (Zenity/YAD) — user request 2026-06-20

Wrap `install.sh` in a graphical installer so a non-terminal user can install OxideMX:
- **Detect + show the OS** (distro, atomic vs traditional, package manager) and the available
  **install methods** for that OS.
- Let the user **select options** (which components: daemon / overlay / agentd / GNOME ext /
  udev rules / flows; build-from-source vs prebuilt; install prefix).
- Show **live status / progress** of each step (building, installing, enabling services).
- **Obtain sudo/root** via the GUI (pkexec — the polkit dialog), not a terminal prompt.

## Toolkit choice (decide in brainstorm)
- **Zenity** — ships with GNOME, simple dialogs + `--progress`. Limited layout.
- **YAD** (Yet Another Dialog) — Zenity fork, far richer (forms, multi-progress, notebooks,
  checklists) — likely the best fit for "select options + per-step status". Not always preinstalled.
- **kdialog** (KDE), or a tiny GTK/relm4 binary we ship. 
Recommendation to evaluate: **YAD** for the rich option/checklist + multi-step progress UI, with a
**Zenity fallback** (detect which is present; degrade gracefully) since YAD may not be installed.

## Must-haves (from the request)
- Detected OS + applicable install methods shown up front.
- Option selection (components / mode).
- What's happening + per-step status + final result.
- pkexec-based privilege escalation (one auth, not repeated).

## Ties into current work
This slice should ALSO fold agentd into `install.sh` properly: install the `oxidemx-agentd`
binary + the **systemd user unit** (`oxidemx-agentd.service`, created live 2026-06-20 in
~/.config/systemd/user/, Restart=on-failure, WantedBy=graphical-session.target) + enable it —
mirroring how the daemon unit is generated. Right now agentd is installed + unit-enabled by hand;
install.sh doesn't know about it. The GUI is the natural place to surface "Agent daemon" as a
selectable component.

## Status
Captured; its own brainstorm → spec → plan slice. Sequence after the overlay-open fix (so the
freshly-installed stack actually opens) OR per the user's preference.
