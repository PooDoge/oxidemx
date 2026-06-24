/* global React, ReactDOM, window */
// ───────────────────────────────────────────────────────────────
// OxideMX · Composer — extracted feature file
// A fully-functional prompt composer, docked at the bottom of a
// minimal mock thread so it reads in-context. Demonstrates:
//   · + attach menu (upload / repo file / image / clipboard / code / camera)
//   · provider menu (Gemini · Claude · Local LLM groups + thinking level,
//     prompt optimizer, composer settings sub-page)
//   · attachment chips (removable) above the input
//   · auto-expanding rich-text editor — Shift+Enter newline, grows to a
//     line cap (tweakable 3/4/5) then scrolls; drag the top grip to resize
//   · live inline markdown — *bold* _italic_ `code` ~~strike~~ + # headings,
//     consumed on space
//   · gboard-style next-word prediction — 3-chip strip (or inline ghost)
//
// Freya authoring contract honored: data-freya / layout / region / action /
// anim annotations + named tokens only (no raw hex/rgba). Companion spec in
// "OxideMX - Composer.freya.json".
// ───────────────────────────────────────────────────────────────
const { useState, useRef, useEffect, useCallback, useLayoutEffect } = React;
const Icon = window.Icon;
const FX = window.FX;

// ── model catalog ───────────────────────────────────────────────
const PROVIDERS = [
  { id: "gemini", label: "Gemini", icon: "sparkle", tone: "blue", models: [
    { id: "gemini-3",        name: "Gemini 3",        sub: "frontier · multimodal" },
    { id: "gemini-2.5-pro",  name: "Gemini 2.5 Pro",  sub: "deep reasoning" },
    { id: "gemini-2.5-flash",name: "Gemini 2.5 Flash",sub: "fast · cheap" },
  ] },
  { id: "claude", label: "Claude", icon: "brain", tone: "peach", models: [
    { id: "opus-4.8",   name: "Opus 4.8",   sub: "top-tier · agentic" },
    { id: "sonnet-4.6", name: "Sonnet 4.6", sub: "balanced default" },
    { id: "haiku-4.6",  name: "Haiku 4.6",  sub: "snappy" },
  ] },
  { id: "local", label: "Local LLM", icon: "chip", tone: "green", models: [
    { id: "qwen-tools",  name: "Qwen 3B — Tool Calling", sub: "on-device · gateway" },
    { id: "qwen-web",    name: "Qwen 3B — Web Search",   sub: "on-device · gateway" },
    { id: "qwen-all",    name: "Qwen 3B — All",          sub: "on-device · gateway" },
  ] },
];
const MODEL_BY_ID = {};
PROVIDERS.forEach(p => p.models.forEach(m => { MODEL_BY_ID[m.id] = { ...m, provider: p.id, icon: p.icon, tone: p.tone, label: p.label }; }));

// ── attach sources ──────────────────────────────────────────────
const ATTACH_SOURCES = [
  { id: "upload", icon: "download",  label: "Upload file",        hint: "from disk",
    make: () => ({ icon: "disk",      tone: "blue",   name: "metrics-export.csv", meta: "42 KB" }) },
  { id: "repo",   icon: "folder",    label: "Reference repo file",hint: "@ file in project",
    make: () => ({ icon: "folder",    tone: "accent", name: "run_bridge.rs",      meta: "agentd/src" }) },
  { id: "image",  icon: "camera",    label: "Image / screenshot", hint: "png · jpg",
    make: () => ({ icon: "camera",    tone: "mauve",  name: "screenshot.png",     meta: "1440×900" }) },
  { id: "paste",  icon: "clipboard", label: "Paste from clipboard",hint: "current contents",
    make: () => ({ icon: "clipboard", tone: "teal",   name: "Clipboard",          meta: "text · 1.2 KB" }) },
  { id: "code",   icon: "terminal",  label: "Code snippet",       hint: "fenced block",
    make: () => ({ icon: "terminal",  tone: "green",  name: "snippet.ts",         meta: "12 lines" }) },
  { id: "camera", icon: "camera",    label: "Camera",             hint: "capture now",
    make: () => ({ icon: "camera",    tone: "peach",  name: "capture.jpg",        meta: "live" }) },
];

// ── prediction model (canned, gboard-flavored) ──────────────────
const COMPLETIONS = [
  "refactor","function","component","implement","optimize","explain","generate","summarize",
  "documentation","repository","dependencies","authentication","configuration","performance",
  "interface","responsive","accessibility","because","conversation","concurrency","architecture",
];
const NEXT = {
  "":          ["Refactor", "Explain", "Summarize"],
  "refactor":  ["the", "this", "these"],
  "explain":   ["the", "how", "why"],
  "summarize": ["the", "this", "what"],
  "the":       ["function", "component", "file"],
  "this":      ["function", "file", "into"],
  "add":       ["a", "support", "tests"],
  "write":     ["a", "tests", "the"],
  "fix":       ["the", "this", "all"],
  "make":      ["it", "the", "this"],
  "into":      ["a", "smaller", "the"],
  "a":         ["new", "single", "small"],
};
const NEXT_FALLBACK = ["the", "and", "to"];

function predict(lineBeforeCaret) {
  const partialMatch = lineBeforeCaret.match(/([A-Za-z][\w-]*)$/);
  const partial = partialMatch ? partialMatch[1] : "";
  if (partial) {
    const lp = partial.toLowerCase();
    const hits = COMPLETIONS.filter(w => w.startsWith(lp) && w !== lp).slice(0, 3);
    return { mode: "complete", partial, items: hits };
  }
  const words = lineBeforeCaret.trim().toLowerCase().split(/\s+/);
  const prev = words[words.length - 1] || "";
  const items = (NEXT[prev] || NEXT_FALLBACK).slice(0, 3);
  return { mode: "next", partial: "", items };
}

