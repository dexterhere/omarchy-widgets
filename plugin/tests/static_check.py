#!/usr/bin/env python3
"""Static safety check over the plugin's own QML and JS (everything outside tests/).

Nothing a widget file contains may be executed, evaluated, loaded as a component or URL,
or drawn as rich text; colours come from the theme adapter; the canvas never takes the
keyboard. This is a lint, not a proof: it makes the obvious and the sneaky spellings of
those mistakes fail CI. `--selftest` plants a violation for every rule and checks each fires.

  static_check.py            scan ../ (the plugin folder); exit 1 on any violation
  static_check.py PATH       scan the plugin folder at PATH (the mirror script lints the export)
  static_check.py --selftest check the rules against known-bad and known-good snippets
"""
import os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
PLUGIN = os.path.dirname(HERE)
THEME = {"Theme.qml"}


REGEX_AFTER_WORD = ("return", "typeof", "case", "in", "of", "void", "delete", "throw", "else")


def regex_end(text, i):
    """If text[i] == '/' starts a JS regex literal (judged by the previous token), return the index just past
    it and its flags; else -1. A regex such as /"/ would otherwise open a string to the comment stripper."""
    j = i - 1
    while j >= 0 and text[j] in " \t\r\n":
        j -= 1
    if j >= 0 and text[j] not in "(,=:[!&|?{};+-*%<>~^":
        k = j
        while k >= 0 and (text[k].isalnum() or text[k] == "_"):
            k -= 1
        if text[k + 1:j + 1] not in REGEX_AFTER_WORD:
            return -1
    n, k, cls = len(text), i + 1, False
    while k < n:
        ch = text[k]
        if ch == "\n":
            return -1
        if ch == "\\":
            k += 2
            continue
        if ch == "[":
            cls = True
        elif ch == "]":
            cls = False
        elif ch == "/" and not cls:
            k += 1
            while k < n and text[k].isalpha():
                k += 1
            return k
        k += 1
    return -1


def strip_comments(text):
    """Remove // and /* */ comments but leave string literals alone."""
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if c in "\"'`":
            j = i + 1
            while j < n and text[j] != c:
                j += 2 if text[j] == "\\" else 1
            out.append(text[i:j + 1]); i = j + 1
        elif text.startswith("//", i):
            while i < n and text[i] != "\n": i += 1
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2); j = n if j < 0 else j + 2
            out.append("\n" * text.count("\n", i, j)); i = j
        elif c == "/" and regex_end(text, i) > 0:
            j = regex_end(text, i); out.append(text[i:j]); i = j
        else:
            out.append(c); i += 1
    return "".join(out)


def blank_strings(text):
    """Same length, string contents replaced by spaces (so braces in strings do not count)."""
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if c in "\"'`":
            j = i + 1
            while j < n and text[j] != c:
                j += 2 if text[j] == "\\" else 1
            out.append(c + " " * (j - i - 1) + (text[j] if j < n else "")); i = j + 1
        elif c == "/" and regex_end(text, i) > 0:
            j = regex_end(text, i); out.append("/" + " " * (j - i - 1)); i = j
        else:
            out.append(c); i += 1
    return "".join(out)


def items(clean, types):
    """Yield (type, start_line, own_body) for every `Type {` item, nested blocks blanked from own_body."""
    shaped = blank_strings(clean)
    for m in re.finditer(r"(?<![\w.])(%s)\s*\{" % "|".join(types), shaped):
        depth, j = 1, m.end()
        while j < len(shaped) and depth:
            depth += {"{": 1, "}": -1}.get(shaped[j], 0); j += 1
        body = clean[m.end():j - 1]
        sb = shaped[m.end():j - 1]
        own, d = [], 0
        for k, ch in enumerate(sb):               # keep only depth-0 text: properties of this item itself
            if ch == "{": d += 1
            if d == 0: own.append(body[k])
            if ch == "}": d -= 1
        yield m.group(1), clean.count("\n", 0, m.start()) + 1, "".join(own)


def line(clean, pos):
    return clean.count("\n", 0, pos) + 1


