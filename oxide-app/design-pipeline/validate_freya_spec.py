#!/usr/bin/env python3
"""Validate a Claude Design `*.freya.json` spec against the OxideMX→Freya authoring
contract, and (optionally) check it against the design's JSX/HTML for drift.

The companion JSON is the agent-facing contract (see OxideMX-Freya-authoring-contract.md):
the design emits it; the translator reads it instead of parsing freeform JSX. This
validator is a deterministic gate — no JSX AST, just JSON structure + regex checks —
so a design either satisfies the contract or it doesn't.

Usage:
    validate_freya_spec.py <spec.freya.json> [--src FILE ...]

Exit code 0 = no errors (warnings allowed); 1 = contract errors; 2 = bad invocation.
"""
import json
import re
import sys

# Freya built-ins a design may map to (see the skill's "use built-ins" inventory).
BUILTINS = {
    "Button", "Input", "Switch", "Checkbox", "RadioItem", "Slider", "Select", "Chip",
    "SegmentedButton", "Card", "Accordion", "SideBarItem", "FloatingTab", "Popup", "Menu",
    "Tooltip", "ProgressBar", "CircularLoader", "Skeleton", "Table", "Calendar", "ColorPicker",
    "ScrollView", "VirtualScrollView", "Link", "ResizableContainer", "MarkdownViewer",
    "ImageViewer", "GifViewer", "CodeEditor", "Terminal", "Router", "Outlet", "Tile",
    "DragZone", "DropZone", "custom",
}
ACTIONS = {"press", "input", "toggle", "select", "navigate"}
REGION_KINDS = {"rail", "flex", "scroll-body", "footer", "row", "col", "content"}

# `${T.token}1a` — the alpha-hex-suffix the contract forbids (use a named alpha token).
ALPHA_SUFFIX = re.compile(r"\$\{?\s*T\.\w+\s*\}?[0-9a-fA-F]{2}\b")

errors: list[str] = []
warnings: list[str] = []


def err(msg): errors.append(msg)
def warn(msg): warnings.append(msg)


def walk(component, theme_tokens, anim_names, path="components"):
    """Recursively validate one component node and its children."""
    name = component.get("name")
    where = f"{path}[{name or '?'}]"
    if not name:
        err(f"{path}: component missing `name`")
    freya = component.get("freya")
    if not freya:
        err(f"{where}: missing `freya` (built-in name or \"custom\")")
    elif freya not in BUILTINS:
        err(f"{where}: `freya` = {freya!r} is not a known Freya built-in or \"custom\"")
    for tok in component.get("tokens", []):
        if tok not in theme_tokens:
            warn(f"{where}: token {tok!r} not defined in theme.tokens")
    action = component.get("action")
    if action is not None and action not in ACTIONS:
        err(f"{where}: action {action!r} not one of {sorted(ACTIONS)}")
    anim = component.get("anim")
    if anim is not None and anim not in anim_names:
        warn(f"{where}: anim {anim!r} not declared in animations[]")
    for child in component.get("children", []):
        walk(child, theme_tokens, anim_names, f"{where}.children")
    return [name] + [n for c in component.get("children", []) for n in walk_names(c)]


def walk_names(component):
    out = [component.get("name")] if component.get("name") else []
    for c in component.get("children", []):
        out += walk_names(c)
    return out


def main():
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    spec_path = args[0]
    src_files = []
    if "--src" in args:
        src_files = args[args.index("--src") + 1:]

    try:
        with open(spec_path, encoding="utf-8") as f:
            spec = json.load(f)
    except (OSError, json.JSONDecodeError) as e:
        print(f"✗ cannot read/parse {spec_path}: {e}")
        return 1

    for key in ("theme", "components", "layout", "animations"):
        if key not in spec:
            err(f"top-level: missing required key {key!r}")

    theme_tokens = set((spec.get("theme") or {}).get("tokens", {}).keys())
    if not theme_tokens:
        err("theme.tokens is missing or empty")
    anim_names = {a.get("name") for a in spec.get("animations", []) if a.get("name")}
    for a in spec.get("animations", []):
        for k in ("name", "kind", "ms"):
            if k not in a:
                err(f"animations[{a.get('name', '?')}]: missing {k!r}")

    spec_component_names = []
    for c in spec.get("components", []):
        walk(c, theme_tokens, anim_names)
        spec_component_names += walk_names(c)

    layout = spec.get("layout") or {}
    for r in layout.get("regions", []):
        if not r.get("id"):
            err("layout.regions: a region is missing `id`")
        kind = r.get("kind")
        if kind not in REGION_KINDS:
            warn(f"layout.regions[{r.get('id', '?')}]: kind {kind!r} not one of {sorted(REGION_KINDS)}")

    # Optional drift checks against the design source.
    src_text = ""
    for fp in src_files:
        try:
            with open(fp, encoding="utf-8") as f:
                src_text += f.read() + "\n"
        except OSError as e:
            warn(f"could not read --src {fp}: {e}")
    if src_text:
        for m in set(ALPHA_SUFFIX.findall(src_text)):
            err(f"source uses forbidden alpha-hex suffix {m!r} — use a named alpha token (e.g. T.accent_15)")
        for m in set(re.findall(r'data-freya="([^"]+)"', src_text)):
            if m not in BUILTINS:
                warn(f'source has data-freya="{m}" which is not a known built-in or "custom"')
        for name in spec_component_names:
            stem = name.split(".")[0]
            if stem and stem not in src_text:
                warn(f"spec component {name!r} not found in source (possible drift)")

    print(f"spec: {spec_path}")
    print(f"  components: {len(spec_component_names)} · tokens: {len(theme_tokens)} · "
          f"animations: {len(anim_names)} · regions: {len(layout.get('regions', []))}")
    for w in warnings:
        print(f"  ⚠ {w}")
    for e in errors:
        print(f"  ✗ {e}")
    if errors:
        print(f"FAIL — {len(errors)} error(s), {len(warnings)} warning(s)")
        return 1
    print(f"OK — contract satisfied ({len(warnings)} warning(s))")
    return 0


if __name__ == "__main__":
    sys.exit(main())
