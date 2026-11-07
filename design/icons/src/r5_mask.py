from lib import *

# Round 5, masked-ghost family (new numbering): 01-07 on red, 08 the calling card


def figure(mask, eye_l, eye_r, inlay=""):
    return f"""
<g transform="rotate(-9 512 512)">
  <path d="{GHOST}" fill="#fff"/>
  {poly(mask, fill="#000", stroke="#fff", stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")}
  {inlay}
  {poly(eye_l, fill="#fff")}{poly(eye_r, fill="#fff")}
</g>"""


BURST = poly(burst(512, 512, 13, 375, 285, jitter=0.18, seed=3, phase=0.2), fill="#000")
DOTS = halftone(130, 920, 14, 640, step=30) + halftone(920, 110, 11, 430, step=30)
FIG = figure(MASK3, MASK3_EYE_SM_L, MASK3_EYE_SM_R)
STARS = stars(512, 560, 5, 1150, 95, [RED, "#a8000c"], phase=-math.pi / 2 - 0.12)
RINGS = rings(512, 540, 900, 70, [RED, "#a8000c"]) + splatter([(170, 220, 22), (860, 830, 26), (880, 260, 12), (190, 840, 14)], seed=2)


def icon(name, body):
    write(name, svg(f'<rect x="0" y="0" width="1024" height="1024" fill="{RED}"/>' + body), sizes=(1024, 64, 32))
    icon_sizes(name)


icon("r5-01-flat-mask", DOTS + BURST + FIG)
icon("r5-02-stars", STARS + BURST + FIG)
icon("r5-03-rings", RINGS + BURST + FIG)
# 01i with more red: bright and dark red stars with black, so black is one band in three
icon("r5-04-stars-black", stars(512, 560, 5, 1150, 95, [RED, "#a8000c", RED, "#000"], phase=-math.pi / 2 - 0.12) + BURST + FIG)
icon("r5-05-splatter", splatter2([(180, 210, 60), (862, 836, 72), (880, 230, 30), (150, 700, 34), (560, 935, 22),
                                  (930, 560, 20)], seed=8) + BURST + FIG)
icon("r5-06-splatter-big", splatter2([(150, 260, 80), (880, 860, 90), (840, 170, 44), (230, 880, 46), (940, 520, 26),
                                        (520, 120, 20)], seed=21) + BURST + FIG)
icon("r5-07-flame-mask", DOTS + BURST + figure(MASK5, MASK3_EYE_SM_L, MASK3_EYE_SM_R))
icon("r5-08-flame-inlay", STARS + BURST + figure(MASK5, MASK3_EYE_SM_L, MASK3_EYE_SM_R,
                                               poly(MASK5_INLAY_L, fill="#fff") + poly(MASK5_INLAY_R, fill="#fff")))

# calling card (02d/02e) with the smaller eyes; 09 plain card, 10 star-printed card
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
  {poly([(x + 22, y + 22) for x, y in card], fill="#fff")}
  {poly(card, fill=RED)}
  <clipPath id="cc">{poly(card)}</clipPath><g clip-path="url(#cc)">{card_fill}</g>
</g>
<g transform="translate(556 468) rotate(8) scale(0.8) translate(-548 -524)">
  <path d="{GHOST}" fill="#000" transform="translate(20 20)"/>
  <path d="{GHOST}" fill="#fff"/>
  {poly(MASK3, fill="#000", stroke="#fff", stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3, paint_order="stroke")}
  {poly(MASK3_EYE_SM_L, fill="#fff")}{poly(MASK3_EYE_SM_R, fill="#fff")}
</g>"""
    write(name, svg(body), sizes=(1024, 64, 32))
    icon_sizes(name)


card_icon("r5-09-card", "")
card_icon("r5-10-card-stars", stars(470, 640, 5, 900, 80, [RED, "#a8000c"], phase=-math.pi / 2 - 0.2))
