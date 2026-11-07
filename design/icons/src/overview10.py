# Round 10 contact sheet (same layout as overview5.py, without "was" panels)
from lib import *

names = ["r10-65-neutral-flat", "r10-66-neutral-lit", "r10-67-neutral-outline", "r10-68-neutral-round-eyes", "r10-69-neutral-soft", "r10-70-neutral-soft-oval", "r10-71-neutral-soft-shallow", "r10-72-neutral-soft-flatter", "r10-73-neutral-soft-mid", "r10-74-neutral-soft-35", "r10-75-neutral-wave", "r10-76-neutral-even", "r10-77-neutral-even-round"]
CW, CH, TILE = 480, 700, 390
w, h = len(names) * CW, CH
s = (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}">'
     f'<rect width="{w}" height="{h}" fill="#8e8f93"/>')
for i, n in enumerate(names):
    x, y = i * CW + (CW - TILE) // 2, 20
    s += f'<image x="{x}" y="{y}" width="{TILE}" height="{TILE}" xlink:href="{n}.png"/>'
    s += f'<image x="{x + 50}" y="{y + 400}" width="128" height="128" image-rendering="optimizeSpeed" xlink:href="{n}-64.png"/>'
    s += f'<image x="{x + 230}" y="{y + 432}" width="64" height="64" image-rendering="optimizeSpeed" xlink:href="{n}-32.png"/>'
    num, name = n.split("-")[1], n.split("-", 2)[2]
    s += (f'<text x="{x + TILE // 2}" y="{y + 596}" text-anchor="middle" font-family="Helvetica Neue, Arial" '
          f'font-weight="bold" font-size="60" fill="#111">{num}</text>'
          f'<text x="{x + TILE // 2}" y="{y + 648}" text-anchor="middle" font-family="Helvetica Neue, Arial" '
          f'font-size="48" fill="#111">{name}</text>')
s += "</svg>"
p = os.path.join(OUT, "overview-round10.svg")
open(p, "w").write(s)
subprocess.run([RENDER, p, os.path.join(OUT, "overview-round10.png"), str(w)], check=True)
