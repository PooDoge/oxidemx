# design-pipeline — Claude Design → Freya

The two-way pipeline that turns Claude Design mockups into the Freya (`oxide-app`) UI.

- **`OxideMX-Freya-authoring-contract.md`** — paste into Claude Design (or pin as a project
  instruction). Makes designs deterministically translatable: name components after Freya
  built-ins + `data-freya`, named tokens (incl. named alpha), `data-region`/`data-rail-width`,
  structured `data-radius`/`data-gradient`, named `data-anim`, `data-action`, **and a companion
  machine-readable `<Name>.freya.json`** (the agent-facing contract — the translator reads this,
  not the JSX).
- **`validate_freya_spec.py`** — deterministic gate (no JSX AST): checks a `*.freya.json` for
  structure + internal consistency, and (with `--src <jsx/html>`) flags drift + contract
  violations (forbidden `${T.x}1a` alpha suffixes, unknown `data-freya`, missing tokens/regions).
  `python3 validate_freya_spec.py <spec.freya.json> --src <design files...>`
- **`fixtures/`** — a clean spec that passes + a dirty spec that exercises every check (self-test).

Translation itself is the `claude-design-to-freya` skill (`.claude/skills/`), which consumes a
`*.freya.json` when present and falls back to inferring from JSX otherwise.
