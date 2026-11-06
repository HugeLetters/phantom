from lib import *

AZURE = "#14a5ec"
CYAN = "#7ff3ff"

# upright ghost (head centre 512,380) with a long tail splitting into three thin wisps
ghost = ("M362,380 A150,150 0 0 1 662,380 "
         "C662,520 700,650 790,800 C710,750 646,712 604,700 "
         "C620,790 600,890 548,980 C524,880 494,790 454,716 "
         "C410,780 340,840 250,870 C334,750 362,570 362,380 Z")
eyeL = [(404, 360), (494, 392), (484, 414), (418, 406)]
eyeR = mirror(eyeL)
shade = [(560, 150), (800, 150), (800, 1000), (600, 1000), (640, 600)]
place = "translate(600 640) rotate(150) scale(0.78) translate(-512 -380)"

ribbons = [
    "M-20,250 C150,260 300,330 420,420 C330,330 180,250 -20,230 Z",
    "M120,40 C230,140 330,260 400,420 C350,270 260,130 150,30 Z",
    "M330,10 C380,150 410,300 420,440 C440,290 420,140 360,0 Z",
]
rnd = random.Random(7)
shards = ""
for cx, cy, s in [(190, 740, 30), (850, 610, 24), (800, 850, 18), (250, 560, 14), (870, 330, 18), (640, 260, 12),
                  (560, 900, 12), (360, 860, 16)]:
    a = rnd.uniform(0, 360)
    p = [(cx - s, cy - s * 0.4), (cx + s * 0.3, cy - s * 0.8), (cx + s, cy + s * 0.3), (cx - s * 0.2, cy + s * 0.7)]
    shards += poly(rot(p, a, cx, cy), fill=rnd.choice([CYAN, "#ffffff", "#2fd6ff"]), opacity="0.85")

body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{AZURE}"/>
<g fill="#000">{''.join(f'<path d="{r}"/>' for r in ribbons)}</g>
{shards}
<g transform="{place}">
  <clipPath id="gc"><path d="{ghost}"/></clipPath>
  <path d="{ghost}" fill="#fff"/>
  <g clip-path="url(#gc)">{poly(shade, fill="#a9b4bf")}</g>
  <path d="{ghost}" fill="none" stroke="#000" stroke-width="12" stroke-linejoin="round"/>
  {poly(eyeL, fill="#000")}{poly(eyeR, fill="#000")}
</g>
"""
write("07b-falling-ghost", svg(body), sizes=(1024, 64, 32))
icon_sizes("07b-falling-ghost")
