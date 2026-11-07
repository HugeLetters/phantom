# Round 8 contact sheet (same layout as overview5.py). Each family starts a new row; the earlier versions it builds on come first,
# labelled "was NNx" in grey, then the new designs. Main preview 390 px, 64 and 32 px renders at 2x
# (nearest neighbour), number 60 px, name 48 px.
from lib import *

SHEETS = {
    "all": [
        (["r7-46-mid-mask-stars", "r7-48-flame-eyes-out", "r6-28-flame-eyes-stars"], ["r8-59-burning-eyes"]),
        (["r7-52-keyhole-big-eyes", "r7-53-keyhole-mask-white"], ["r8-61-keyhole-mask-eyes", "r8-62-keyhole-lens-eyes"]),
    ],
}
COLS, CW, CH, TILE = 6, 480, 700, 390

for sheet, families in SHEETS.items():
    tiles = []  # (name, label_num, label_name, previous?) with None marking a forced row break
    for prev, new in families:
        if tiles:
            tiles += [None] * ((-len(tiles)) % COLS)
        tiles += [(n, "was " + n.split("-")[1], "", True) for n in prev]
        tiles += [(n, n.split("-")[1], n.split("-", 2)[2], False) for n in new]
    rows = math.ceil(len(tiles) / COLS)
    w, h = COLS * CW, rows * CH
    s = (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}">'
         f'<rect width="{w}" height="{h}" fill="#8e8f93"/>')
    for i, t in enumerate(tiles):
        if t is None:
            continue
        n, num, name, prev = t
        x, y = (i % COLS) * CW + (CW - TILE) // 2, (i // COLS) * CH + 20
        if prev:
            s += f'<rect x="{x - 12}" y="{y - 12}" width="{TILE + 24}" height="{CH - 16}" rx="18" fill="#7a7b80"/>'
        s += f'<image x="{x}" y="{y}" width="{TILE}" height="{TILE}" xlink:href="{n}.png"/>'
        s += f'<image x="{x + 50}" y="{y + 400}" width="128" height="128" image-rendering="optimizeSpeed" xlink:href="{n}-64.png"/>'
        s += f'<image x="{x + 230}" y="{y + 432}" width="64" height="64" image-rendering="optimizeSpeed" xlink:href="{n}-32.png"/>'
        col = "#3a3a3a" if prev else "#111"
        s += (f'<text x="{x + TILE // 2}" y="{y + 596}" text-anchor="middle" font-family="Helvetica Neue, Arial" '
              f'font-weight="bold" font-size="60" fill="{col}">{num}</text>')
        if name:
            s += (f'<text x="{x + TILE // 2}" y="{y + 648}" text-anchor="middle" font-family="Helvetica Neue, Arial" '
                  f'font-size="48" fill="#111">{name}</text>')
    s += "</svg>"
    p = os.path.join(OUT, f"overview-round8-{sheet}.svg")
    open(p, "w").write(s)
    subprocess.run([RENDER, p, os.path.join(OUT, f"overview-round8-{sheet}.png"), str(w)], check=True)
