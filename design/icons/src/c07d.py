from lib import *

# 01b in the Persona 3 palette: azure, white ghost, black faceted mask, grey cel shade,
# black ribbon strands and glass shards instead of the starburst and halftone
shade = [(600, 150), (900, 150), (900, 900), (640, 900), (700, 560)]
ribbons = [
    "M1040,80 C860,170 740,280 650,420 C760,300 880,200 1040,120 Z",
    "M1040,260 C900,300 800,380 730,480 C820,400 920,330 1040,300 Z",
    "M-20,760 C140,720 250,660 320,580 C260,690 150,770 -20,800 Z",
    "M-20,900 C120,880 230,840 300,780 C240,850 140,900 -20,930 Z",
]
spots = [(190, 300, 28), (860, 700, 24), (800, 870, 16), (230, 520, 14), (880, 540, 12), (330, 180, 14),
         (580, 910, 12), (420, 900, 18)]


def figure(outline):
    return f"""
<g transform="rotate(-9 512 512)">
  <clipPath id="gc"><path d="{GHOST}"/></clipPath>
  <path d="{GHOST}" fill="#fff"/>
  <g clip-path="url(#gc)">{poly(shade, fill="#a9b4bf")}</g>
  {poly(MASK, fill="#000", stroke="#fff", stroke_width=outline, stroke_linejoin="miter", paint_order="stroke")}
  {poly(MASK_EYE_L, fill="#fff")}{poly(MASK_EYE_R, fill="#fff")}
</g>"""


body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{AZURE}"/>
<g fill="#000">{''.join(f'<path d="{r}"/>' for r in ribbons)}</g>
{shards(spots)}
{figure(22)}
"""
write("07d-masked-ghost-p3", svg(body), sizes=(1024, 64, 32))
icon_sizes("07d-masked-ghost-p3")
