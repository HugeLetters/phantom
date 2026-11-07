# contact sheet per round: each concept at 300px with real-size 64 and 32 px renders beneath, numbered.
# usage: python3 overview.py 2|3
from lib import *

ROUNDS = {2: ["01b-masked-ghost-angular", "01b-masked-ghost-angular-small", "02b-calling-card-ghost",
         "04b-download-ghost-red", "04c-download-ghost-black", "07d-masked-ghost-p3", "07e-sinking-ghost",
         "08b-keyhole-ghost-bighead", "08c-keyhole-ghost-tall", "08d-keyhole-ghost-bell"],
          3: ["01c-masked-ghost-simple-glasses", "01d-masked-ghost-medium-dots", "01e-masked-ghost-big-dots",
              "02c-red-card-white-ghost", "04d-download-ghost-burst", "04e-download-ghost-plain",
              "07f-p3-gradient-wedge", "07g-p3-moon-window", "07h-p3-underwater",
              "08e-bighead-wide-neck", "08f-bighead-wide-neck-burst"],
          4: ["01f-flat-mask", "01g-blindfold-mask", "01h-star-pattern", "01i-dark-stars", "01j-rings-splatter",
              "01k-ink-splatter", "02d-card-big-ghost", "02e-card-star-print", "04g-download-ghost-shaded",
              "07i-shadow-mask-white", "07j-shadow-dark-underwater", "07k-shadow-white-underwater",
              "07l-shadow-dark-hour", "08g-bighead-centred", "08h-bighead-small-burst", "08i-bighead-white-burst"]}
rnd_ = int(sys.argv[1])
names = ROUNDS[rnd_]
labels = {"01b-masked-ghost-angular-small": "01b small (16/32 px art)"}
cols, cw, ch = (5 if rnd_ == 2 else 6), 360, (470 if rnd_ < 4 else 545)
w, h = cols * cw, ch * math.ceil(len(names) / cols)
s = (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}">'
     f'<rect width="{w}" height="{h}" fill="#8e8f93"/>')
for i, n in enumerate(names):
    x, y = (i % cols) * cw + 30, (i // cols) * ch + 20
    s += f'<image x="{x}" y="{y}" width="300" height="300" xlink:href="{n}.png"/>'
    if rnd_ < 4:
        s += f'<image x="{x + 90}" y="{y + 316}" width="64" height="64" xlink:href="{n}-64.png"/>'
        s += f'<image x="{x + 186}" y="{y + 332}" width="32" height="32" xlink:href="{n}-32.png"/>'
    else:  # from round 4: 64 and 32 px renders shown at 2x, nearest neighbour
        s += f'<image x="{x + 40}" y="{y + 312}" width="128" height="128" image-rendering="optimizeSpeed" xlink:href="{n}-64.png"/>'
        s += f'<image x="{x + 196}" y="{y + 344}" width="64" height="64" image-rendering="optimizeSpeed" xlink:href="{n}-32.png"/>'
    label = labels.get(n, n.split("-", 1)[0] + " " + n.split("-", 1)[1])
    if rnd_ < 4:
        s += (f'<text x="{x + 150}" y="{y + 420}" text-anchor="middle" font-family="Helvetica Neue, Arial" '
              f'font-weight="bold" font-size="20" fill="#222">{label}</text>')
    else:  # number and name on separate lines so the bigger type fits the column
        num, rest = label.split(" ", 1)
        s += (f'<text x="{x + 150}" y="{y + 478}" text-anchor="middle" font-family="Helvetica Neue, Arial" '
              f'font-weight="bold" font-size="30" fill="#222">{num}</text>'
              f'<text x="{x + 150}" y="{y + 508}" text-anchor="middle" font-family="Helvetica Neue, Arial" '
              f'font-size="24" fill="#222">{rest}</text>')
s += "</svg>"
p = os.path.join(OUT, f"overview-round{rnd_}.svg")
open(p, "w").write(s)
zoom = 1 if rnd_ < 4 else 1.2  # from round 4 the sheet renders 20% bigger
subprocess.run([RENDER, p, os.path.join(OUT, f"overview-round{rnd_}.png"), str(round(w * zoom))], check=True)