# (name, regex, applies_in_Theme_qml)
RX = [
    ("dynamic QML object creation", r"createQmlObject|createComponent|incubateObject|Qt\s*\.\s*include", True),
    ("eval, Function constructor or .constructor call", r"\beval\s*\(|new\s+Function\b|(?<![\w.])Function\s*\(|\.\s*constructor\s*\(|\bconstructor\s*\[", True),
    ("computed member access on Qt, this, globalThis, Function or Object", r"\b(Qt|this|globalThis|Function|Object|Quickshell)\s*\[\s*[\"'`]", True),
    ("Loader source set at run time", r"\bsetSource\b|\.\s*source\s*=(?!=)|\[\s*[\"'`]source[\"'`]\s*\]|property\s*:\s*[\"'`]source[\"'`]", True),
    ("rich text mode", r"\b(RichText|StyledText|AutoText|MarkdownText)\b", True),
    ("image, icon or shader types", r"\b(Image|AnimatedImage|BorderImage|IconImage|ColorImage|SvgImage|ShaderEffect|ShaderEffectSource)\b|iconPath|image://", True),
    ("process, network or browser access", r"\bProcess\b|SocketServer|execDetached|Quickshell\s*\.\s*exec|XMLHttpRequest|WebSocket|openUrlExternally|\bfetch\s*\(", True),
    ("quitting, exiting or reloading the shell (any spelling)", r"Qt\s*\.\s*(quit|exit)|\b(quit|exit)\s*\(|Quickshell\s*\.\s*(reload|exit)|[\"'`](quit|exit)[\"'`]\s*\]", True),
    ("Qt.resolvedUrl or Qt.callLater", r"Qt\s*\.\s*(resolvedUrl|callLater)", True),
    ("URL scheme in an import", r"import\s+[\"']\s*(file|https?|ftp|qrc|data|wss?):", True),
    ("keyboard input items", r"\b(TextInput|TextEdit|TextField|TextArea|Label|Keys|forceActiveFocus|activeFocusOnTab|focusPolicy)\b|\bfocus\s*:\s*true", True),
    ("colour built outside the theme adapter", r"Qt\s*\.\s*(rgba|hsla|hsva|color|tint|lighter|darker)\s*\(", False),
    ("hex colour literal outside the theme adapter", r"#[0-9a-fA-F]{3,8}\b", False),
    ("named colour outside the theme adapter", r"\b(color|strokeColor|fillColor)\s*:\s*[\"'`](?!transparent[\"'`])[^\"'`]*[\"'`]", False),
    ("qs.Commons imported outside the theme adapter", r"^\s*import\s+qs\.", False),
]
# canvas surface rules: only these values, anywhere
CANVAS = [
    ("keyboardFocus other than WlrKeyboardFocus.None", r"keyboardFocus\s*:\s*(?!\s)(?!WlrKeyboardFocus\.None\b)"),
    ("exclusionMode other than ExclusionMode.Ignore", r"exclusionMode\s*:\s*(?!\s)(?!ExclusionMode\.Ignore\b)"),
    ("layer other than WlrLayer.Bottom", r"WlrLayershell\s*\.\s*layer\s*:\s*(?!\s)(?!WlrLayer\.Bottom\b)"),
    ("layer-shell layer used outside the canvas and Center.qml", r"\bWlrLayer\.(Overlay|Top|Background)\b"),
]
# The Widget Center overlay is the one place that may take the keyboard and sit on the Overlay layer
# (WIDGET-CENTER-V0). Narrow, per file, and each exemption has a planted-violation self-test.
CENTER_SURFACE = [
    ("keyboardFocus other than WlrKeyboardFocus.OnDemand", r"keyboardFocus\s*:\s*(?!\s)(?!WlrKeyboardFocus\.OnDemand\b)"),
    ("exclusionMode other than ExclusionMode.Ignore", r"exclusionMode\s*:\s*(?!\s)(?!ExclusionMode\.Ignore\b)"),
    ("layer other than WlrLayer.Overlay", r"WlrLayershell\s*\.\s*layer\s*:\s*(?!\s)(?!WlrLayer\.Overlay\b)"),
]
KEYBOARD_RULE = "keyboard input items"
KEYBOARD_OK = {"CenterPanel.qml": r"\b(Keys|focus)\b"}     # only Escape handling; no text input anywhere
ESCAPE_ONLY = re.compile(r"\s*\.\s*onEscapePressed\s*:")  # the one Keys handler CenterPanel may have
CENTER_FILES = {"Center.qml", "CenterState.qml", "CenterPanel.qml", "CenterModel.js"}
# What the Widget Center may not touch: anything that writes, sends, copies, rescans or starts something.
CENTER_FORBIDDEN = r"clipboardText|setText\s*\(|\.write\s*\(|\.flush\s*\(|\.send\b|\brescan\w*|\bgeneration\b|\.connected\s*=(?!=)|\bFileView\b|\bSocket\b|\bSocketServer\b|execDetached"
# Presence, not just value: each surface line must exist exactly once, the window exactly once.
REQUIRED = {
    "Center.qml": [
        ("PanelWindow instances", r"(?<![\w.])PanelWindow\s*\{"),
        ("WlrLayershell.layer: WlrLayer.Overlay", r"WlrLayershell\s*\.\s*layer\s*:\s*WlrLayer\.Overlay\b"),
        ("WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand", r"WlrLayershell\s*\.\s*keyboardFocus\s*:\s*WlrKeyboardFocus\.OnDemand\b"),
        ("exclusionMode: ExclusionMode.Ignore", r"exclusionMode\s*:\s*ExclusionMode\.Ignore\b"),
        ("onCloseRequested: root.hide()", r"onCloseRequested\s*:\s*root\.hide\(\)"),
        ("active: root.open", r"\bactive\s*:\s*root\.open\b"),
    ],
    "DesktopCanvas.qml": [
        ("PanelWindow instances", r"(?<![\w.])PanelWindow\s*\{"),
        ("WlrLayershell.layer: WlrLayer.Bottom", r"WlrLayershell\s*\.\s*layer\s*:\s*WlrLayer\.Bottom\b"),
        ("WlrLayershell.keyboardFocus: WlrKeyboardFocus.None", r"WlrLayershell\s*\.\s*keyboardFocus\s*:\s*WlrKeyboardFocus\.None\b"),
        ("exclusionMode: ExclusionMode.Ignore", r"exclusionMode\s*:\s*ExclusionMode\.Ignore\b"),
    ],
    "CenterState.qml": [
        ("rows computed only while open", r"\brows\s*:\s*open\s*\?"),
    ],
}
SURFACE_WRITES = [
    ("surface property assigned instead of declared", r"WlrLayershell\s*\.\s*\w+\s*=(?!=)|\.\s*(layer|keyboardFocus|exclusionMode|namespace)\s*=(?!=)"),
    ("surface property set by computed access or Binding", r"\[\s*[\"'`](layer|keyboardFocus|exclusionMode|namespace)[\"'`]\s*\]|property\s*:\s*[\"'`](layer|keyboardFocus|exclusionMode|namespace)[\"'`]"),
    ("clipboard access", r"clipboardText"),
    ("setText call", r"\bsetText\s*\("),
]
ONLY_IN = {   # type -> files that may instantiate it
    "FileView": {"Store.qml", "Service.qml"},
    "FolderListModel": {"Store.qml"},
    "Socket": {"Bridge.qml"},
    "PanelWindow": {"DesktopCanvas.qml", "Center.qml"},
    "FloatingWindow": set(), "PopupWindow": set(), "WlSessionLock": set(), "Window": set(), "ApplicationWindow": set(),
}
ALLOWED_IMPORTS = {"QtQuick", "QtQuick.Layouts", "QtQuick.Shapes", "Quickshell", "Quickshell.Io",
                   "Quickshell.Wayland", "Qt.labs.folderlistmodel"}
