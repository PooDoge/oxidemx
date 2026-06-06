# Troubleshooting

## GNOME/GDM login loop after installing the daemon ("A graphical session is already running!")

### Symptom

After enabling `oxidemx-daemon.service`, the graphical login starts bouncing:
you enter the **correct** password and are immediately thrown back to the user
picker, without ever reaching the desktop. A TTY/console login
(`Ctrl+Alt+F3`) still works fine. The problem **survives reboots**, and on
atomic distros (Bazzite, Silverblue, Kinoite) people often misdiagnose it as a
corrupt install and reinstall the OS — which is never necessary.

The journal shows, on every login attempt:

```
gnome-session-binary[...]: A graphical session is already running!
systemd-coredump[...]: Process ... (gnome-session-b) of user 1000 dumped core.
```

### Root cause

A misconfigured **systemd user unit** that force-activates
`graphical-session.target` outside of a real graphical session. The bug is the
combination of:

- `Wants=graphical-session.target` in `[Unit]`, **and**
- `WantedBy=default.target` in `[Install]`

This bites specifically when **user lingering is enabled**
(`loginctl enable-linger`, which is common when the user also runs rootless
Podman / Quadlet containers 24/7). With lingering:

1. The user's `systemd --user` manager starts at **boot** and runs `default.target`.
2. `default.target` pulls in this daemon (because `WantedBy=default.target`).
3. The daemon's `Wants=graphical-session.target` **force-activates that target**
   — with no real GUI session behind it.
4. Later, GDM tries to start GNOME → `gnome-session` checks the user manager,
   sees `graphical-session.target` already `active`, refuses, and core-dumps.
5. → login loop, persisting across reboots because lingering never restarts the
   user manager from a clean state.

A service that belongs to the graphical session must be **pulled up by** the
target, never **push the target up** itself.

### The fix (already applied to `oxidemx-daemon.service`)

```ini
[Unit]
After=graphical-session.target
PartOf=graphical-session.target
# (no Wants=graphical-session.target)

[Install]
WantedBy=graphical-session.target   # NOT default.target
```

If you installed an older copy, re-point the unit:

```bash
systemctl --user disable oxidemx-daemon.service   # removes default.target.wants symlink
# (replace the unit file with the corrected version)
systemctl --user daemon-reload
systemctl --user enable oxidemx-daemon.service    # creates graphical-session.target.wants symlink
```

### Emergency recovery (locked out right now)

From a TTY (`Ctrl+Alt+F3`), log in and clear the stuck target, then switch back
to the greeter (`Ctrl+Alt+F1`) and log in:

```bash
systemctl --user stop oxidemx-daemon.service
systemctl --user stop graphical-session.target
```

Then apply the unit fix above so it doesn't recur on the next boot.

### How to verify the wiring is correct

While **not** logged into a GUI (i.e. sitting at a TTY only), run:

```bash
systemctl --user is-active graphical-session.target      # must say: inactive
systemctl --user list-dependencies --reverse graphical-session.target
```

If the target is `active` with no GUI session, something is force-activating it.
The reverse-dependency list names the culprit unit. The daemon's symlink should
live in `~/.config/systemd/user/graphical-session.target.wants/`, **not**
`default.target.wants/`.
