# Picks sheet (../overview-picks.png): every pick with its number and name (main preview plus 64 and
# 32 px at 2x), grouped by theme, then each designed pair with the dark icon on a dark panel and the
# light one on a light panel. Icons, pairs and names come from app_icons.py.
from app_icons import ICONS, PAIRS, ICONS_DIR
from lib import *

SRC = {id_: os.path.join(ICONS_DIR, "picks", id_) for id_, *_ in ICONS}
NAMES = {id_: name for id_, name, _, _ in ICONS}
NUMS = {id_: str(num) for id_, _, _, num in ICONS}
ICON_ROWS = [[i for i, _, t, _ in ICONS if t == theme] for theme in ("plain", "persona-3-reload")]
P5 = [i for i, _, t, _ in ICONS if t == "persona-5"]
ICON_ROWS += [P5[:3], P5[3:]]
LIGHT_BG, DARK_BG, TILE, CW, CH = "#e8e8ea", "#1c1c1e", 256, 340, 470
FONT = 'font-family="Helvetica Neue, Arial"'

for n in SRC:
    for size in (TILE, 64, 32):
        subprocess.run([RENDER, SRC[n] + ".svg", os.path.join(OUT, f"final-{n}-{size}.png"), str(size)], check=True)


def tile(x, y, n, fg="#111"):
    return (f'<image x="{x}" y="{y}" width="{TILE}" height="{TILE}" xlink:href="final-{n}-{TILE}.png"/>'
            f'<image x="{x + 20}" y="{y + 270}" width="128" height="128" image-rendering="optimizeSpeed" xlink:href="final-{n}-64.png"/>'
            f'<image x="{x + 180}" y="{y + 302}" width="64" height="64" image-rendering="optimizeSpeed" xlink:href="final-{n}-32.png"/>'
            f'<text x="{x + TILE // 2}" y="{y + 440}" text-anchor="middle" {FONT} font-size="22" fill="{fg}">'
            f'<tspan font-weight="bold">{NUMS[n]}</tspan> {NAMES[n]}</text>')


cols = max(len(r) for r in ICON_ROWS)
w = max(cols * CW, 2 * (CW + 40)) + 40
y, s = 20, ""
s += f'<text x="20" y="{y + 40}" {FONT} font-weight="bold" font-size="44" fill="#111">Icons</text>'
y += 70
for row in ICON_ROWS:
    for i, n in enumerate(row):
        s += tile(20 + i * CW + (CW - TILE) // 2, y, n)
    y += CH
s += f'<text x="20" y="{y + 40}" {FONT} font-weight="bold" font-size="44" fill="#111">Pairs (dark / light)</text>'
y += 70
PW = 2 * CW + 40
for i, (dark, light) in enumerate(PAIRS):
    x = 20 + (i % 2) * (PW + 40)
    if i % 2 == 0 and i:
        y += CH + 80
    s += (f'<text x="{x}" y="{y + 34}" {FONT} font-weight="bold" font-size="30" fill="#111">'
          f'{NAMES[dark]} / {NAMES[light]}</text>')
    s += f'<rect x="{x}" y="{y + 50}" width="{CW}" height="{CH + 10}" rx="20" fill="{DARK_BG}"/>'
    s += f'<rect x="{x + CW}" y="{y + 50}" width="{CW}" height="{CH + 10}" rx="20" fill="{LIGHT_BG}"/>'
    s += tile(x + (CW - TILE) // 2, y + 60, dark, "#eee") + tile(x + CW + (CW - TILE) // 2, y + 60, light)
y += CH + 90
w = max(w, 2 * (PW + 40) + 20)
s = (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{y}">'
     f'<rect width="{w}" height="{y}" fill="#8e8f93"/>' + s + "</svg>")
p = os.path.join(OUT, "overview-picks.svg")
open(p, "w").write(s)
subprocess.run([RENDER, p, os.path.join(ICONS_DIR, "overview-picks.png"), str(w)], check=True)
