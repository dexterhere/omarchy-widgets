#!/usr/bin/env python3
"""Static safety check over the plugin's own QML and JS (everything outside tests/).

Nothing a widget file contains may be executed, evaluated, loaded as a component or URL,
or drawn as rich text; colours come from the theme adapter; the canvas never takes the
keyboard. This is a lint, not a proof: it makes the obvious and the sneaky spellings of
those mistakes fail CI. `--selftest` plants a violation for every rule and checks each fires.

  static_check.py            scan ../ (the plugin folder); exit 1 on any violation
  static_check.py --selftest check the rules against known-bad and known-good snippets
"""
import os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
PLUGIN = os.path.dirname(HERE)
THEME = {"Theme.qml"}


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
ONLY_IN = {   # type -> files that may instantiate it
    "FileView": {"Store.qml", "Service.qml"},
    "FolderListModel": {"Store.qml"},
    "Socket": {"Bridge.qml"},
    "PanelWindow": {"DesktopCanvas.qml"},
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
            add(m.start(), rule, m.group(0))
    # a Loader (or anything) `source:` may only be the one constant file, followed by a delimiter
    for m in re.finditer(r"(?<![\w.])source\s*:", clean):
        if not LOADER_OK.match(clean, m.start()):
            add(m.start(), "Loader/Image source other than the one constant file", clean[m.start():m.start() + 30])
    # URL-looking strings outside the store (FolderListModel needs a file: URL there)
    if base != "Store.qml":
        for m in re.finditer(r"[\"'`]\s*(file|https?|ftp|qrc|data|wss?):", clean):
            add(m.start(), "URL scheme in a string", m.group(0))
    for rule, rx in CANVAS:
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
    return found


def scan():
    problems, files = [], 0
    for f in sorted(os.listdir(PLUGIN)):
        if f.endswith((".qml", ".js")):
            files += 1
            problems += check_text(f, open(os.path.join(PLUGIN, f), encoding="utf-8").read())
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
    "bridge socket": ("Bridge.qml", "Item { Socket { path: x } }"),
    "word quit in prose string": ("x.qml", "Item { property string s: \"do not quit\" }"),
}


def selftest():
    bad = 0
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
    print("selftest: %d bad snippets, %d good snippets, %s" % (len(BAD), len(GOOD), "FAILED" if bad else "ok"))
    return 1 if bad else 0


if __name__ == "__main__":
    if "--selftest" in sys.argv:
        sys.exit(selftest())
    files, problems = scan()
    for p in problems:
        print("VIOLATION", p)
    print("static check: %d files scanned, %d violations" % (files, len(problems)))
    sys.exit(1 if problems else 0)
