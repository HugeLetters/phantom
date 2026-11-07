from lib import *

# Round 5, download ghost (from 04g): three ways to fill the arrowhead
plain = ("M292,560 L292,420 C292,282 390,196 512,196 C634,196 732,282 732,420 L732,560 "
         "L836,520 L776,620 L512,856 L248,620 L188,520 Z")
eyeL, eyeR = CALM_EYE_L, mirror(CALM_EYE_L)
SMALL_BURST = poly(burst(512, 540, 12, 330, 262, jitter=0.16, seed=3, phase=0.2), fill="#000")


def icon(name, inside, outside=""):
    body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
{halftone(120, 930, 13, 640, step=30)}
{halftone(920, 100, 10, 420, step=30)}
{SMALL_BURST}
<g transform="translate(512 526) rotate(-10) scale(0.9) translate(-512 -526)">
<clipPath id="ac"><path d="{plain}"/></clipPath>
<path d="{plain}" fill="#fff"/>
<g clip-path="url(#ac)">{inside}</g>
{poly(eyeL, fill="#000")}{poly(eyeR, fill="#000")}
{outside}
</g>
"""
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


# 11: a sly jagged grin across the arrowhead
grin = [(372, 560), (420, 584), (446, 556), (480, 588), (512, 560), (544, 588), (578, 556), (604, 584), (652, 560),
        (600, 648), (512, 690), (424, 648)]
icon("r5-11-grin", "", poly(grin, fill="#000"))
# 12: chevrons cut through the arrowhead, like a download motion mark
chev = "".join(poly([(150, y), (512, y + 330), (874, y), (874, y + 34), (512, y + 364), (150, y + 34)], fill=RED)
               for y in (296, 386))
icon("r5-12-chevrons", chev)
# 13: comic halftone shading filling the arrowhead, densest at the tip
icon("r5-13-halftone", halftone(512, 860, 16, 380, step=26, color="#000"))
