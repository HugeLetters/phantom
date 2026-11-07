from lib import *

# Round 6, "royal" set (41-45): ribbed gold, white with black drop shadows, star glints, red radial glow
BG = '<rect x="0" y="0" width="1024" height="1024" fill="url(#royalbg)"/>'
GLINTS = sparkle(800, 250, 110, 12) + sparkle(250, 770, 70, -8) + sparkle(860, 760, 46, 20)


def out(name, body):
    write(name, svg(BG + body, GOLD_DEFS), sizes=(1024, 64, 32))
    icon_sizes(name)


def ghost(mask_svg, shadow=True):
    sh = f'<path d="{GHOST}" fill="#000" transform="translate(22 18)"/>' if shadow else ""
    return f'<g transform="rotate(-9 512 512)">{sh}<path d="{GHOST}" fill="#fff"/>{mask_svg}</g>'


GOLD_MASK = gold_ribbed(MASK6, ribs=2, step=10) + poly(MASK6_EYE_L, fill="#1a0d02") + poly(MASK6_EYE_R, fill="#1a0d02")
BLACK_MASK = (poly(MASK6, fill="#000", stroke="#fff", stroke_width=22, stroke_linejoin="miter", stroke_miterlimit=3,
                   paint_order="stroke") + poly(MASK6_EYE_L, fill="#fff") + poly(MASK6_EYE_R, fill="#fff"))

# 41: white ghost with a ribbed gold mask
out("r6-41-royal-gold-mask", ghost(GOLD_MASK) + GLINTS)
# 42: the black-mask ghost over a ribbed gold burst instead of the black one
out("r6-42-royal-gold-burst", gold_ribbed(burst(512, 512, 13, 380, 300, jitter=0.12, seed=3, phase=0.2), ribs=3, step=16)
    + ghost(BLACK_MASK) + GLINTS)
# 43: gold mask with flame-shaped eye holes
FLAME_GOLD = (gold_ribbed(MASK6, ribs=2, step=10)
              + f'<path d="{FLAME_EYE_L}" fill="#1a0d02" transform="translate(402 444) scale(0.86) translate(-392 -440)"/>'
              + f'<path d="{FLAME_EYE_R}" fill="#1a0d02" transform="translate(626 444) scale(0.86) translate(-636 -440)"/>')
out("r6-43-royal-flame-eyes", ghost(FLAME_GOLD) + GLINTS)

# 44: gold calling card with the white masked ghost breaking out of it
rnd = random.Random(11)
x0, x1, y0, y1 = 512 - 250, 512 + 250, 512 - 300, 512 + 320
card = [(x0, y0), (x1 - 80, y0), (x1, y0 + 80), (x1, y1)]
for i in range(1, 13):
    x = x1 - (x1 - x0) * i / 13
    card.append((x, y1 + (rnd.uniform(-22, -4) if i % 2 else rnd.uniform(4, 16))))
card.append((x0, y1 - 10))
card = [(512 + (x - 512) * 0.84 - 12, 512 + (y - 512) * 0.84 - 12) for x, y in rot(card, -12)]
out("r6-44-royal-gold-card", poly([(x + 18, y + 18) for x, y in card], fill="#000") + gold_ribbed(card, ribs=3, step=14)
    + f"""<g transform="translate(556 468) rotate(8) scale(0.8) translate(-548 -524)">
  <path d="{GHOST}" fill="#000" transform="translate(20 20)"/><path d="{GHOST}" fill="#fff"/>{BLACK_MASK}</g>""" + GLINTS)

# 45: keyhole ghost (round 6's 35) with a gold cut-out shadow instead of a black one
R, NECK, LENGTH, FLARE, TEETH, DEPTH, TILT = 230, 160, 430, 210, 3, 60, -6
ny = math.sqrt(R * R - NECK * NECK)
HOLE = f"M{f(512 - NECK)},{f(ny)} A{R},{R} 0 1 1 {f(512 + NECK)},{f(ny)} L{f(512 + FLARE)},{LENGTH} "
seg = 2 * FLARE / TEETH
for i in range(TEETH):
    x = 512 + FLARE - seg * i
    HOLE += f"L{f(x - seg / 2)},{f(LENGTH - DEPTH)} L{f(x - seg)},{f(LENGTH + (DEPTH * 0.15 if i % 2 else 0))} "
HOLE += "Z"
S = 620 / (LENGTH + R)
k = 0.86
eyes = (f'<path d="{FLAME_EYE_L}" fill="#000" transform="translate({f(512 - 112 * k)} 8) scale({k}) translate(-392 -440)"/>'
        f'<path d="{FLAME_EYE_R}" fill="#000" transform="translate({f(512 + 112 * k)} 8) scale({k}) translate(-636 -440)"/>')
out("r6-45-royal-keyhole", f"""<g transform="translate(492 462) rotate({TILT}) scale({f(S)}) translate(-512 0)">
  <path d="{HOLE}" fill="#000" stroke="#000" stroke-width="{f(64 / S)}" stroke-linejoin="miter" stroke-miterlimit="1.6" transform="translate({f(34 / S)} {f(34 / S)})"/>
  <path d="{HOLE}" fill="url(#gold)" stroke="url(#gold)" stroke-width="{f(52 / S)}" stroke-linejoin="miter" stroke-miterlimit="1.6" transform="translate({f(26 / S)} {f(26 / S)})"/>
  <path d="{HOLE}" fill="#fff"/>{eyes}
</g>""" + GLINTS)
