from lib import *

crown = "M380,150 C450,138 574,138 644,150 C632,240 628,320 630,392 L394,392 C396,320 392,240 380,150 Z"
crown_top = "M380,150 C450,170 574,170 644,150 C574,128 450,128 380,150 Z"
band = "M392,330 L632,330 L630,384 L394,384 Z"
brim = "M236,362 C300,420 724,420 788,362 C800,440 720,470 512,470 C304,470 224,440 236,362 Z"
mask = (
    "M262,526 L344,532 C404,512 472,518 512,546 C552,518 620,512 680,532 L762,526 "
    "L728,584 C700,646 620,660 566,632 L512,606 L458,632 C404,660 324,646 296,584 Z"
)
eyeL = "M336,558 L470,586 C440,622 368,618 336,558 Z"
eyeR = "M688,558 L554,586 C584,622 656,618 688,558 Z"
ghost = ("M310,560 C310,470 400,420 512,420 C624,420 714,470 714,560 L724,720 L780,900 L680,836 "
         "L630,920 L566,830 L512,920 L458,830 L394,920 L344,836 L244,900 L300,720 Z")

body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
{halftone(900, 110, 14, 480, step=30)}
{halftone(110, 940, 11, 360, step=30)}
<g transform="translate(512 535) scale(0.9) translate(-512 -512) rotate(-10 512 512)">
  <clipPath id="gb"><path d="{ghost}"/></clipPath>
  <g clip-path="url(#gb)">{halftone(512, -2400, 175, 3380, step=26, color="#fff", angle=0)}</g>
  <path d="{crown}" fill="#000"/>
  <path d="{band}" fill="#fff"/>
  <path d="{brim}" fill="#000"/>
  <path d="{mask}" fill="#000"/>
  <path d="{eyeL}" fill="{RED}"/><path d="{eyeR}" fill="{RED}"/>
</g>
"""
write("03-invisible-thief", svg(body), sizes=(1024, 64, 32))
preview("03-invisible-thief")
