from lib import *

# 04d iterated: smaller burst, tilted ghost, angular but calm eyes, something in the arrowhead
eyeL = [(350, 404), (486, 414), (474, 452), (366, 448)]
eyeR = mirror(eyeL)


def zig(a, b, teeth, depth):
    """Points from a to b with `teeth` notches cut inward (perpendicular, towards the body)."""
    (ax, ay), (bx, by) = a, b
    L = math.hypot(bx - ax, by - ay)
    nx, ny = -(by - ay) / L, (bx - ax) / L
    out = []
    for i in range(1, teeth * 2):
        t = i / (teeth * 2)
        d = depth if i % 2 else 0
        out.append((ax + (bx - ax) * t + nx * d, ay + (by - ay) * t + ny * d))
    return out


ragged = ([(292, 560), (292, 420)] + [(292 + 220 * (1 - math.cos(a)), 420 - 224 * math.sin(a)) for a in
                                       [i * math.pi / 24 for i in range(1, 24)]]
          + [(732, 420), (732, 560), (836, 520), (776, 620)] + zig((776, 620), (512, 856), 3, 46) + [(512, 856)]
          + zig((512, 856), (248, 620), 3, 46) + [(248, 620), (188, 520)])
plain = ("M292,560 L292,420 C292,282 390,196 512,196 C634,196 732,282 732,420 L732,560 "
         "L836,520 L776,620 L512,856 L248,620 L188,520 Z")
SMALL_BURST = poly(burst(512, 540, 12, 330, 262, jitter=0.16, seed=3, phase=0.2), fill="#000")


def icon(name, ghost, extra=""):
    body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
{halftone(120, 930, 13, 640, step=30)}
{halftone(920, 100, 10, 420, step=30)}
{SMALL_BURST}
<g transform="translate(512 526) rotate(-10) scale(0.9) translate(-512 -526)">
{ghost}
{poly(eyeL, fill="#000")}{poly(eyeR, fill="#000")}
{extra}
</g>
"""
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


shade = '<clipPath id="ac"><path d="' + plain + '"/></clipPath>' + \
    f'<g clip-path="url(#ac)">{poly([(512, 560), (900, 400), (900, 900), (512, 900)], fill="#c9c9c9")}</g>'
icon("04g-download-ghost-shaded", f'<path d="{plain}" fill="#fff"/>' + shade)
