// Pixel measurements on rendered frames, for comparing sketch options.
//
//   bun tools/px.ts sky <png>...       share of sky pixels that are light cloud, in %
//   bun tools/px.ts diff <a> <b> [t]   pixels whose grey level differs by more than t (2)
//   bun tools/px.ts mean <png>...      mean grey level
//
// Frames are decoded by ffmpeg into raw RGB; the counting runs on typed arrays.

async function rgb(path: string): Promise<Uint8Array> {
  const p = Bun.spawn(["ffmpeg", "-loglevel", "error", "-i", path, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"], {
    stdout: "pipe",
  });
  const buf = new Uint8Array(await new Response(p.stdout).arrayBuffer());
  if ((await p.exited) !== 0) throw new Error(`ffmpeg failed on ${path}`);
  return buf;
}

// Sky: blue clearly above red and green; light cloud: blue above 150.
function sky(d: Uint8Array): number {
  let sky = 0;
  let light = 0;
  for (let i = 0; i < d.length; i += 3) {
    const r = d[i], g = d[i + 1], b = d[i + 2];
    if (b > r + 20 && b > g) {
      sky++;
      if (b > 150) light++;
    }
  }
  return (light / sky) * 100;
}

const grey = (d: Uint8Array, i: number) => (d[i] * 299 + d[i + 1] * 587 + d[i + 2] * 114) / 1000;

const [cmd, ...args] = Bun.argv.slice(2);
if (cmd === "sky") {
  for (const f of args) console.log(f, sky(await rgb(f)).toFixed(2));
} else if (cmd === "mean") {
  for (const f of args) {
    const d = await rgb(f);
    let s = 0;
    for (let i = 0; i < d.length; i += 3) s += grey(d, i);
    console.log(f, (s / (d.length / 3)).toFixed(2));
  }
} else if (cmd === "diff") {
  const [a, b, t = "2"] = args;
  const [da, db] = await Promise.all([rgb(a), rgb(b)]);
  let n = 0;
  for (let i = 0; i < da.length; i += 3) if (Math.abs(grey(da, i) - grey(db, i)) > Number(t)) n++;
  console.log(n);
} else {
  console.error("usage: bun tools/px.ts sky|mean <png>... | diff <a> <b> [threshold]");
  process.exit(1);
}
