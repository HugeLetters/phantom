/**
 * A reproducible sample desk for the artifacts, generated from a seed: entries, ids, the follow-up tree and the
 * images all come from a seeded generator, so only the seed is committed. Roles the screenshots need (images, video,
 * a diagram, the walk-through's entries) are fixed; everything else is random.
 */
import type { Entry } from "../entry";

/** mulberry32: small, fast, good enough to vary sample data. */
function generator(seed: number) {
	let state = seed >>> 0;
	const next = () => {
		state = (state + 0x6d2b79f5) >>> 0;
		let t = Math.imul(state ^ (state >>> 15), 1 | state);
		t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
	const int = (below: number) => Math.floor(next() * below);
	const pick = <A>(items: ReadonlyArray<A>) => items[int(items.length)]!;
	const hex = (length: number) => Array.from({ length }, () => int(16).toString(16)).join("");
	return { int, pick, uuid: () => `${hex(8)}-${hex(4)}-4${hex(3)}-${pick(["8", "9", "a", "b"])}${hex(3)}-${hex(12)}` };
}

const topics = ["the torrent list", "magnet links", "the menu bar icon", "seeding limits", "tracker errors", "label colors", "the download folder", "Check", "paused torrents", "the sidebar"];
const changes = ["remember its sort order", "show a badge", "move to Settings", "use the system accent color", "retry on wake", "warn before removing data", "group by label"];
const labels = ["design", "review", "glossary", "service", "gui", "destinations"];
const states = ["Queued", "Downloading", "Seeding", "Paused", "Checking", "Missing", "Done"];
const colors = ["#e5262f", "#111111", "#2563eb", "#059669", "#f59e0b", "#8250df"];

export interface Sample {
	readonly entries: ReadonlyArray<Entry>;
	/** SVG files for `coordination/media/`. */
	readonly images: Record<string, string>;
	/** Entries the stills and the walk-through visit. */
	readonly roles: { images: string; video: string; diagram: string; family: string; walk: string; mine: string };
}

export function sample(seed: number): Sample {
	const { int, pick, uuid } = generator(seed);
	const entries: Array<Entry> = [];
	const base = Date.parse("2026-11-07T16:00:00.000Z");
	const add = (fields: Partial<Entry> & Pick<Entry, "title" | "question">): Entry => {
		const entry: Entry = {
			id: uuid(),
			parent: null,
			asker: "agent",
			status: "open",
			session: "ses_main",
			labels: [pick(labels)],
			created: new Date(base + entries.length * 60_000).toISOString(),
			answer: null,
			...fields,
		};
		entries.push(entry);
		return entry;
	};
	const question = () => `Should ${pick(topics)} ${pick(changes)}?`;
	const prose = () => `Today ${pick(topics)} doesn't ${pick(changes)}; ${pick(topics)} does. I recommend keeping them consistent.`;

	const images = Object.fromEntries(
		["a", "b", "c"].map((name) => {
			const [fill, mark] = [pick(colors), pick(colors)];
			const shape = int(2) ? `<circle cx="64" cy="60" r="${24 + int(16)}" fill="${mark}"/>` : `<path d="M64 26v52M40 56l24 24 24-24" stroke="${mark}" stroke-width="12" fill="none" stroke-linecap="round"/>`;
			return [`${name}.svg`, `<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 128 128"><rect width="128" height="128" rx="28" fill="${fill}"/>${shape}<rect x="34" y="96" width="60" height="10" rx="5" fill="#fff"/></svg>`];
		}),
	);
	const imagesEntry = add({ title: "Which icon should ship?", question: `Three candidates:\n\n| a | b | c |\n|---|---|---|\n| ![a](coordination/media/a.svg) | ![b](coordination/media/b.svg) | ![c](coordination/media/c.svg) |\n\nI recommend **${pick(["a", "b", "c"])}**.` });
	const video = add({ title: `Review PR #${30 + int(60)}: ${pick(topics)}`, question: `${prose()}\n\n![demo](coordination/media/demo.gif)\n\nFull recording:\n\n![recording](coordination/media/demo.mp4)`, labels: ["review"] });
	const answered = add({ title: question(), question: prose(), status: "answered", answer: `Yes, ${pick(changes)}.` });
	const flow = Array.from({ length: 4 }, () => pick(states));
	const diagram = add({
		title: question(),
		parent: answered.id,
		question: `The states involved:\n\n\`\`\`mermaid\nstateDiagram-v2\n    [*] --> ${flow[0]}\n    ${flow[0]} --> ${flow[1]}\n    ${flow[1]} --> ${flow[2]}\n    ${flow[1]} --> ${flow[3]}\n\`\`\`\n\n\`\`\`rust\nfn ${pick(["check", "pause", "resume"])}(torrent: &Torrent) -> bool {\n    torrent.pieces().all(|piece| piece.verify())\n}\n\`\`\``,
	});
	for (let i = 0; i < 4; i++) add({ title: question(), question: prose(), parent: int(2) ? pick(entries).id : null });
	const family = add({ title: question(), question: prose(), parent: pick(entries.slice(4)).id });
	add({ title: question(), question: prose(), parent: family.id });
	const walk = add({ title: question(), question: prose(), parent: answered.id });
	const mine = add({ title: `Why does ${pick(topics)} ${pick(changes)}?`, question: prose(), asker: "user", status: "answered", answer: prose() });
	return { entries, images, roles: { images: imagesEntry.id, video: video.id, diagram: diagram.id, family: family.id, walk: walk.id, mine: mine.id } };
}
