#!/usr/bin/env python3
"""Static safety check over the plugin's own QML and JS (everything outside tests/).

Nothing a widget file contains may be executed, evaluated, loaded as a component or URL,
or drawn as rich text, and colours must come from the theme adapter. This greps for the
ways code could break that, and `--selftest` proves each rule fires on a planted violation.

  static_check.py            scan ../ (the plugin folder); exit 1 on any violation
  static_check.py --selftest check the rules against known-bad snippets
"""
import os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
PLUGIN = os.path.dirname(HERE)

# (name, regex, applies_to_theme_qml_too)
RULES = [
    ("dynamic QML object creation", r"createQmlObject|createComponent|Qt\.include|incubateObject", True),
    ("eval or Function constructor", r"\beval\s*\(|new\s+Function\b|(?<![\w.])Function\s*\(", True),
    ("Loader source set at run time", r"\bsetSource\b", True),
    ("Loader/Image source other than the one constant file", r"\bsource\s*:(?!\s*\"LayoutNode\.qml\")", True),
    ("rich text", r"\b(RichText|StyledText|AutoText|MarkdownText)\b", True),
    ("textFormat other than PlainText", r"textFormat\s*:\s*(?!\s)(?!Text\.PlainText\b)", True),
    ("image or shader types", r"\b(Image|AnimatedImage|BorderImage|ShaderEffect|ShaderEffectSource)\s*\{", True),
    ("process or network access", r"\b(Process|SocketServer)\s*\{|execDetached|XMLHttpRequest|WebSocket|Qt\.openUrlExternally|fetch\s*\(", True),
    ("quitting or reloading the shell", r"Qt\.quit|Qt\.exit|Quickshell\.reload|Quickshell\.exit", True),
    ("colour built outside the theme adapter", r"Qt\.(rgba|hsla|hsva|color|tint|lighter|darker)\s*\(", False),
    ("hex colour literal outside the theme adapter", r"#[0-9a-fA-F]{3,8}\b", False),
    ("named colour outside the theme adapter", r"\b(color|strokeColor|fillColor)\s*:\s*\"(?!transparent\")[^\"]*\"", False),
    ("qs.Commons imported outside the theme adapter", r"^\s*import\s+qs\.", False),
]
ALLOWED_IMPORTS = {"QtQuick", "QtQuick.Layouts", "QtQuick.Shapes", "Quickshell", "Quickshell.Io",
                   "Quickshell.Wayland", "Qt.labs.folderlistmodel"}


def strip_comments(text):
    """Remove // and /* */ comments but leave string literals alone."""
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if c in "\"'":
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


def check_text(name, text):
    """Return a list of 'file:line: rule' strings."""
    base = os.path.basename(name)
    is_theme = base == "Theme.qml"
    clean = strip_comments(text)
    found = []
    for rule, rx, in_theme in RULES:
        if is_theme and not in_theme:
            continue
        for m in re.finditer(rx, clean, re.M):
            found.append("%s:%d: %s: %s" % (name, clean.count("\n", 0, m.start()) + 1, rule, m.group(0).strip()[:40]))
    if base.endswith(".qml"):
        # a Text item defaults to AutoText, which renders HTML: every one must say PlainText
        texts = len(re.findall(r"(?<![\w.])Text\s*\{", clean))
        plain = len(re.findall(r"textFormat\s*:\s*Text\.PlainText\b", clean))
        if texts > plain:
            found.append("%s: %d Text items but only %d set textFormat: Text.PlainText" % (name, texts, plain))
        for m in re.finditer(r"^\s*import\s+([A-Za-z0-9_.]+)", clean, re.M):
            mod = m.group(1)
            if mod in ALLOWED_IMPORTS or (mod == "qs.Commons" and is_theme):
                continue
            if mod.startswith("qs."):
                continue  # already reported by the qs rule
            found.append("%s:%d: import of an unlisted module: %s" % (name, clean.count("\n", 0, m.start()) + 1, mod))
        for m in re.finditer(r"^\s*import\s+\"([^\"]*)\"(?:\s+as\s+\w+)?", clean, re.M):
            if not re.fullmatch(r"[A-Za-z]+\.js", m.group(1)):
                found.append("%s:%d: import of a path other than a sibling .js: %s" % (name, clean.count("\n", 0, m.start()) + 1, m.group(1)))
    if base.endswith(".js"):
        for m in re.finditer(r"^\s*\.import\s+\"([^\"]*)\"", clean, re.M):
            if not re.fullmatch(r"[A-Za-z]+\.js", m.group(1)):
                found.append("%s:%d: .import of something other than a sibling .js: %s" % (name, clean.count("\n", 0, m.start()) + 1, m.group(1)))
    return found


def scan():
    problems, files = [], 0
    for f in sorted(os.listdir(PLUGIN)):
        if f.endswith((".qml", ".js")):
            files += 1
            problems += check_text(f, open(os.path.join(PLUGIN, f), encoding="utf-8").read())
    return files, problems


BAD = {
    "createQmlObject": "Item { Component.onCompleted: Qt.createQmlObject(widget.code, this) }",
    "createComponent": "var c = Qt.createComponent(url)",
    "Qt.include": "Qt.include(\"x.js\")",
    "eval": "var x = eval(s)",
    "Function constructor": "var f = new Function(\"return 1\")",
    "setSource": "loader.setSource(node.url)",
    "Loader source": "Loader { source: node.file }",
    "Loader source url": "Loader { source: \"file:///etc/x.qml\" }",
    "RichText": "Text { textFormat: Text.RichText }",
    "StyledText": "Text { textFormat: Text.StyledText }",
    "AutoText": "Text { textFormat: Text.AutoText }",
    "MarkdownText": "Text { textFormat: Text.MarkdownText }",
    "textFormat variable": "Text { textFormat: fmt }",
    "Text without format": "Text { text: x }",
    "Image": "Image { source: x }",
    "Process": "Process { command: [\"sh\"] }",
    "execDetached": "Quickshell.execDetached([\"x\"])",
    "XHR": "var x = new XMLHttpRequest()",
    "Qt.quit": "Item { Component.onCompleted: Qt.quit() }",
    "hex colour": "Rectangle { color: \"#ff0000\" }",
    "short hex": "Rectangle { color: \"#f00\" }",
    "named colour": "Rectangle { color: \"red\" }",
    "Qt.rgba": "Rectangle { color: Qt.rgba(1, 0, 0, 1) }",
    "qs import": "import qs.Commons",
    "unknown import": "import QtWebEngine",
    "path import": "import \"../other\"",
}
GOOD = {
    "plain text": "Text { textFormat: Text.PlainText; text: \"x\" }",
    "transparent": "Rectangle { color: \"transparent\" }",
    "comment mentions": "// Qt.quit() and RichText and #ff0000 are forbidden\nItem {}",
    "block comment": "/* eval( Qt.createQmlObject( */ Item {}",
    "constant loader": "Loader { source: \"LayoutNode.qml\" }",
    "sourceComponent": "Loader { sourceComponent: foo }",
    "role colour": "Rectangle { color: Theme.accent }",
}


def selftest():
    bad = 0
    for name, snippet in BAD.items():
        if not check_text("bad.qml", snippet):
            print("FAIL selftest: rule did not fire for", name); bad += 1
    for name, snippet in GOOD.items():
        r = check_text("good.qml", snippet)
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