LOADER_OK = re.compile(r"source\s*:\s*\"LayoutNode\.qml\"\s*(?=[,;\n}])")
TEXT_TYPES = ["Text"]


def check_text(name, text):
    """Return a list of 'file:line: rule: excerpt' strings."""
    base = os.path.basename(name)
    is_theme = base in THEME
    clean = strip_comments(text)
    found = []

    def add(pos, rule, ex=""):
        found.append("%s:%d: %s%s" % (name, line(clean, pos), rule, (": " + ex.strip()[:40]) if ex else ""))

    for rule, rx, in_theme in RX:
        if is_theme and not in_theme:
            continue
        for m in re.finditer(rx, clean, re.M):
            if rule == KEYBOARD_RULE and base in KEYBOARD_OK and re.fullmatch(KEYBOARD_OK[base], m.group(0).replace("focus: true", "focus")):
                continue
            add(m.start(), rule, m.group(0))
    # a Loader (or anything) `source:` may only be the one constant file, followed by a delimiter
    for m in re.finditer(r"(?<![\w.])source\s*:", clean):
        if not LOADER_OK.match(clean, m.start()):
            add(m.start(), "Loader/Image source other than the one constant file", clean[m.start():m.start() + 30])
    # URL-looking strings outside the store (FolderListModel needs a file: URL there)
    if base != "Store.qml":
        for m in re.finditer(r"[\"'`]\s*(file|https?|ftp|qrc|data|wss?):", clean):
            add(m.start(), "URL scheme in a string", m.group(0))
    for rule, rx in (CENTER_SURFACE if base == "Center.qml" else CANVAS):
        for m in re.finditer(rx, clean):
            if rule.startswith("layer-shell layer used") and base == "Center.qml":
                continue
            add(m.start(), rule, clean[m.start():m.start() + 50])
    for t, allowed in ONLY_IN.items():
        for m in re.finditer(r"(?<![\w.])%s\s*\{" % t, clean):
            if base not in allowed:
                add(m.start(), "%s instantiated outside %s" % (t, sorted(allowed) or "any plugin file"))
    # every Text item must set PlainText ITSELF (the default is AutoText, which renders HTML)
    if base.endswith(".qml"):
        for t, ln, own in items(clean, TEXT_TYPES):
            m = re.search(r"\btextFormat\s*:\s*(\S+)", own)
            if not m or not re.match(r"Text\.PlainText\b", m.group(1)):
                found.append("%s:%d: Text item without textFormat: Text.PlainText of its own%s" % (name, ln, (" (has " + m.group(1)[:20] + ")") if m else ""))
        for m in re.finditer(r"textFormat\s*:\s*(?!\s)(?!Text\.PlainText\b)", clean):
            add(m.start(), "textFormat other than PlainText", m.group(0))
        for m in re.finditer(r"^\s*import\s+([A-Za-z0-9_.]+)", clean, re.M):
            mod = m.group(1)
            if mod in ALLOWED_IMPORTS or (mod == "qs.Commons" and is_theme) or mod.startswith("qs."):
                continue
            add(m.start(), "import of an unlisted module", mod)
        for m in re.finditer(r"^\s*import\s+\"([^\"]*)\"", clean, re.M):
            if not re.fullmatch(r"[A-Za-z]+\.js", m.group(1)):
                add(m.start(), "import of a path other than a sibling .js", m.group(1))
    if base.endswith(".js"):
        for m in re.finditer(r"^\s*\.import\s+\"([^\"]*)\"", clean, re.M):
            if not re.fullmatch(r"[A-Za-z]+\.js", m.group(1)):
                add(m.start(), ".import of something other than a sibling .js", m.group(1))
    # ---- presence rules and write rules
    for rule, rx in SURFACE_WRITES:
        for m in re.finditer(rx, clean):
            add(m.start(), rule, m.group(0))
    if base != "Bridge.qml":
        for m in re.finditer(r"\.\s*write\s*\(", clean):
            add(m.start(), "socket or file write outside Bridge.qml", m.group(0))
    if base in CENTER_FILES:
        for m in re.finditer(CENTER_FORBIDDEN, clean):
            add(m.start(), "Widget Center touching a write, send, rescan or process path", m.group(0))
    if base not in ("Center.qml", "DesktopCanvas.qml"):
        for m in re.finditer(r"\bWlrLayershell\b", clean):
            add(m.start(), "WlrLayershell used outside the two surface files", m.group(0))
    else:
        for m in re.finditer(r"WlrLayershell\s*\.\s*(\w+)", clean):
            if m.group(1) not in ("namespace", "layer", "keyboardFocus"):
                add(m.start(), "unexpected WlrLayershell property", m.group(0))
    if base == "CenterPanel.qml":
        for m in re.finditer(r"\bKeys\b", clean):
            if not ESCAPE_ONLY.match(clean, m.end()):
                add(m.start(), "keyboard input items: CenterPanel may only handle Escape (Keys.onEscapePressed)")
    for rule, rx in REQUIRED.get(base, []):
        n = len(re.findall(rx, clean))
        if n != 1:
            found.append("%s:1: required line missing or repeated (%d, need exactly 1): %s" % (name, n, rule))
    return found


