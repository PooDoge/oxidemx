# Manual test recipe — radial pages, AI chat redesign, center-puck handoff

Build + install the dev binary (`cargo build --release -p oxidemx-overlay`,
then your usual `dev.sh` / `local-test-install.sh` flow). A fresh config is
needed to see the new built-in pages — either move `~/.config/oxidemx/config.json`
aside, or copy the `pages` array from `oxidemx-shared/default-config.json`
into yours.

## Contract 1 — center-puck handoff

1. Open the menu (gesture tap → toggle mode). Scroll over the centre puck
   to cycle pages; land on **AI Assistant** (last dot).
2. ✔ The disc morphs to the chat; the centre puck **flies to the header's
   left slot**, shrinking to 32 px, ring lit in accent with an outer glow,
   page dots still live.
3. **Wheel anywhere** over the chat (don't click): ✔ pages keep cycling —
   scrolling away reverses the morph onto the neighbouring page. No
   keyboard caret appears in the input while armed.
4. Land on AI again, keep the mouse perfectly still: ✔ nothing activates.
5. Move the cursor deliberately out of where the disc centre was
   (≥ ~8 px travel): ✔ puck ring dims to the muted stroke, the input gains
   the caret (focus). Alternatively click anywhere in the chat (not on the
   puck): same activation. Clicking the puck itself must NOT activate.
6. After activation: wheel over the conversation scrolls it; wheel **over
   the header puck** still cycles pages (chat morphs back out).
7. `Esc` and the header ✕ close the chat entirely at any point.

## Contract 2 — chat redesign

1. Header: title + "gemini … · N tools armed" status, ✱ (memories),
   ◔ (scheduled tasks), ＋ (new chat), ✕ (light circle). Drag pill centred;
   dragging empty header surface moves the window.
2. Thread strip: chips for recent threads, "+ New", mode pill, "✦ Flash
   mode" (click toggles Pro).
3. Resize: drag the bottom-right grip — ✔ dashed accent ghost + live
   `W × H` mono badge; on release the window resizes once and
   `overlay.chat_size` lands in config.json; relaunch restores the size.
   Minimum commit is 484×560.
4. Theme: switch theme in settings (e.g. Dracula) — every chat colour
   reskins (chrome, chips, cards, aurora tint). No hardcoded colours.
5. Memories view (✱): search filter, count + size, pin toggle (✦ turns
   yellow), delete (✕), retention footnote.
6. Tasks view (◔): rows for `oxidemx-task-*` timers with enable switch,
   Run now, Delete.

## Contract 3 — agent tools (needs `~/.config/oxidemx/gemini.key`)

1. Ask: *"Run brightnessctl to set the screen to 60%"* → ✔ green
   **Command executed** card with `$ command`, trimmed output, exit code.
2. Ask: *"Run rm -rf /tmp/test-dir"* (not allowlisted) → ✔ confirmation
   chips "Run it / Don't run" appear before anything executes.
3. Ask: *"Every day at 22:00 set brightness to 40%"* → ✔ accent **Task
   scheduled** card with schedule + next run + enable switch;
   `systemctl --user list-timers 'oxidemx-task-*'` shows the timer;
   the card's switch enables/disables it live.
4. Ask: *"Remember that I prefer warm light after sunset"* → ✔ mauve
   **Memory saved** card; the entry appears in the memories view and in
   `~/.local/share/oxidemx/memories.json`; a new session's first reply can
   recall it (injection).

## Contract 4 — new pages

1. Cycle to **Device**: dial wedges show live % under brightness/volume
   icons; wheel over them adjusts in 5 % steps. Power and Mouse open
   submenus (chevron badges on the wedge icons). Night light toggles and
   its green dot tracks the real gsettings state. Network wedge shows a
   live ↓ rate.
2. Cycle to **Widgets**: CPU (with sparkline + cores/temp), Memory
   (used / total GB), Network ↓↑ with sparkline, Disk GB free, mouse
   Battery % from the daemon. Weather shows "— / set location" until a
   location is set: Settings → Settings tab → "Weather widget" → search
   a city (Open-Meteo geocoder) and pick a result. Tasks counts
   scheduled oxidemx timers due within 24 h.
3. Per-page wedge labels render under every icon on all pages.

## Vision-loop captures (no daemon needed)

```bash
HOME=/tmp/ox-vision XDG_CONFIG_HOME=/tmp/ox-vision/.config \
OXIDEMX_VISION_SHOT=/tmp/shot.png OXIDEMX_START_PAGE=3 \
OXIDEMX_VISION_DELAY_MS=1200 target/release/oxidemx-overlay
```

Pages: 0 Apps · 1 Device · 2 Widgets · 3 AI (armed chat). The instance
self-shows, captures its own window after the delay, saves the PNG, exits.
