from lib import *

ghost = ("M292,560 L292,420 C292,282 390,196 512,196 C634,196 732,282 732,420 L732,560 "
         "L836,520 L776,620 L512,856 L248,620 L188,520 Z")
# calmer eyes: nearly level top lid, rounded bottom
eyeL = "M352,398 L486,416 Q478,466 424,466 Q370,462 352,398 Z"
eyeR = "M672,398 L538,416 Q546,466 600,466 Q654,462 672,398 Z"


def icon(name, with_burst):
    burst_ = poly(burst(512, 520, 13, 400, 300, jitter=0.18, seed=3, phase=0.2), fill="#000") if with_burst else ""
    body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
{halftone(120, 930, 13, 640, step=30)}
{halftone(920, 100, 10, 420, step=30)}
{burst_}
<g transform="translate(512 526) scale(0.92) translate(-512 -526)">
<path d="{ghost}" fill="#fff"/>
<path d="{eyeL}" fill="#000"/><path d="{eyeR}" fill="#000"/>
</g>
"""
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("04d-download-ghost-burst", True)
icon("04e-download-ghost-plain", False)
