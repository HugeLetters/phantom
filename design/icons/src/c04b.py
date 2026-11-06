from lib import *

# ghost body = arrow shaft, hem = arrow head (wing tips flick up like a ghost's tail)
ghost = ("M292,560 L292,420 C292,282 390,196 512,196 C634,196 732,282 732,420 L732,560 "
         "L836,520 L776,620 L512,856 L248,620 L188,520 Z")
eyeL = [(352, 386), (492, 432), (476, 478), (392, 470)]
eyeR = mirror(eyeL)


def icon(name, bg, fg, shade, dots):
    body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{bg}"/>
{halftone(120, 930, 13, 640, step=30, color=dots)}
{halftone(920, 100, 10, 420, step=30, color=dots)}
<g transform="translate(0 6)">
<path d="{ghost}" fill="{shade}" transform="translate(-26 26)"/>
<path d="{ghost}" fill="{fg}"/>
{poly(eyeL, fill=shade)}{poly(eyeR, fill=shade)}
</g>
"""
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("04b-download-ghost-red", RED, "#fff", "#000", "#000")
icon("04c-download-ghost-black", "#0d0d0d", "#fff", RED, "#3a3a3a")