// ───────────────────────────────────────────────────────────────
// Editor (uncontrolled contenteditable, imperatively managed)
// ───────────────────────────────────────────────────────────────
const MD_PATTERNS = [
  { re: /`([^`]+)`$/,        tag: "code" },
  { re: /~~([^~]+)~~$/,      tag: "s" },
  { re: /\*([^*\n]+)\*$/,    tag: "strong" },
  { re: /_([^_\n]+)_$/,      tag: "em" },
];

function normalizeLines(editor) {
  // Every direct child must be a div.cline; self-heal after merges/paste.
  const kids = Array.from(editor.childNodes);
  if (kids.length === 0) {
    const d = document.createElement("div");
    d.className = "cline"; d.appendChild(document.createElement("br"));
    editor.appendChild(d);
    return;
  }
  kids.forEach(node => {
    if (node.nodeType === 1 && node.classList && node.classList.contains("cline")) {
      if (node.childNodes.length === 0) node.appendChild(document.createElement("br"));
      return;
    }
    const wrap = document.createElement("div");
    wrap.className = "cline";
    editor.insertBefore(wrap, node);
    wrap.appendChild(node);
  });
}

function currentLine(editor) {
  const sel = window.getSelection();
  if (!sel.rangeCount) return editor.firstChild;
  let n = sel.getRangeAt(0).startContainer;
  while (n && n.parentNode !== editor) n = n.parentNode;
  return (n && n.parentNode === editor) ? n : editor.firstChild;
}

function stripGhost(editor) {
  editor.querySelectorAll(".ghost").forEach(g => g.remove());
}

function getText(editor) {
  const clone = editor.cloneNode(true);
  clone.querySelectorAll(".ghost").forEach(g => g.remove());
  return Array.from(clone.querySelectorAll(".cline"))
    .map(l => l.textContent.replace(/\u00a0/g, " "))
    .join("\n");
}

function lineTextBeforeCaret(editor) {
  const sel = window.getSelection();
  if (!sel.rangeCount) return "";
  const range = sel.getRangeAt(0).cloneRange();
  const line = currentLine(editor);
  if (!line) return "";
  range.setStart(line, 0);
  return range.toString().replace(/\u00a0/g, " ");
}

function placeCaret(node, offset) {
  const sel = window.getSelection();
  const r = document.createRange();
  r.setStart(node, offset);
  r.collapse(true);
  sel.removeAllRanges();
  sel.addRange(r);
}

// ───────────────────────────────────────────────────────────────
function Composer({ T, tw, onSend, working, modelId, setModelId, thinking, setThinking }) {
  const edRef = useRef(null);
  const [empty, setEmpty] = useState(true);
  const [lineCount, setLineCount] = useState(1);
  const [isLong, setIsLong] = useState(false);
  const [pred, setPred] = useState({ mode: "next", partial: "", items: NEXT[""] });
  const [attachments, setAttachments] = useState([]);
  const [attachOpen, setAttachOpen] = useState(false);
  const [providerOpen, setProviderOpen] = useState(false);
  const [provView, setProvView] = useState("models"); // models | settings
  const [optimizer, setOptimizer] = useState(false);
  const [sendOnEnter, setSendOnEnter] = useState(true);
  const [manualH, setManualH] = useState(null);

  const model = MODEL_BY_ID[modelId];
  const lineH = 23;
  const capPx = tw.lineCap * lineH + 6;

  // init editor once
  useEffect(() => {
    const ed = edRef.current;
    if (ed && ed.childNodes.length === 0) normalizeLines(ed);
  }, []);

  const refresh = useCallback(() => {
    const ed = edRef.current;
    if (!ed) return;
    stripGhost(ed);
    normalizeLines(ed);
    const txt = getText(ed);
    const isEmpty = txt.trim() === "";
    setEmpty(isEmpty);
    setLineCount(ed.querySelectorAll(".cline").length);
    setIsLong(ed.scrollHeight > ed.clientHeight + 1);

    // predictions
    const before = lineTextBeforeCaret(ed);
    const p = predict(before);
    setPred(p);

    // ghost (inline completion) — only when caret at end of its line
    if (tw.prediction === "ghost" && p.mode === "complete" && p.items.length) {
      const sel = window.getSelection();
      if (sel.rangeCount) {
        const line = currentLine(ed);
        const r = sel.getRangeAt(0);
        const atEnd = line && r.collapsed &&
          (() => { const rr = document.createRange(); rr.selectNodeContents(line); rr.setStart(r.endContainer, r.endOffset); return rr.toString() === ""; })();
        if (atEnd) {
          const suffix = p.items[0].slice(p.partial.length);
          if (suffix) {
            const g = document.createElement("span");
            g.className = "ghost";
            g.setAttribute("contenteditable", "false");
            g.textContent = suffix;
            line.appendChild(g);
          }
        }
      }
    }
  }, [tw.prediction]);

  // re-run ghost logic when prediction mode changes
  useEffect(() => { refresh(); }, [tw.prediction, refresh]);

  const insertAtCaret = (text, replaceLen = 0) => {
    const ed = edRef.current;
    ed.focus();
    const sel = window.getSelection();
    if (!sel.rangeCount) return;
    const range = sel.getRangeAt(0);
    if (replaceLen > 0 && range.startContainer.nodeType === 3) {
      const node = range.startContainer;
      const off = range.startOffset;
      const del = document.createRange();
      del.setStart(node, Math.max(0, off - replaceLen));
      del.setEnd(node, off);
      del.deleteContents();
    } else if (replaceLen > 0) {
      for (let i = 0; i < replaceLen; i++) document.execCommand("delete");
    }
    document.execCommand("insertText", false, text);
    refresh();
  };

  const acceptSuggestion = (word) => {
    if (pred.mode === "complete") insertAtCaret(word + " ", pred.partial.length);
    else insertAtCaret(word + " ", 0);
  };

  const tryInlineMarkdown = () => {
    const ed = edRef.current;
    const sel = window.getSelection();
    if (!sel.rangeCount) return false;
    const range = sel.getRangeAt(0);
    if (!range.collapsed || range.startContainer.nodeType !== 3) return false;
    const node = range.startContainer;
    const offset = range.startOffset;
    const before = node.textContent.slice(0, offset);
    for (const p of MD_PATTERNS) {
      const m = before.match(p.re);
      if (m) {
        const start = offset - m[0].length;
        const r = document.createRange();
        r.setStart(node, start); r.setEnd(node, offset);
        r.deleteContents();
        const el = document.createElement(p.tag);
        el.textContent = m[1];
        r.insertNode(el);
        const space = document.createTextNode("\u00a0");
        el.after(space);
        placeCaret(space, 1);
        return true;
      }
    }
    return false;
  };

  const tryHeading = () => {
    const ed = edRef.current;
    const before = lineTextBeforeCaret(ed).trimStart();
    const m = before.match(/^(#{1,3})$/);
    if (!m) return false;
    const line = currentLine(ed);
    if (!line) return false;
    line.innerHTML = "";
    line.appendChild(document.createElement("br"));
    line.className = "cline md-h" + m[1].length;
    placeCaret(line, 0);
    return true;
  };

  const splitLine = () => {
    const ed = edRef.current;
    const sel = window.getSelection();
    if (!sel.rangeCount) return;
    const range = sel.getRangeAt(0);
    const line = currentLine(ed);
    if (!line) return;
    const tail = document.createRange();
    tail.setStart(range.endContainer, range.endOffset);
    tail.setEnd(line, line.childNodes.length);
    const frag = tail.extractContents();
    const nl = document.createElement("div");
    nl.className = "cline";
    if (frag.textContent === "" && frag.childNodes.length === 0) nl.appendChild(document.createElement("br"));
    else nl.appendChild(frag);
    if (nl.childNodes.length === 0) nl.appendChild(document.createElement("br"));
    line.after(nl);
    if (line.textContent === "") { line.innerHTML = ""; line.appendChild(document.createElement("br")); }
    placeCaret(nl, 0);
  };

  const doSend = () => {
    const ed = edRef.current;
    const txt = getText(ed).trim();
    if (!txt && attachments.length === 0) return;
    onSend(txt, attachments);
    ed.innerHTML = "";
    normalizeLines(ed);
    setAttachments([]);
    setManualH(null);
    refresh();
  };

  const onKeyDown = (e) => {
    stripGhost(edRef.current);
    if (e.key === "Tab") {
      // accept ghost / first suggestion
      if (pred.items.length) { e.preventDefault(); acceptSuggestion(pred.items[0]); return; }
    }
    if (e.key === "Enter") {
      if (e.shiftKey) { e.preventDefault(); splitLine(); refresh(); return; }
      if (sendOnEnter) { e.preventDefault(); doSend(); return; }
      // sendOnEnter off → Enter makes a newline
      e.preventDefault(); splitLine(); refresh(); return;
    }
    if (e.key === " " && tw.markdown) {
      if (tryHeading()) { e.preventDefault(); refresh(); return; }
      if (tryInlineMarkdown()) { e.preventDefault(); refresh(); return; }
    }
  };

  // ── resize grip (drag top edge) ──
  const dragRef = useRef(null);
  const onGripDown = (e) => {
    e.preventDefault();
    const ed = edRef.current;
    dragRef.current = { startY: e.clientY, startH: ed.clientHeight };
    const move = (ev) => {
      const d = dragRef.current; if (!d) return;
      const next = Math.max(capPx, Math.min(520, d.startH + (d.startY - ev.clientY)));
      setManualH(next);
    };
    const up = () => { dragRef.current = null; window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", up); };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const addAttachment = (src) => {
    setAttachments(a => [...a, { id: src.id + "-" + Date.now(), ...src.make() }]);
    setAttachOpen(false);
    setTimeout(() => edRef.current && edRef.current.focus(), 0);
  };
  const removeAttachment = (id) => setAttachments(a => a.filter(x => x.id !== id));

  const showGrip = (isLong || manualH != null);
  const editorMaxH = manualH != null ? manualH : capPx;

  return (
    <div data-region="footer" data-layout="col" data-bp="wide"
      style={{ padding: "12px 18px 16px", borderTop: `1px solid ${T.hair}`, background: T.base, position: "relative" }}>

      {/* activity line */}
      {tw.activity && (
        <div data-layout="row" style={{ display: "flex", alignItems: "center", gap: 7, fontSize: 11, color: T.subtext0, padding: "0 4px 9px" }}>
          <span style={{ display: "inline-flex", color: model.tone ? T[model.tone] : T.accent }}><Icon name={model.icon} size={12} /></span>
          <span>{model.name} · {thinking} reasoning · {optimizer ? "optimizer on" : "3 tools armed"}</span>
          <span data-align="push-right" style={{ marginLeft: "auto", fontFamily: "var(--mono)", fontSize: 10.5, color: T.faint }}>/ for commands</span>
        </div>
      )}

      <div data-freya="Card" style={{ borderRadius: 16, background: T.crust, border: `1px solid ${providerOpen || attachOpen ? T.accent_33 : T.surface2}`,
        boxShadow: showGrip ? `0 -1px 0 ${T.hair}` : "none", transition: "border-color 160ms" }}>

        {/* resize grip */}
        {showGrip && (
          <div data-freya="custom" onPointerDown={onGripDown}
            style={{ height: 14, display: "grid", placeItems: "center", cursor: "ns-resize", touchAction: "none" }} title="Drag to resize">
            <span style={{ width: 34, height: 3, borderRadius: 2, background: T.surface2 }} />
          </div>
        )}

        {/* prediction chips strip */}
        {tw.prediction === "chips" && !empty && pred.items.length > 0 && (
          <div data-layout="row" data-freya="custom" data-anim="fade" data-anim-ms="120"
            style={{ display: "flex", gap: 7, padding: showGrip ? "0 12px 8px" : "10px 12px 8px", overflowX: "auto" }} className="noscroll">
            {pred.items.map((w, i) => (
              <button key={w + i} data-freya="Chip" data-action="press" onMouseDown={(e) => { e.preventDefault(); acceptSuggestion(w); }}
                style={{ flex: "0 0 auto", fontSize: 12.5, fontWeight: 500, color: i === 0 ? T.accent : T.subtext1,
                  background: i === 0 ? T.accent_14 : T.surface0, border: `1px solid ${i === 0 ? T.accent_33 : T.hair}`,
                  borderRadius: 999, padding: "5px 13px", cursor: "pointer", fontFamily: "var(--font)", whiteSpace: "nowrap" }}>
                {pred.mode === "complete" ? w : w}
              </button>
            ))}
            <span style={{ flex: "0 0 auto", alignSelf: "center", marginLeft: 2, fontFamily: "var(--mono)", fontSize: 10, color: T.faint, whiteSpace: "nowrap" }}>⇥ tab</span>
          </div>
        )}

        {/* attachment chips */}
        {attachments.length > 0 && (
          <div data-layout="row" style={{ display: "flex", flexWrap: "wrap", gap: 8, padding: (tw.prediction === "chips" && !empty) ? "0 12px 4px" : "12px 12px 4px" }}>
            {attachments.map(a => (
              <span key={a.id} data-freya="Chip" style={{ display: "inline-flex", alignItems: "center", gap: 8, padding: "5px 6px 5px 5px",
                borderRadius: 10, background: T.surface0, border: `1px solid ${T.hair}` }}>
                <span style={{ width: 26, height: 26, borderRadius: 7, flex: "0 0 26px", display: "grid", placeItems: "center",
                  background: T[a.tone + "_16"], color: T[a.tone], border: `1px solid ${T[a.tone + "_33"]}` }}><Icon name={a.icon} size={13} /></span>
                <span style={{ display: "flex", flexDirection: "column", lineHeight: 1.2 }}>
                  <span style={{ fontSize: 12, fontWeight: 600, color: T.text }}>{a.name}</span>
                  <span style={{ fontFamily: "var(--mono)", fontSize: 9.5, color: T.subtext0 }}>{a.meta}</span>
                </span>
                <button data-freya="Button" data-action="press" onClick={() => removeAttachment(a.id)} title="Remove"
                  style={{ width: 20, height: 20, borderRadius: 6, display: "grid", placeItems: "center", color: T.subtext0,
                    background: "transparent", border: "none", cursor: "pointer" }}><Icon name="close" size={12} /></button>
              </span>
            ))}
          </div>
        )}

        {/* editor */}
        <div style={{ position: "relative", padding: "10px 14px 6px" }}>
          {empty && (
            <div style={{ position: "absolute", top: 10, left: 14, fontSize: 14.5, color: T.faint, pointerEvents: "none" }}>
              Ask, paste, or describe an automation… <span style={{ fontFamily: "var(--mono)", color: T.subtext0 }}>*bold*</span> renders live
            </div>
          )}
          <div ref={edRef} data-freya="Input" data-action="input" contentEditable suppressContentEditableWarning
            role="textbox" aria-multiline="true" spellCheck={false}
            onInput={refresh} onKeyDown={onKeyDown} onKeyUp={(e) => { if (["ArrowLeft","ArrowRight","ArrowUp","ArrowDown"].includes(e.key)) refresh(); }}
            onClick={refresh}
            className="cm-editor noscroll"
            style={{ fontSize: 14.5, lineHeight: lineH + "px", color: T.text, outline: "none", minHeight: lineH + "px",
              maxHeight: editorMaxH, overflowY: "auto", caretColor: T.accent, fontFamily: "var(--font)" }} />
        </div>

        {/* toolbar */}
        <div data-layout="row" style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 10px 10px" }}>
          {/* + attach */}
          <div style={{ position: "relative" }}>
            <button data-freya="Button" data-action="press" title="Attach" onClick={() => { setAttachOpen(o => !o); setProviderOpen(false); }}
              style={{ width: 34, height: 34, borderRadius: 10, display: "grid", placeItems: "center", cursor: "pointer",
                color: attachOpen ? T.accent : T.subtext1, background: attachOpen ? T.accent_14 : T.surface0,
                border: `1px solid ${attachOpen ? T.accent_33 : T.hair}`, transition: "all 140ms",
                transform: attachOpen ? "rotate(45deg)" : "none" }}>
              <Icon name="plus" size={18} />
            </button>
            {attachOpen && <AttachMenu T={T} onPick={addAttachment} onClose={() => setAttachOpen(false)} />}
          </div>

          {/* provider pill */}
          <div style={{ position: "relative" }}>
            <button data-freya="Select" data-action="select" onClick={() => { setProviderOpen(o => !o); setProvView("models"); setAttachOpen(false); }}
              style={{ display: "inline-flex", alignItems: "center", gap: 8, height: 34, padding: "0 11px", borderRadius: 10, cursor: "pointer",
                background: providerOpen ? T.accent_14 : T.surface0, border: `1px solid ${providerOpen ? T.accent_33 : T.hair}`, color: T.text }}>
              <span style={{ display: "inline-flex", color: T[model.tone] }}><Icon name={model.icon} size={14} /></span>
              <span style={{ fontSize: 13, fontWeight: 600 }}>{model.name}</span>
              <span style={{ fontFamily: "var(--mono)", fontSize: 9.5, color: T.subtext0, background: T.surface1, border: `1px solid ${T.hair}`,
                borderRadius: 5, padding: "1px 5px" }}>{thinking[0].toUpperCase()}</span>
              <Icon name="chevronDown" size={13} style={{ color: T.subtext0 }} />
            </button>
            {providerOpen && (
              <ProviderMenu T={T} modelId={modelId} setModelId={(id) => { setModelId(id); setProviderOpen(false); }}
                thinking={thinking} setThinking={setThinking}
                optimizer={optimizer} setOptimizer={setOptimizer}
                sendOnEnter={sendOnEnter} setSendOnEnter={setSendOnEnter}
                view={provView} setView={setProvView} onClose={() => setProviderOpen(false)} />
            )}
          </div>

          {optimizer && (
            <span data-freya="Chip" title="Prompt optimizer on" style={{ display: "inline-flex", alignItems: "center", gap: 5, height: 28, padding: "0 9px",
              borderRadius: 999, fontSize: 11, fontWeight: 600, color: T.mauve, background: T.mauve_16, border: `1px solid ${T.mauve_33}` }}>
              <Icon name="sparkle" size={12} /> optimize
            </span>
          )}

          <span data-align="push-right" style={{ marginLeft: "auto", display: "inline-flex", alignItems: "center", gap: 10 }}>
            {(lineCount > 1 || isLong) && (
              <span style={{ fontFamily: "var(--mono)", fontSize: 10, color: T.faint }}>{lineCount} lines · ⇧⏎ newline</span>
            )}
            <button data-freya="Button" data-action="press" data-shadow="0 4 12 T.accent_50" onClick={working ? undefined : doSend} title={working ? "Stop" : "Send"}
              style={{ width: 38, height: 38, borderRadius: 11, display: "grid", placeItems: "center", cursor: "pointer", border: "none",
                background: working ? T.surface2 : (empty && attachments.length === 0 ? T.surface1 : T.accent),
                color: working ? T.red : (empty && attachments.length === 0 ? T.faint : T.crust),
                boxShadow: working || (empty && attachments.length === 0) ? "none" : `0 4px 12px ${T.accent_50}`, transition: "all 160ms" }}>
              <Icon name={working ? "stop" : "send"} size={working ? 15 : 17} strokeWidth={2} />
            </button>
          </span>
        </div>
      </div>
    </div>
  );
}

// ── attach menu (Popup) ─────────────────────────────────────────
function AttachMenu({ T, onPick, onClose }) {
  useEffect(() => {
    const h = (e) => { if (!e.target.closest("[data-attach-menu]")) onClose(); };
    setTimeout(() => document.addEventListener("mousedown", h), 0);
    return () => document.removeEventListener("mousedown", h);
  }, [onClose]);
  return (
    <div data-attach-menu data-freya="Menu" data-anim="slide" data-anim-ms="140" data-radius="14,14,14,14"
      style={{ position: "absolute", bottom: "100%", marginBottom: 8, left: 0, width: 252, zIndex: 40,
        background: T.mantle, border: `1px solid ${T.surface1}`, borderRadius: 14, padding: 6,
        boxShadow: `0 18px 44px ${T.shadowDeep}`, animation: "fx-menu-pop 140ms ease-out" }}>
      <div style={{ fontSize: 9.5, letterSpacing: "0.12em", fontWeight: 700, color: T.subtext0, padding: "5px 9px 4px" }}>ATTACH</div>
      {ATTACH_SOURCES.map(s => (
        <button key={s.id} data-freya="Button" data-action="press" onClick={() => onPick(s)}
          style={{ display: "flex", alignItems: "center", gap: 11, width: "100%", padding: "8px 9px", borderRadius: 9,
            background: "transparent", border: "none", cursor: "pointer", textAlign: "left", color: T.text }}
          onMouseEnter={(e) => e.currentTarget.style.background = T.surface0}
          onMouseLeave={(e) => e.currentTarget.style.background = "transparent"}>
          <span style={{ width: 28, height: 28, borderRadius: 8, flex: "0 0 28px", display: "grid", placeItems: "center",
            color: T.subtext1, background: T.surface0, border: `1px solid ${T.hair}` }}><Icon name={s.icon} size={15} /></span>
          <span style={{ display: "flex", flexDirection: "column", lineHeight: 1.25 }}>
            <span style={{ fontSize: 13, fontWeight: 500 }}>{s.label}</span>
            <span style={{ fontFamily: "var(--mono)", fontSize: 10, color: T.subtext0 }}>{s.hint}</span>
          </span>
        </button>
      ))}
    </div>
  );
}

// ── provider menu (Popup) ───────────────────────────────────────
function ProviderMenu({ T, modelId, setModelId, thinking, setThinking, optimizer, setOptimizer, sendOnEnter, setSendOnEnter, view, setView, onClose }) {
  useEffect(() => {
    const h = (e) => { if (!e.target.closest("[data-provider-menu]")) onClose(); };
    setTimeout(() => document.addEventListener("mousedown", h), 0);
    return () => document.removeEventListener("mousedown", h);
  }, [onClose]);

  const Row = ({ children, onClick, freya = "Button", action = "press" }) => (
    <button data-freya={freya} data-action={action} onClick={onClick}
      style={{ display: "flex", alignItems: "center", gap: 10, width: "100%", padding: "8px 10px", borderRadius: 9,
        background: "transparent", border: "none", cursor: "pointer", textAlign: "left", color: T.text }}
      onMouseEnter={(e) => e.currentTarget.style.background = T.surface0}
      onMouseLeave={(e) => e.currentTarget.style.background = "transparent"}>{children}</button>
  );

  return (
    <div data-provider-menu data-freya="Menu" data-anim="slide" data-anim-ms="140" data-radius="16,16,16,16"
      style={{ position: "absolute", bottom: "100%", marginBottom: 8, left: 0, width: 308, zIndex: 40,
        background: T.mantle, border: `1px solid ${T.surface1}`, borderRadius: 16, overflow: "hidden",
        boxShadow: `0 22px 52px ${T.shadowDeep}`, animation: "fx-menu-pop 140ms ease-out" }}>

      {view === "models" ? (
        <>
          <div style={{ maxHeight: 320, overflowY: "auto", padding: 6 }} className="noscroll">
            {PROVIDERS.map(p => (
              <div key={p.id}>
                <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "7px 9px 4px" }}>
                  <span style={{ display: "inline-flex", color: T[p.tone] }}><Icon name={p.icon} size={12} /></span>
                  <span style={{ fontSize: 10, letterSpacing: "0.1em", fontWeight: 700, color: T.subtext0 }}>{p.label.toUpperCase()}</span>
                </div>
                {p.models.map(m => {
                  const active = m.id === modelId;
                  return (
                    <button key={m.id} data-freya="RadioItem" data-action="select" onClick={() => setModelId(m.id)}
                      style={{ display: "flex", alignItems: "center", gap: 10, width: "100%", padding: "7px 10px", borderRadius: 9, cursor: "pointer",
                        textAlign: "left", border: `1px solid ${active ? T.accent_33 : "transparent"}`, color: T.text,
                        background: active ? T.accent_14 : "transparent" }}
                      onMouseEnter={(e) => { if (!active) e.currentTarget.style.background = T.surface0; }}
                      onMouseLeave={(e) => { if (!active) e.currentTarget.style.background = "transparent"; }}>
                      <span style={{ flex: 1, minWidth: 0 }}>
                        <span style={{ fontSize: 13, fontWeight: 600, color: active ? T.accent : T.text }}>{m.name}</span>
                        <span style={{ display: "block", fontSize: 10.5, color: T.subtext0 }}>{m.sub}</span>
                      </span>
                      {active && <Icon name="check" size={15} style={{ color: T.accent }} />}
                    </button>
                  );
                })}
              </div>
            ))}
          </div>

          {/* thinking level */}
          <div style={{ borderTop: `1px solid ${T.surface0}`, padding: "9px 12px 6px" }}>
            <div style={{ fontSize: 10, letterSpacing: "0.1em", fontWeight: 700, color: T.subtext0, marginBottom: 6 }}>THINKING LEVEL</div>
            <div data-freya="SegmentedButton" data-action="select" style={{ display: "flex", gap: 4, background: T.surface0, border: `1px solid ${T.hair}`, borderRadius: 9, padding: 3 }}>
              {["low", "medium", "high"].map(l => {
                const on = thinking === l;
                return (
                  <button key={l} onClick={() => setThinking(l)}
                    style={{ flex: 1, padding: "6px 0", borderRadius: 7, border: "none", cursor: "pointer", textTransform: "capitalize",
                      fontSize: 12, fontWeight: 600, fontFamily: "var(--font)",
                      color: on ? T.crust : T.subtext1, background: on ? T.accent : "transparent" }}>{l}</button>
                );
              })}
            </div>
          </div>

          {/* footer rows */}
          <div style={{ borderTop: `1px solid ${T.surface0}`, padding: 6 }}>
            <Row freya="Switch" action="toggle" onClick={() => setOptimizer(v => !v)}>
              <span style={{ width: 28, height: 28, borderRadius: 8, display: "grid", placeItems: "center", color: optimizer ? T.mauve : T.subtext1,
                background: optimizer ? T.mauve_16 : T.surface0, border: `1px solid ${optimizer ? T.mauve_33 : T.hair}` }}><Icon name="sparkle" size={15} /></span>
              <span style={{ flex: 1, fontSize: 13, fontWeight: 500 }}>Prompt optimizer</span>
              <ToggleTrack T={T} on={optimizer} />
            </Row>
            <Row onClick={() => setView("settings")}>
              <span style={{ width: 28, height: 28, borderRadius: 8, display: "grid", placeItems: "center", color: T.subtext1,
                background: T.surface0, border: `1px solid ${T.hair}` }}><Icon name="gear" size={15} /></span>
              <span style={{ flex: 1, fontSize: 13, fontWeight: 500 }}>Composer settings</span>
              <Icon name="chevronRight" size={15} style={{ color: T.subtext0 }} />
            </Row>
          </div>
        </>
      ) : (
        <div style={{ padding: 6 }}>
          <button data-freya="Button" data-action="navigate" onClick={() => setView("models")}
            style={{ display: "flex", alignItems: "center", gap: 8, width: "100%", padding: "8px 9px", borderRadius: 9, marginBottom: 2,
              background: "transparent", border: "none", cursor: "pointer", color: T.subtext1, fontSize: 12.5, fontWeight: 600 }}>
            <Icon name="chevronLeft" size={15} /> Composer settings
          </button>
          <SettingRow T={T} icon="send" label="Send on Enter" hint="Shift+Enter = newline" on={sendOnEnter} onToggle={() => setSendOnEnter(v => !v)} />
          <SettingRow T={T} icon="sparkle" label="Prompt optimizer" hint="rewrite before send" on={optimizer} onToggle={() => setOptimizer(v => !v)} />
          <div style={{ padding: "8px 10px", fontSize: 10.5, color: T.faint, lineHeight: 1.5 }}>
            Prediction style, line cap &amp; live markdown live in <span style={{ color: T.accent, fontWeight: 600 }}>Tweaks</span>.
          </div>
        </div>
      )}
    </div>
  );
}

function ToggleTrack({ T, on }) {
  return (
    <span style={{ width: 36, height: 20, borderRadius: 999, flex: "0 0 36px", padding: 2, transition: "background 160ms",
      background: on ? T.accent : T.surface2 }}>
      <span style={{ display: "block", width: 16, height: 16, borderRadius: 999, background: on ? T.crust : T.subtext1,
        transform: on ? "translateX(16px)" : "none", transition: "transform 160ms" }} />
    </span>
  );
}

function SettingRow({ T, icon, label, hint, on, onToggle }) {
  return (
    <button data-freya="Switch" data-action="toggle" onClick={onToggle}
      style={{ display: "flex", alignItems: "center", gap: 11, width: "100%", padding: "8px 10px", borderRadius: 9,
        background: "transparent", border: "none", cursor: "pointer", textAlign: "left", color: T.text }}
      onMouseEnter={(e) => e.currentTarget.style.background = T.surface0}
      onMouseLeave={(e) => e.currentTarget.style.background = "transparent"}>
      <span style={{ width: 28, height: 28, borderRadius: 8, flex: "0 0 28px", display: "grid", placeItems: "center", color: T.subtext1,
        background: T.surface0, border: `1px solid ${T.hair}` }}><Icon name={icon} size={15} /></span>
      <span style={{ flex: 1, display: "flex", flexDirection: "column", lineHeight: 1.25 }}>
        <span style={{ fontSize: 13, fontWeight: 500 }}>{label}</span>
        <span style={{ fontSize: 10.5, color: T.subtext0 }}>{hint}</span>
      </span>
      <ToggleTrack T={T} on={on} />
    </button>
  );
}

// ───────────────────────────────────────────────────────────────
// Mock thread shell
// ───────────────────────────────────────────────────────────────
function Bubble({ T, role, text, atts }) {
  const user = role === "user";
  return (
    <div data-freya="custom" data-align={user ? "push-right" : undefined}
      data-radius={user ? "14,14,4,14" : "14,14,14,4"}
      style={{ alignSelf: user ? "flex-end" : "flex-start", maxWidth: "78%",
        background: user ? T.accent_1a : T.surface0,
        border: `1px solid ${user ? T.accent_33 : T.hair}`,
        borderRadius: user ? "14px 14px 4px 14px" : "14px 14px 14px 4px",
        padding: "10px 14px", fontSize: 13.5, lineHeight: 1.55, color: T.text, whiteSpace: "pre-wrap" }}>
      {atts && atts.length > 0 && (
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginBottom: text ? 8 : 0 }}>
          {atts.map(a => (
            <span key={a.id} style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 11, fontFamily: "var(--mono)",
              color: T.subtext1, background: T.crust, border: `1px solid ${T.hair}`, borderRadius: 7, padding: "3px 8px" }}>
              <Icon name={a.icon} size={11} style={{ color: T[a.tone] }} />{a.name}
            </span>
          ))}
        </div>
      )}
      {text}
    </div>
  );
}

const SEED = [
  { role: "user", text: "Can you wire the run-event bridge to thread the conversation id through?" },
  { role: "assistant", text: "Done — I routed conversation_id into the Payload and added a regression test. Want me to run the suite?" },
];

function Root() {
  const [tw, setTweak] = window.useTweaks({
    accent: "#00d4ff",
    prediction: "chips",   // chips | ghost | off
    lineCap: 5,            // 3 | 4 | 5
    markdown: true,
    activity: true,
  });
  const accentSet = Object.values(FX.ACCENTS).find(a => a.accent === tw.accent) || FX.ACCENTS.cyan;
  const T = FX.makeTheme(accentSet);

  const [messages, setMessages] = useState(SEED);
  const [working, setWorking] = useState(false);
  const [modelId, setModelId] = useState("sonnet-4.6");
  const [thinking, setThinking] = useState("medium");
  const scrollRef = useRef(null);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages, working]);

  const onSend = (text, attachments) => {
    setMessages(m => [...m, { role: "user", text, atts: attachments, id: Date.now() }]);
    setWorking(true);
    setTimeout(() => {
      setMessages(m => [...m, { role: "assistant", text: "On it — drafting that change now.", id: Date.now() + 1 }]);
      setWorking(false);
    }, 1100);
  };

  return (
    <div style={{ minHeight: "100vh", display: "grid", placeItems: "center", padding: 32, boxSizing: "border-box",
      background: `radial-gradient(120% 90% at 50% 0%, ${T.bg1}, ${T.bg2} 60%, ${T.bg0})`, fontFamily: "var(--font)" }}>

      <div style={{ width: "100%", maxWidth: 760, marginBottom: 18 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 11 }}>
          <div style={{ width: 28, height: 28, borderRadius: 8, display: "grid", placeItems: "center",
            background: `linear-gradient(150deg, ${T.accent}, ${T.accentDim})`, color: T.crust, fontWeight: 800, fontSize: 12 }}>OX</div>
          <h1 style={{ margin: 0, fontSize: 19, fontWeight: 700, color: T.text, letterSpacing: "-0.02em" }}>Composer — feature study</h1>
        </div>
        <p style={{ margin: "9px 0 0", fontSize: 13, lineHeight: 1.6, color: T.subtext1, maxWidth: 620 }}>
          Try it: type <span style={{ fontFamily: "var(--mono)", color: T.accent }}>*bold*</span>, <span style={{ fontFamily: "var(--mono)", color: T.accent }}>`code`</span> or <span style={{ fontFamily: "var(--mono)", color: T.accent }}># heading</span> then space · <b style={{ color: T.text }}>Shift+Enter</b> for a new line · <b style={{ color: T.text }}>Tab</b> to accept a prediction · <b style={{ color: T.text }}>+</b> to attach · the model pill for providers. Behaviors are in <b style={{ color: T.accent }}>Tweaks</b>.
        </p>
      </div>

      {/* window */}
      <div data-layout="col" style={{ width: "100%", maxWidth: 760, height: 600, display: "flex", flexDirection: "column",
        background: T.base, borderRadius: 18, border: `1px solid ${T.surface1}`, overflow: "hidden",
        boxShadow: `0 30px 80px ${T.shadowDeep}, 0 0 60px ${T.accent_10}` }}>

        {/* slim header */}
        <div data-region="header" data-layout="row" style={{ display: "flex", alignItems: "center", gap: 9, padding: "12px 16px",
          borderBottom: `1px solid ${T.hair}`, background: T.mantle }}>
          <span style={{ display: "inline-flex", color: T.accent }}><Icon name="chip" size={15} /></span>
          <span style={{ fontSize: 13.5, fontWeight: 600, color: T.text }}>oxidemx-phase1</span>
          <span data-freya="Chip" style={{ display: "inline-flex", alignItems: "center", gap: 4, fontSize: 10.5, fontFamily: "var(--mono)",
            color: T.accent, background: T.accent_14, border: `1px solid ${T.accent_33}`, padding: "1px 7px", borderRadius: 999 }}>
            <Icon name="switch" size={11} />wt/bridge-fix</span>
          <span data-align="push-right" style={{ marginLeft: "auto", fontSize: 11, color: T.subtext0 }}>run-event bridge</span>
        </div>

        {/* transcript */}
        <div ref={scrollRef} data-region="scroll-body" data-freya="ScrollView" className="noscroll"
          style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "16px 18px", display: "flex", flexDirection: "column", gap: 12 }}>
          {messages.map((m, i) => <Bubble key={m.id || i} T={T} role={m.role} text={m.text} atts={m.atts} />)}
          {working && (
            <div style={{ alignSelf: "flex-start", display: "inline-flex", alignItems: "center", gap: 8, padding: "9px 13px",
              borderRadius: "14px 14px 14px 4px", background: T.surface0, border: `1px solid ${T.hair}`, color: T.subtext0, fontSize: 12.5 }}>
              <span data-anim="pulse" style={{ width: 6, height: 6, borderRadius: 3, background: T.accent, boxShadow: `0 0 6px ${T.accent}` }} />
              thinking…
            </div>
          )}
        </div>

        <Composer T={T} tw={tw} onSend={onSend} working={working}
          modelId={modelId} setModelId={setModelId} thinking={thinking} setThinking={setThinking} />
      </div>

      {/* Tweaks */}
      <window.TweaksPanel title="Tweaks">
        <window.TweakSection label="Prediction">
          <window.TweakRadio label="Style" value={tw.prediction}
            options={["chips", "ghost", "off"]} onChange={v => setTweak("prediction", v)} />
        </window.TweakSection>
        <window.TweakSection label="Editor">
          <window.TweakRadio label="Auto-expand cap" value={String(tw.lineCap)}
            options={["3", "4", "5"]} onChange={v => setTweak("lineCap", parseInt(v, 10))} />
          <window.TweakToggle label="Live markdown" value={tw.markdown} onChange={v => setTweak("markdown", v)} />
          <window.TweakToggle label="Activity line" value={tw.activity} onChange={v => setTweak("activity", v)} />
        </window.TweakSection>
        <window.TweakSection label="Theme">
          <window.TweakColor label="Accent" value={tw.accent}
            options={["#00d4ff", "#b388ff", "#ffab40", "#7be06a"]} onChange={v => setTweak("accent", v)} />
        </window.TweakSection>
      </window.TweaksPanel>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")).render(<Root />);
