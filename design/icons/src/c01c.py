from lib import *

# 01 with the simpler glasses; three halftone scales to compare at small sizes


def figure(outline=22):
    return f"""
<g transform="rotate(-9 512 512)">
  <path d="{GHOST}" fill="#fff"/>
  <path d="{MASK2}" fill="#000" stroke="#fff" stroke-width="{outline}" stroke-linejoin="round" paint-order="stroke"/>
  <path d="{MASK2_EYE_L}" fill="#fff"/><path d="{MASK2_EYE_R}" fill="#fff"/>
</g>"""


def icon(name, step, k):
    body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
{halftone(130, 920, 14 * k, 640, step=step)}
{halftone(920, 110, 11 * k, 430, step=step)}
{poly(burst(512, 512, 13, 375, 285, jitter=0.18, seed=3, phase=0.2), fill="#000")}
{figure()}
"""
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("01c-masked-ghost-simple-glasses", 30, 1)
icon("01d-masked-ghost-medium-dots", 46, 1.55)
icon("01e-masked-ghost-big-dots", 64, 2.15)
