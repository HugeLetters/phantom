from lib import *

ghost = (
    "M292,500 C292,330 392,226 516,226 C648,226 736,330 736,490 "
    "L742,640 L812,800 L700,738 L664,822 L594,730 L532,812 L480,722 L408,786 L384,700 L318,742 L300,620 Z"
)
# faceted domino mask: straight cuts, wings swept up to sharp points, notch at the bridge
half = [(514, 424), (466, 390), (340, 392), (214, 328), (268, 452), (346, 516), (452, 506), (514, 470)]
mask = half + mirror(half[1:-1], 514)[::-1]
eyeL = [(308, 408), (478, 444), (458, 490), (364, 484)]
eyeR = mirror(eyeL, 514)


def figure(outline):
    return f"""
<g transform="rotate(-9 512 512)">
  <path d="{ghost}" fill="#fff"/>
  {poly(mask, fill="#000", stroke="#fff", stroke_width=outline, stroke_linejoin="miter", paint_order="stroke")}
  {poly(eyeL, fill="#fff")}{poly(eyeR, fill="#fff")}
</g>"""


body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
{halftone(130, 920, 14, 640, step=30)}
{halftone(920, 110, 11, 430, step=30)}
{poly(burst(512, 512, 13, 375, 285, jitter=0.18, seed=3, phase=0.2), fill="#000")}
{figure(22)}
"""
write("01b-masked-ghost-angular", svg(body), sizes=(1024, 64, 32))
icon_sizes("01b-masked-ghost-angular")

# small-size artwork (16/32 px slots of the .icns): no burst or halftone, figure scaled up
small = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
<g transform="translate(512 516) scale(1.1) translate(-524 -524)">{figure(30)}</g>
"""
write("01b-masked-ghost-angular-small", svg(small), sizes=(1024, 64, 32))
icon_sizes("01b-masked-ghost-angular-small")
