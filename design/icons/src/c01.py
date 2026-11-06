from lib import *

ghost = (
    "M292,500 C292,330 392,226 516,226 C648,226 736,330 736,490 "
    "L742,640 L812,800 L700,738 L664,822 L594,730 L532,812 L480,722 L408,786 L384,700 L318,742 L300,620 Z"
)
# domino mask with swept, pointed wings
mask = (
    "M236,392 L330,404 C400,384 470,392 514,420 C558,392 628,384 698,404 L796,384 "
    "L752,446 C726,512 640,540 576,512 L514,478 L452,512 C388,540 302,512 276,450 Z"
)
eyeL = "M326,430 L462,466 C436,504 362,500 326,430 Z"
eyeR = "M702,430 L566,466 C592,504 666,500 702,430 Z"

body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
{halftone(130, 920, 14, 640, step=30)}
{halftone(920, 110, 11, 430, step=30)}
{poly(burst(512, 512, 13, 375, 285, jitter=0.18, seed=3, phase=0.2), fill="#000")}
<g transform="rotate(-9 512 512)">
  <path d="{ghost}" fill="#fff"/>
  <path d="{mask}" fill="#000" stroke="#fff" stroke-width="22" stroke-linejoin="miter" paint-order="stroke"/>
  <path d="{eyeL}" fill="#fff"/><path d="{eyeR}" fill="#fff"/>
</g>
"""
write("01-masked-ghost", svg(body), sizes=(1024, 64, 32))
preview("01-masked-ghost")
