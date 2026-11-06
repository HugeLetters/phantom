from lib import *

# ghost body = arrow shaft, hem = arrow head (wing tips flick up like a ghost's tail)
ghost = ("M292,560 L292,420 C292,282 390,196 512,196 C634,196 732,282 732,420 L732,560 "
         "L836,520 L776,620 L512,856 L248,620 L188,520 Z")
eyeL = "M366,392 L486,436 C474,486 394,488 366,392 Z"
eyeR = "M658,392 L538,436 C550,486 630,488 658,392 Z"

body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="#0d0d0d"/>
{halftone(120, 930, 13, 640, step=30, color=RED)}
{halftone(920, 100, 10, 420, step=30, color=RED)}
<g transform="translate(0 6)">
<path d="{ghost}" fill="#fff" transform="translate(-24 24)"/>
<path d="{ghost}" fill="{RED}"/>
<path d="{eyeL}" fill="#fff"/><path d="{eyeR}" fill="#fff"/>
</g>
"""
write("04-download-ghost", svg(body), sizes=(1024, 64, 32))
preview("04-download-ghost")
