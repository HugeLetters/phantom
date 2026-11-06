from lib import *

# 01's ghost, upright, cel-shaded; a sweep of black "bangs" hides one eye
shade = [(600, 150), (900, 150), (900, 900), (640, 900), (700, 560)]
bangs = ("M676,262 C590,300 470,360 300,560 L408,486 L372,600 L470,500 L490,560 "
         "C520,450 590,350 720,300 Z")
eye = [(700, 420), (570, 452), (600, 500), (680, 478)]
ribbons = [
    "M1040,120 C860,200 760,330 700,520 C780,350 880,230 1040,150 Z",
    "M1040,300 C900,330 820,420 760,560 C840,430 920,360 1040,330 Z",
    "M-20,820 C140,760 260,690 330,600 C270,720 150,800 -20,850 Z",
]
spots = [(190, 300, 28), (860, 700, 24), (800, 870, 16), (230, 560, 14), (880, 520, 12), (330, 180, 14),
         (560, 900, 12), (200, 760, 18)]

body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{AZURE}"/>
<g fill="#000">{''.join(f'<path d="{r}"/>' for r in ribbons)}</g>
{shards(spots)}
<g transform="translate(512 520) scale(0.92) rotate(-8) translate(-548 -524)">
  <clipPath id="gc"><path d="{GHOST}"/></clipPath>
  <path d="{GHOST}" fill="#fff"/>
  <g clip-path="url(#gc)">{poly(shade, fill="#a9b4bf")}</g>
  <path d="{GHOST}" fill="none" stroke="#000" stroke-width="14" stroke-linejoin="round"/>
  {poly(eye, fill="#000")}
  <path d="{bangs}" fill="#000"/>
</g>
"""
write("07c-cel-ghost", svg(body), sizes=(1024, 64, 32))
icon_sizes("07c-cel-ghost")
