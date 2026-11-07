from lib import *

# Round 7, masked ghost (46-51): MASK7 (between 02 and 25), flames flowing out of the eyes, cards with MASK7


def figure(eyes, mask=MASK7):
    return f"""
<g transform="rotate(-9 512 512)">
  <path d="{GHOST}" fill="#fff"/>
  {poly(mask, fill="#000", stroke="#fff", stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")}
  {eyes}
</g>"""


TRI = poly(MASK7_EYE_L, fill="#fff") + poly(MASK7_EYE_R, fill="#fff")


def flames(l, r, color="#fff"):
    # black edge so the part of the flame outside the mask separates from the white ghost and the burst
    # scaled towards the inner eye corner so black mask stays visible around the flame
    return "".join(f'<path d="{d}" fill="{color}" stroke="#000" stroke-width="10" stroke-linejoin="round" paint-order="stroke" '
                   f'transform="translate({x} 446) scale(0.78) translate(-{x} -446)"/>' for d, x in ((l, 470), (r, 558)))


BURST = poly(burst(512, 512, 13, 375, 285, jitter=0.18, seed=3, phase=0.2), fill="#000")
STARS = stars(512, 560, 5, 1150, 95, [RED, "#a8000c"], phase=-math.pi / 2 - 0.12)
RINGS = rings(512, 540, 900, 70, [RED, "#a8000c"]) + splatter([(170, 220, 22), (860, 830, 26), (880, 260, 12), (190, 840, 14)], seed=2)


def icon(name, body):
    write(name, svg(f'<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>' + body), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("r7-46-mid-mask-stars", STARS + BURST + figure(TRI))
icon("r7-47-mid-mask-rings", RINGS + BURST + figure(TRI))
icon("r7-48-flame-eyes-out", STARS + BURST + figure(flames(FLAME_OUT_L, FLAME_OUT_R)))
icon("r7-49-flame-eyes-red", STARS + BURST + figure(flames(FLAME_OUT_L, FLAME_OUT_R, RED)))

# cards (round 5's 09 and 10) with MASK7
rnd = random.Random(11)
x0, x1, y0, y1 = 512 - 250, 512 + 250, 512 - 300, 512 + 320
card = [(x0, y0), (x1 - 80, y0), (x1, y0 + 80), (x1, y1)]
for i in range(1, 13):
    x = x1 - (x1 - x0) * i / 13
    card.append((x, y1 + (rnd.uniform(-22, -4) if i % 2 else rnd.uniform(4, 16))))
card.append((x0, y1 - 10))
card = rot(card, -12)


def card_icon(name, card_fill):
    body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="#0d0d0d"/>
{halftone(900, 900, 15, 700, step=30, color="#3a3a3a")}
{halftone(110, 120, 11, 420, step=30, color="#3a3a3a")}
<g transform="translate(500 500) scale(0.84) translate(-512 -512)">
  {poly([(x + 22, y + 22) for x, y in card], fill="#fff")}{poly(card, fill=RED)}
  <clipPath id="cc">{poly(card)}</clipPath><g clip-path="url(#cc)">{card_fill}</g>
</g>
<g transform="translate(556 468) rotate(8) scale(0.8) translate(-548 -524)">
  <path d="{GHOST}" fill="#000" transform="translate(20 20)"/>
  <path d="{GHOST}" fill="#fff"/>
  {poly(MASK7, fill="#000", stroke="#fff", stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")}
  {TRI}
</g>"""
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


card_icon("r7-50-card-mid-mask", "")
card_icon("r7-51-card-mid-mask-stars", stars(470, 640, 5, 900, 80, [RED, "#a8000c"], phase=-math.pi / 2 - 0.2))
