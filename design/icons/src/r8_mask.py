from lib import *

# Round 8, burning eyes (59-61): flames in the band-flame style coming out of MASK7's eyes, on 46's scene


def burning_eyes(tongues=None, escape=None):
    """Flame eyes built from tongues (lib.tongue), clipped to the mask so the band stays black around them;
    optional `escape` tongues break free past the wing tips, outlined in black."""
    tongues = tongues or FLAME_TONGUES_L
    inner = ""
    for side in ("L", "R"):
        mp = (lambda p: p) if side == "L" else (lambda p: mirror(p, 514))
        md = (lambda d: d) if side == "L" else mirror_path
        inner += f'<path d="{md(FLAME_BASE_L)}" fill="#fff"/>' + "".join(poly(mp(t), fill="#fff") for t in tongues)
    out = f'<clipPath id="mclip">{poly(MASK7)}</clipPath><g clip-path="url(#mclip)">{inner}</g>'
    for side in ("L", "R"):
        mp = (lambda p: p) if side == "L" else (lambda p: mirror(p, 514))
        out += "".join(poly(mp(t), fill="#fff", stroke="#000", stroke_width=8, stroke_linejoin="round", paint_order="stroke")
                       for t in (escape or []))
    return out


def figure(eyes):
    return f"""
<g transform="rotate(-9 512 512)">
  <path d="{GHOST}" fill="#fff"/>
  {poly(MASK7, fill="#000", stroke="#fff", stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")}
  {eyes}
</g>"""


BURST = poly(burst(512, 512, 13, 375, 285, jitter=0.18, seed=3, phase=0.2), fill="#000")
STARS = stars(512, 560, 5, 1150, 95, [RED, "#a8000c"], phase=-math.pi / 2 - 0.12)


def icon(name, body):
    write(name, svg(f'<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>' + body), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("r8-59-burning-eyes", STARS + BURST + figure(burning_eyes()))
