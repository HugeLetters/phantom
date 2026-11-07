from lib import *


def keyhole(r, neck, length, flare, teeth, depth):
    """Keyhole ghost centred on x=512 with the head circle at y=0 (same construction as c08b.py)."""
    ny = math.sqrt(r * r - neck * neck)
    d = f"M{f(512 - neck)},{f(ny)} A{f(r)},{f(r)} 0 1 1 {f(512 + neck)},{f(ny)} L{f(512 + flare)},{f(length)} "
    seg = 2 * flare / teeth
    for i in range(teeth):
        x = 512 + flare - seg * i
        d += f"L{f(x - seg / 2)},{f(length - depth)} L{f(x - seg)},{f(length + (depth * 0.15 if i % 2 else 0))} "
    return d + "Z"


def calm_eyes(r, k=1.0):
    """Nearly level top lid, rounded bottom (the 04d eyes), scaled to the head."""
    s = r / 180 * k
    def eye(sign):
        p = lambda x, y: f"{f(512 + sign * x * s)},{f(y * s)}"
        return f"M{p(-112, -18)} L{p(-14, -4)} Q{p(-20, 40)} {p(-62, 40)} Q{p(-104, 36)} {p(-112, -18)} Z"
    return eye(1), eye(-1)


def icon(name, neck, with_burst, r=230, length=430, flare=210, teeth=3, depth=60, height=620):
    hole = keyhole(r, neck, length, flare, teeth, depth)
    eL, eR = calm_eyes(r, 1.15)
    s = height / (length + r)
    cy = (length - r) / 2
    burst_ = poly(burst(512, 520, 13, 400, 300, jitter=0.18, seed=3, phase=0.2), fill="#000") if with_burst else ""
    body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>
{halftone(120, 920, 14, 600, step=30)}
{halftone(920, 110, 12, 480, step=30)}
{burst_}
<g transform="translate(512 520) rotate(-6) scale({f(s)}) translate(-512 {f(-cy)})">
  {f'<path d="{hole}" fill="#fff" stroke="#fff" stroke-width="{f(44 / s)}" stroke-linejoin="round"/>' if with_burst else ""}
  <path d="{hole}" fill="#fff" stroke="#fff" stroke-width="{f(36 / s)}" stroke-linejoin="miter" stroke-miterlimit="1.6" transform="translate({f(16 / s)} {f(16 / s)})"/>
  <path d="{hole}" fill="#000"/>
  <path d="{eL}" fill="#fff"/><path d="{eR}" fill="#fff"/>
</g>
"""
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("08e-bighead-wide-neck", neck=160, with_burst=False)
icon("08f-bighead-wide-neck-burst", neck=160, with_burst=True)