def scan(folder=PLUGIN):
    problems, files = [], 0
    for f in sorted(os.listdir(folder)):
        if f.endswith((".qml", ".js")):
            files += 1
            problems += check_text(f, open(os.path.join(folder, f), encoding="utf-8").read())
    return files, problems


# (rule fragment that must appear in the report, file name, snippet)
BAD = {
    "createQmlObject": ("dynamic QML object", "x.qml", "Item { Component.onCompleted: Qt.createQmlObject(widget.code, this) }"),
    "createComponent": ("dynamic QML object", "x.js", "var c = Qt.createComponent(url)"),
    "Qt.include": ("dynamic QML object", "x.js", "Qt.include(\"x.js\")"),
    "eval": ("eval", "x.js", "var x = eval(s)"),
    "Function constructor": ("eval", "x.js", "var f = new Function(\"return 1\")"),
    "Function call": ("eval", "x.js", "var f = Function(\"return 1\")"),
    ".constructor(": ("eval", "x.js", "var f = (function(){}).constructor(\"return process\")"),
    "computed Qt": ("computed member", "x.js", "Qt[\"quit\"]()"),
    "computed this": ("computed member", "x.js", "this['eval'](s)"),
    "computed globalThis": ("computed member", "x.js", "globalThis[\"Fun\" + \"ction\"]"),
    "setSource": ("source set", "x.qml", "Item { Component.onCompleted: loader.setSource(node.url) }"),
    ".source =": ("source set", "x.qml", "Item { Component.onCompleted: loader.source = node.url }"),
    "[\"source\"]": ("source set", "x.qml", "Item { Component.onCompleted: loader[\"source\"] = u }"),
    "Binding source": ("source set", "x.qml", "Binding { target: l; property: \"source\"; value: v }"),
    "Loader source variable": ("constant file", "x.qml", "Loader { source: node.file }"),
    "Loader source url": ("constant file", "x.qml", "Loader { source: \"file:///etc/x.qml\" }"),
    "Loader source concat": ("constant file", "x.qml", "Loader { source: \"LayoutNode.qml\" + x }"),
    "Loader source other file": ("constant file", "x.qml", "Loader { source: \"Other.qml\" }"),
    "RichText": ("rich text", "x.qml", "Text { textFormat: Text.RichText }"),
    "StyledText": ("rich text", "x.qml", "Text { textFormat: Text.StyledText }"),
    "AutoText": ("rich text", "x.qml", "Text { textFormat: Text.AutoText }"),
    "MarkdownText": ("rich text", "x.qml", "Text { textFormat: Text.MarkdownText }"),
    "textFormat variable": ("textFormat other", "x.qml", "Text { textFormat: fmt }"),
    "Text without format": ("Text item without", "x.qml", "Text { text: x }"),
    "Text next to a good one": ("Text item without", "x.qml", "Item { Text { textFormat: Text.PlainText } Text { text: y } }"),
    "Text child inherits nothing": ("Text item without", "x.qml", "Text { text: a; Item { property var q: { textFormat: Text.PlainText } } }"),
    "TextInput": ("keyboard input", "x.qml", "TextInput { }"),
    "TextEdit": ("keyboard input", "x.qml", "TextEdit { }"),
    "Keys": ("keyboard input", "x.qml", "Item { Keys.onPressed: x }"),
    "focus true": ("keyboard input", "x.qml", "Item { focus: true }"),
    "Image": ("image", "x.qml", "Image { source: x }"),
    "IconImage": ("image", "x.qml", "IconImage { }"),
    "iconPath": ("image", "x.qml", "Item { property string p: Quickshell.iconPath(x) }"),
    "image://": ("image", "x.js", "var u = \"image://icon/x\""),
    "Process": ("process", "x.qml", "Process { command: [\"sh\"] }"),
    "execDetached": ("process", "x.js", "Quickshell.execDetached([\"x\"])"),
    "XHR": ("process", "x.js", "var x = new XMLHttpRequest()"),
    "Qt.quit": ("quitting", "x.qml", "Item { Component.onCompleted: Qt.quit() }"),
    "Qt.quit split": ("quitting", "x.qml", "Item { Component.onCompleted: Qt\n.quit() }"),
    "Qt.quit alias": ("quitting", "x.js", "var q = Qt.quit; q()"),
    "Qt.exit": ("quitting", "x.js", "Qt.exit(0)"),
    "bare quit": ("quitting", "x.js", "var f = shell.quit(); "),
    "Qt['quit']": ("quitting", "x.js", "Qt['quit']()"),
    "Quickshell.reload": ("quitting", "x.qml", "Item { Component.onCompleted: Quickshell.reload(true) }"),
    "resolvedUrl": ("resolvedUrl", "x.js", "var u = Qt.resolvedUrl(p)"),
    "callLater quit": ("resolvedUrl", "x.js", "Qt.callLater(Qt.quit)"),
    "import file url": ("URL scheme", "x.qml", "import \"file:///etc/evil\""),
    ".import http": ("URL scheme", "x.js", ".import \"http://x/y.js\" as X"),
    "string file url": ("URL scheme in a string", "Tile.qml", "Item { property string u: \"file:///etc/passwd\" }"),
    "string http url": ("URL scheme in a string", "x.js", "var u = 'https://evil.example/'"),
    "hex colour": ("hex colour", "x.qml", "Rectangle { color: \"#ff0000\" }"),
    "short hex": ("hex colour", "x.qml", "Rectangle { color: \"#f00\" }"),
    "named colour": ("named colour", "x.qml", "Rectangle { color: \"red\" }"),
    "Qt.rgba": ("colour built", "x.qml", "Rectangle { color: Qt.rgba(1, 0, 0, 1) }"),
    "qs import": ("qs.Commons", "x.qml", "import qs.Commons"),
    "unknown import": ("unlisted module", "x.qml", "import QtWebEngine"),
    "path import": ("path other", "x.qml", "import \"../other\""),
    "js .import path": (".import of", "x.js", ".import \"../x.js\" as X"),
    "FileView outside store": ("FileView instantiated", "Tile.qml", "Item { FileView { path: \"/etc/passwd\" } }"),
    "Socket outside bridge": ("Socket instantiated", "Tile.qml", "Item { Socket { path: x } }"),
    "FolderListModel outside store": ("FolderListModel instantiated", "Service.qml", "Item { FolderListModel { } }"),
    "PanelWindow outside canvas": ("PanelWindow instantiated", "Tile.qml", "PanelWindow { }"),
    "FloatingWindow": ("FloatingWindow instantiated", "DesktopCanvas.qml", "FloatingWindow { }"),
    "keyboard exclusive": ("keyboardFocus", "DesktopCanvas.qml", "PanelWindow { WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive }"),
    "keyboard on demand": ("keyboardFocus", "DesktopCanvas.qml", "PanelWindow { WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand }"),
    "keyboard variable": ("keyboardFocus", "DesktopCanvas.qml", "PanelWindow { WlrLayershell.keyboardFocus: mode }"),
    "exclusion normal": ("exclusionMode", "DesktopCanvas.qml", "PanelWindow { exclusionMode: ExclusionMode.Normal }"),
    "exclusion auto": ("exclusionMode", "DesktopCanvas.qml", "PanelWindow { exclusionMode: ExclusionMode.Auto }"),
    "layer overlay": ("layer", "DesktopCanvas.qml", "PanelWindow { WlrLayershell.layer: WlrLayer.Overlay }"),
    "layer top": ("layer", "DesktopCanvas.qml", "PanelWindow { WlrLayershell.layer: WlrLayer.Top }"),
    "layer variable": ("layer", "DesktopCanvas.qml", "PanelWindow { WlrLayershell.layer: lyr }"),
    "center keyboard exclusive": ("keyboardFocus", "Center.qml", "PanelWindow { WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive }"),
    "center keyboard none is also pinned": ("keyboardFocus", "Center.qml", "PanelWindow { WlrLayershell.keyboardFocus: WlrKeyboardFocus.None }"),
    "center layer top": ("layer", "Center.qml", "PanelWindow { WlrLayershell.layer: WlrLayer.Top }"),
    "center exclusion": ("exclusionMode", "Center.qml", "PanelWindow { exclusionMode: ExclusionMode.Normal }"),
    "canvas on demand": ("keyboardFocus", "DesktopCanvas.qml", "PanelWindow { WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand }"),
    "keys in the tile": ("keyboard input", "Tile.qml", "Item { Keys.onEscapePressed: x }"),
    "keys in center.qml": ("keyboard input", "Center.qml", "Item { Keys.onEscapePressed: x }"),
    "text input in the panel": ("keyboard input", "CenterPanel.qml", "Item { TextInput { } }"),
    "focus policy in the panel": ("keyboard input", "CenterPanel.qml", "Item { activeFocusOnTab: true }"),
    "process in the panel": ("process", "CenterPanel.qml", "Item { Process { command: [\"ow\", \"remove\", id] } }"),
    "file view in the panel": ("FileView instantiated", "CenterPanel.qml", "Item { FileView { path: x } }"),
    "regex with a double quote hides eval": ("eval", "x.js", "var r = /\"/; var x = eval(s)"),
    "regex with a single quote hides eval": ("eval", "x.js", "var r = /'/g; var x = eval(s)"),
    "regex with a quote after return": ("eval", "x.js", "function f(s) { return /\"/.test(s) && eval(s) }"),
    "regex with a quote hides a Process": ("process", "x.qml", "Item { property var r: /'/; Process { command: [\"sh\"] } }"),
    "write outside bridge": ("write outside", "Tile.qml", "Item { Component.onCompleted: s.write(\"x\") }"),
    "setText anywhere": ("setText", "Tile.qml", "Item { Component.onCompleted: f.setText(\"x\") }"),
    "clipboard anywhere": ("clipboard", "Tile.qml", "Item { Component.onCompleted: Quickshell.clipboardText = \"x\" }"),
    "FileView in Center.qml": ("FileView instantiated", "Center.qml", "Item { FileView { path: \"/etc/passwd\" } }"),
    "keys other than escape in panel": ("only handle Escape", "CenterPanel.qml", "Item { focus: true; Keys.onPressed: x }"),
    "overlay layer elsewhere": ("layer-shell layer used", "Service.qml", "Item { property var l: WlrLayer.Overlay }"),
}
GOOD = {
    "plain text": ("x.qml", "Text { textFormat: Text.PlainText; text: \"x\" }"),
    "transparent": ("x.qml", "Rectangle { color: \"transparent\" }"),
    "comment mentions": ("x.qml", "// Qt.quit() and RichText and #ff0000 and Image and file:///x are forbidden\nItem {}"),
    "block comment": ("x.qml", "/* eval( Qt.createQmlObject( Qt.quit */ Item {}"),
    "constant loader": ("x.qml", "Loader { source: \"LayoutNode.qml\" }"),
    "constant loader newline": ("x.qml", "Loader {\n  source: \"LayoutNode.qml\"\n  onLoaded: x\n}"),
    "sourceComponent": ("x.qml", "Loader { sourceComponent: foo }"),
    "role colour": ("x.qml", "Rectangle { color: Theme.accent }"),
    "two plain texts": ("x.qml", "Item { Text { textFormat: Text.PlainText } Text { textFormat: Text.PlainText } }"),
    "text with nested item": ("x.qml", "Text { textFormat: Text.PlainText; Item { } }"),
    "canvas ok": ("DesktopCanvas.qml", "PanelWindow { exclusionMode: ExclusionMode.Ignore; WlrLayershell.layer: WlrLayer.Bottom; WlrLayershell.keyboardFocus: WlrKeyboardFocus.None }"),
    "store file urls": ("Store.qml", "Item { FileView { path: x } FolderListModel { folder: \"file://\" + x } }"),
    "regexes and divisions": ("x.js", "var r = /\"/g, s = a / 2, t = b / 3; var u = /[/\"]/.test(s) // eval("),
    "panel escape": ("CenterPanel.qml", "Item { focus: true; Keys.onEscapePressed: panel.closeRequested() }"),
    "bridge socket": ("Bridge.qml", "Item { Socket { path: x } }"),
    "word quit in prose string": ("x.qml", "Item { property string s: \"do not quit\" }"),
}


