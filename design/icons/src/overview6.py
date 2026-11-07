# Round 6 contact sheets (same layout as overview5.py). Each family starts a new row; the earlier versions it builds on come first,
# labelled "was NNx" in grey, then the new designs. Main preview 390 px, 64 and 32 px renders at 2x
# (nearest neighbour), number 60 px, name 48 px.
from lib import *

SHEETS = {
    "red": [
        (["r5-01-flat-mask", "r5-02-stars", "r5-03-rings"],
         ["r6-25-tri-mask-stars", "r6-26-tri-mask-rings", "r6-27-tri-mask-dots", "r6-28-flame-eyes-stars", "r6-29-flame-eyes-rings"]),
        (["r5-09-card", "r5-10-card-stars"], ["r6-30-card-tri-mask"]),
        ([], ["r6-31-download-mask", "r6-32-download-big-head"]),
        (["r5-14-keyhole", "r5-15-keyhole-white"], ["r6-33-keyhole-white", "r6-34-keyhole-black", "r6-35-keyhole-flame-eyes"]),
        ([], ["r6-41-royal-gold-mask", "r6-42-royal-gold-burst", "r6-43-royal-flame-eyes", "r6-44-royal-gold-card",
              "r6-45-royal-keyhole"]),
    ],
    "p3": [
        (["r5-21-shadow-white", "r5-23-hour-shadow"],
         ["r6-36-hour-shadow", "r6-37-blue-moon-shadow", "r6-38-blue-moon-white", "r6-39-vein-shadow", "r6-40-keyart-shadow"]),
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
    p = os.path.join(OUT, f"overview-round6-{sheet}.svg")
    open(p, "w").write(s)
    subprocess.run([RENDER, p, os.path.join(OUT, f"overview-round6-{sheet}.png"), str(w)], check=True)
