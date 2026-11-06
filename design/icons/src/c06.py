from lib import *

cx, top, r = 512, 200, 210
outline = []
for i in range(7):  # dome, left -> right
    a = math.pi + math.pi * i / 6
    outline.append((cx + r * math.cos(a), top + r + r * math.sin(a)))
outline += [(cx + r + 10, 610)]
hem = [(cx + r + 50, 820), (cx + 110, 720), (cx, 830), (cx - 110, 720), (cx - r - 50, 820)]
outline += hem + [(cx - r - 10, 610)]
eyes = [(cx - 88, 440), (cx + 88, 440)]
core = (cx, 590)
nodes = outline + eyes + [core]
N = len(outline)
edges = [(i, (i + 1) % N) for i in range(N)]
E1, E2, C = N, N + 1, N + 2
edges += [(E1, 1), (E1, 0), (E2, 5), (E2, 6), (E1, 3), (E2, 3),
          (E1, C), (E2, C), (C, 9), (C, 11)]

lines = "".join(f'<line x1="{f(nodes[a][0])}" y1="{f(nodes[a][1])}" x2="{f(nodes[b][0])}" y2="{f(nodes[b][1])}"/>' for a, b in edges)
dots = "".join(f'<circle cx="{f(x)}" cy="{f(y)}" r="30"/>' for x, y in outline + [core])
pupils = "".join(f'<circle cx="{f(x + 12)}" cy="{f(y + 6)}" r="20"/>' for x, y in eyes)
def almond(x, y, side):
    k = side  # -1 left eye, +1 right eye; inner corner low and pointed
    return (f'<path d="M{f(x - 76 * k)},{f(y - 46)} L{f(x + 70 * k)},{f(y + 8)} '
            f'C{f(x + 40 * k)},{f(y + 66)} {f(x - 60 * k)},{f(y + 56)} {f(x - 76 * k)},{f(y - 46)} Z"/>')
almonds = almond(*eyes[0], 1) + almond(*eyes[1], -1)
eyedots = "".join(f'<circle cx="{f(x)}" cy="{f(y)}" r="50"/>' for x, y in eyes)

defs = '<filter id="glow" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="18"/></filter>'
art = f"""
<g stroke-linecap="round">
  {poly(outline, fill=RED, opacity="0.4")}
  <g stroke="#ff2a4d" stroke-width="18">{lines}</g>
  <g fill="#fff">{dots}</g>
  <g fill="#fff">{almonds}</g>
</g>"""
body = f"""
<rect x="0" y="0" width="1024" height="1024" fill="#0b0508"/>
<circle cx="512" cy="540" r="420" fill="#3a0010" opacity="0.6" filter="url(#glow)"/>
<g transform="translate(512 530) scale(0.8) translate(-512 -512)">
<g filter="url(#glow)" opacity="0.9">{art}</g>
{art}
</g>
"""
write("06-swarm-ghost", svg(body, defs), sizes=(1024, 64, 32))
preview("06-swarm-ghost")