def real(name):
    return open(os.path.join(PLUGIN, name), encoding="utf-8").read()


def drop(rx):
    return lambda t: re.sub(rx, "", t, count=1)


def add_to_window(extra):
    return lambda t: t.replace("PanelWindow {", "PanelWindow {\n      " + extra, 1)


# Mutations of the real surface files: each must be reported (the audit's surviving mutants).
MUTATIONS = {
    "center: layer line deleted": ("required line", "Center.qml", drop(r"WlrLayershell\.layer: WlrLayer\.Overlay")),
    "center: keyboard line deleted": ("required line", "Center.qml", drop(r"WlrLayershell\.keyboardFocus: [\w.]+")),
    "center: exclusion line deleted": ("required line", "Center.qml", drop(r"exclusionMode: [\w.]+")),
    "center: keyboard assigned in a handler": ("assigned", "Center.qml", add_to_window("Component.onCompleted: WlrLayershell.keyboardFocus = WlrKeyboardFocus.Exclusive")),
    "center: layer assigned in a handler": ("assigned", "Center.qml", add_to_window("Component.onCompleted: WlrLayershell.layer = WlrLayer.Top")),
    "center: close wiring deleted": ("required line", "Center.qml", drop(r"onCloseRequested: root\.hide\(\)")),
    "center: window always loaded": ("required line", "Center.qml", lambda t: t.replace("active: root.open", "active: true")),
    "center: second window without a layer": ("required line", "Center.qml", lambda t: t.replace("  LazyLoader {", "  PanelWindow { visible: true }\n  LazyLoader {", 1)),
    "center: second window with a layer": ("required line", "Center.qml", lambda t: t.rstrip()[:-1] + "  PanelWindow { WlrLayershell.layer: WlrLayer.Overlay }\n}\n"),
    "center: binding on the surface": ("computed access or Binding", "Center.qml", add_to_window("Binding { target: win; property: \"keyboardFocus\"; value: 1 }")),
    "center: clipboard": ("clipboard", "Center.qml", add_to_window("Component.onCompleted: Quickshell.clipboardText = \"x\"")),
    "center: file view": ("FileView", "Center.qml", add_to_window("FileView { path: \"/tmp/x\" }")),
    "center: setText": ("setText", "CenterState.qml", lambda t: t.replace("function hide()", "function hide() { f.setText(\"x\") }\n  function hide2()", 1)),
    "state: bridge send": ("write, send", "CenterState.qml", lambda t: t.replace("function hide()", "function poke() { service.bridge.send(\"x\") }\n  function hide()", 1)),
    "state: store rescan": ("write, send", "CenterState.qml", lambda t: t.replace("function hide()", "function poke() { store.rescan() }\n  function hide()", 1)),
    "state: rows always computed": ("required line", "CenterState.qml", lambda t: t.replace("rows: open ?", "rows: true ?", 1)),
    "panel: other key handler": ("CenterPanel may only", "CenterPanel.qml", lambda t: t.replace("Keys.onEscapePressed: panel.closeRequested()", "Keys.onEscapePressed: panel.closeRequested()\n  Keys.onReturnPressed: x", 1)),
    "panel: socket write": ("write", "CenterModel.js", lambda t: t + "\nfunction go(s) { s.write(\"x\") }\n"),
    "canvas: layer line deleted": ("required line", "DesktopCanvas.qml", drop(r"WlrLayershell\.layer: WlrLayer\.Bottom")),
    "canvas: keyboard line deleted": ("required line", "DesktopCanvas.qml", drop(r"WlrLayershell\.keyboardFocus: [\w.]+")),
    "canvas: keyboard assigned": ("assigned", "DesktopCanvas.qml", add_to_window("Component.onCompleted: WlrLayershell.keyboardFocus = WlrKeyboardFocus.Exclusive")),
    "canvas: second window": ("required line", "DesktopCanvas.qml", lambda t: t.rstrip()[:-1] + "  PanelWindow { }\n}\n"),
    "canvas: other file touching WlrLayershell": ("outside the two surface", "Tile.qml", lambda t: t + "\nItem { Component.onCompleted: WlrLayershell.layer = 1 }\n"),
}


