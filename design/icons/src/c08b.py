from lib import *


def keyhole(r, neck, length, flare, teeth, depth):
    """Keyhole ghost centred on x=512 with the head circle at y=0: head radius r, neck half-width,
    body length below the head centre, half-width of the hem, number of hem teeth and their depth."""
    ny = math.sqrt(r * r - neck * neck)
    d = f"M{f(512 - neck)},{f(ny)} A{f(r)},{f(r)} 0 1 1 {f(512 + neck)},{f(ny)} L{f(512 + flare)},{f(length)} "
    seg = 2 * flare / teeth
    for i in range(teeth):
        x = 512 + flare - seg * i
        d += f"L{f(x - seg / 2)},{f(length - depth)} L{f(x - seg)},{f(length + (depth * 0.15 if i % 2 else 0))} "
    return d + "Z", (-r, length)


def eyes(r, y=0.08, size=1.0):
    k = r / 180 * size
    L = [(512 - 110 * k, y * r - 30 * k), (512 - 12 * k, y * r + 2 * k), (512 - 26 * k, y * r + 40 * k), (512 - 90 * k, y * r + 38 * k)]
    return L, mirror(L)


def icon(name, r, neck, length, flare, teeth=4, depth=70, eye=1.0, height=640, tilt=-6):
    hole, (top, bottom) = keyhole(r, neck, length, flare, teeth, depth)
    eL, eR = eyes(r, size=eye)
    s = height / (bottom - top)
    cy = (top + bottom) / 2
    body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
{halftone(120, 920, 14, 600, step=30)}
{halftone(920, 110, 12, 480, step=30)}
<g transform="translate(512 520) rotate({tilt}) scale({f(s)}) translate(-512 {f(-cy)})">
  <path d="{hole}" fill="#fff" stroke="#fff" stroke-width="{f(36 / s)}" stroke-linejoin="miter" stroke-miterlimit="1.6" transform="translate({f(16 / s)} {f(16 / s)})"/>
  <path d="{hole}" fill="#000"/>
  {poly(eL, fill="#fff")}{poly(eR, fill="#fff")}
</g>
"""
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


# big head, short stubby flared skirt
icon("08b-keyhole-ghost-bighead", r=230, neck=110, length=430, flare=200, teeth=3, depth=60, eye=1.15, height=620)
# small head, tall slender body
icon("08c-keyhole-ghost-tall", r=120, neck=46, length=620, flare=170, teeth=4, depth=70, eye=1.0, height=700, tilt=-8)
# bell: wide hem nearly the width of the icon
icon("08d-keyhole-ghost-bell", r=170, neck=70, length=440, flare=290, teeth=5, depth=56, eye=1.05, height=580, tilt=-4)
