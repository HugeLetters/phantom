from lib import *

# keyhole whose flared bottom ends in a ghost's ragged hem
hole = ("M440,540 A180,180 0 1 1 584,540 L676,820 L604,770 L558,840 L512,776 L466,840 L420,770 L348,820 Z")
eyeL = "M402,382 L500,414 C486,452 422,450 402,382 Z"
eyeR = "M622,382 L524,414 C538,452 602,450 622,382 Z"

body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
{halftone(120, 920, 14, 600, step=30)}
{halftone(920, 110, 12, 480, step=30)}
<g transform="translate(512 520) scale(0.84) translate(-512 -540) rotate(-6 512 560)">
  <path d="{hole}" fill="#fff" stroke="#fff" stroke-width="40" stroke-linejoin="miter" transform="translate(16 16)"/>
  <path d="{hole}" fill="#000"/>
  <path d="{eyeL}" fill="#fff"/><path d="{eyeR}" fill="#fff"/>
</g>
"""
write("08-keyhole-ghost", svg(body), sizes=(1024, 64, 32))
preview("08-keyhole-ghost")