def selftest():
    bad = 0
    for name, (needle, fname, fn) in MUTATIONS.items():
        base = real(fname)
        if check_text(fname, base):
            print("FAIL selftest: the real %s is not clean, mutation '%s' proves nothing" % (fname, name)); bad += 1
            continue
        mutated = fn(base)
        if mutated == base:
            print("FAIL selftest: mutation '%s' did not change %s" % (name, fname)); bad += 1
            continue
        r = check_text(fname, mutated)
        if not any(needle.lower() in x.lower() for x in r):
            print("FAIL selftest: mutation '%s' was not caught (got %s)" % (name, r)); bad += 1
    for name, (needle, fname, snippet) in BAD.items():
        r = check_text(fname, snippet)
        if not any(needle.lower() in x.lower() for x in r):
            print("FAIL selftest: rule '%s' did not fire for %s (got %s)" % (needle, name, r)); bad += 1
    for name, (fname, snippet) in GOOD.items():
        r = check_text(fname, snippet)
        if r:
            print("FAIL selftest: false positive for", name, r); bad += 1
    if check_text("Theme.qml", "import qs.Commons as Commons\nQtObject { property color c: \"#cacccc\"; property color d: Qt.rgba(0,0,0,1) }"):
        print("FAIL selftest: Theme.qml should be allowed hex, Qt.rgba and qs.Commons"); bad += 1
    if not check_text("Theme.qml", "Item { Component.onCompleted: Qt.quit() }"):
        print("FAIL selftest: Theme.qml is not exempt from the other rules"); bad += 1
    print("selftest: %d mutations of the real files, %d bad snippets, %d good snippets, %s" % (len(MUTATIONS), len(BAD), len(GOOD), "FAILED" if bad else "ok"))
    return 1 if bad else 0


if __name__ == "__main__":
    if "--selftest" in sys.argv:
        sys.exit(selftest())
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    files, problems = scan(args[0] if args else PLUGIN)
    if files == 0:
        print("static check: no .qml or .js files found, refusing to call that clean"); sys.exit(1)
    for p in problems:
        print("VIOLATION", p)
    print("static check: %d files scanned, %d violations" % (files, len(problems)))
    sys.exit(1 if problems else 0)
