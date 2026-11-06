from lib import *

ghost = ("M352,500 C352,346 446,262 546,270 C656,280 712,382 690,508 C676,590 700,668 760,734 "
         "C676,744 622,714 596,672 C598,744 566,800 500,834 C504,772 478,730 446,708 "
         "C410,772 336,812 248,810 C322,748 352,650 352,500 Z")
eyeL = "M430,452 L520,478 C506,510 450,508 430,452 Z"
eyeR = "M640,446 L566,476 C584,506 632,498 640,446 Z"

defs = """
<linearGradient id="sky" x1="0" y1="0" x2="0.3" y2="1">
  <stop offset="0" stop-color="#123a7a"/><stop offset="1" stop-color="#030b1f"/>
</linearGradient>
<radialGradient id="moon" cx="0.42" cy="0.38" r="0.7">
  <stop offset="0" stop-color="#f2fbff"/><stop offset="0.7" stop-color="#bfe3ff"/><stop offset="1" stop-color="#7fb6ee"/>
</radialGradient>
<filter id="blur" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="26"/></filter>
<filter id="eg" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="10"/></filter>
"""
body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="url(#sky)"/>
{halftone(140, 900, 7, 520, step=26, color="#2b6fd6", opacity=0.5)}
<circle cx="560" cy="470" r="300" fill="#6fc3ff" opacity="0.45" filter="url(#blur)"/>
<circle cx="560" cy="470" r="268" fill="url(#moon)"/>
<circle cx="560" cy="470" r="318" fill="none" stroke="#8fd3ff" stroke-width="6" opacity="0.7"/>
<path d="{ghost}" fill="#061433" stroke="#8fe0ff" stroke-width="10" stroke-linejoin="round"/>
<g fill="#5ff0ff" filter="url(#eg)"><path d="{eyeL}"/><path d="{eyeR}"/></g>
<g fill="#c9fbff"><path d="{eyeL}"/><path d="{eyeR}"/></g>
"""
write("07-moonlit-ghost", svg(body, defs), sizes=(1024, 64, 32))
preview("07-moonlit-ghost")
