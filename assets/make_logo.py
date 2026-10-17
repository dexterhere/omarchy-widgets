#!/usr/bin/env python3
"""Generate the wordmark SVGs. Original pixel lettering drawn for this project, in the same
blocky, stepped-pixel style as the Omarchy wordmark; no Omarchy artwork is copied.
Run: python3 assets/make_logo.py  (writes assets/logo-light.svg and assets/logo-dark.svg)"""
import pathlib

U = 40   # one bitmap pixel in SVG units: heavy 40-unit strokes like the Omarchy wordmark
GAP = 20  # letters sit close together, as in the Omarchy wordmark
GROW = 6  # each pixel is grown by this much on every side: heavier strokes, no hairline seams
G = {  # 5 wide; rows 0-1 ascender, 2-6 x-height, 7-8 descender
 "w": {2:"X...X",3:"X...X",4:"X.X.X",5:"XX.XX",6:"X...X"},
 "i": {0:"..X..",2:".XX..",3:"..X..",4:"..X..",5:"..X..",6:".XXX."},
 "d": {0:"....X",1:"....X",2:".XXXX",3:"X...X",4:"X...X",5:"X...X",6:".XXXX"},
 "g": {2:".XXXX",3:"X...X",4:"X...X",5:"X...X",6:".XXXX",7:"....X",8:".XXX."},
 "e": {2:".XXX.",3:"X...X",4:"XXXXX",5:"X....",6:".XXXX"},
 "t": {1:"..X..",2:"XXXXX",3:"..X..",4:"..X..",5:"..X..",6:"...XX"},
 "s": {2:".XXXX",3:"X....",4:".XXX.",5:"....X",6:"XXXX."},
 "f": {0:"..XX.",1:".X...",2:"XXXX.",3:".X...",4:".X...",5:".X...",6:".X..."},
 "o": {2:".XXX.",3:"X...X",4:"X...X",5:"X...X",6:".XXX."},
 "r": {2:"X.XX.",3:"XX..X",4:"X....",5:"X....",6:"X...."},
 "m": {2:"XXXX.",3:"X.X.X",4:"X.X.X",5:"X.X.X",6:"X.X.X"},
 "a": {2:".XXX.",3:"....X",4:".XXXX",5:"X...X",6:".XXXX"},
 "c": {2:".XXXX",3:"X....",4:"X....",5:"X....",6:".XXXX"},
 "h": {0:"X....",1:"X....",2:"X.XX.",3:"XX..X",4:"X...X",5:"X...X",6:"X...X"},
 "y": {2:"X...X",3:"X...X",4:"X...X",5:".XXXX",6:"....X",7:"....X",8:".XXX."},
}

def rects(text, x0, y0, u, gap=GAP):
    """Merge horizontal runs into rectangles."""
    out, x = [], x0
    for ch in text:
        if ch == " ":
            x += 4 * u
            continue
        for r, row in G[ch].items():
            c = 0
            while c < 5:
                if row[c] == "X":
                    s = c
                    while c < 5 and row[c] == "X":
                        c += 1
                    out.append((x + s * u, y0 + r * u, (c - s) * u, u))
                else:
                    c += 1
        x += 5 * u + gap
    return out

def svg(fill):
    big = rects("widgets", 0, 0, U)
    w_big = 7 * (5 * U + GAP) - GAP
    # icon: 2x2 chunky blocks (a widget grid), same height as the x-height band plus ascender
    gx, gu = w_big + 3 * U, U
    blocks = [(0, 0), (6, 0), (0, 6), (6, 6)]
    icon = [(gx + cx * gu, cy * gu, 5 * gu, 5 * gu) for cx, cy in blocks]
    width = gx + 11 * gu + GROW
    height = 11 * gu
    grown = [(x - GROW, y - GROW, w + 2 * GROW, h + 2 * GROW) for x, y, w, h in big]
    body = "".join(f'<rect x="{x}" y="{y}" width="{w}" height="{h}"/>' for x, y, w, h in grown + icon)
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{-GROW} {-GROW} {width + GROW} {height + 2 * GROW}" fill="{fill}" '
            f'role="img" aria-label="Omarchy Widgets"><title>Omarchy Widgets</title>{body}</svg>\n')

here = pathlib.Path(__file__).parent
(here / "logo-light.svg").write_text(svg("#000000"))   # for light backgrounds
(here / "logo-dark.svg").write_text(svg("#ffffff"))    # for dark backgrounds
print("wrote", here / "logo-light.svg", here / "logo-dark.svg")
