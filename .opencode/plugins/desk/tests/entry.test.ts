import { expect, test } from "bun:test";
import * as Option from "effect/Option";
import { type Entry, fileName, idOf, next, openBelow, ordered, parse, serialize, shortId, waitingOn } from "../entry";

const id = "5c0f9a1e-2b3d-4e5f-8a9b-0c1d2e00a7a7";
const parent = "0d9e8f7a-6b5c-4d3e-9f2a-1b0c9d003333";
const entry: Entry = {
	id,
	parent,
	asker: "agent",
	status: "answered",
	session: "ses_1",
	labels: ["design", "icons"],
	priority: "normal",
	created: "2026-11-07T16:30:00.000Z",
	title: "Which icon?",
	question: "Pick one:\n\n![a](design/a.png)\n\n## Options\n\n- a\n- b",
	answer: "The angular one.",
};

test("an entry round-trips through its file", () => {
	expect(parse(id, serialize(entry))).toEqual(Option.some(entry));
	const open = { ...entry, parent: null, status: "open" as const, answer: null, labels: [] };
	expect(parse(id, serialize(open))).toEqual(Option.some(open));
});

test("the file has a YAML header; the id is the file name", () => {
	expect(serialize(entry)).toStartWith(
		`---\nparent: ${parent}\nasker: agent\nstatus: answered\nsession: ses_1\nlabels: [design, icons]\ncreated: "2026-11-07T16:30:00.000Z"\n---\n\n# Which icon?\n`,
	);
	expect(fileName(id)).toBe(`${id}.md`);
	expect([idOf(`${id}.md`), idOf("0007.md"), idOf("QUESTIONS.md"), idOf(`${id}-x.md`)]).toEqual([id, undefined, undefined, undefined]);
	expect(shortId(id)).toBe("00a7a7");
});

test("files that aren't entries parse to None", () => {
	expect(parse(id, "# Notes\n\nfree-form")).toEqual(Option.none());
	expect(parse(id, "---\nasker: someone\n---\n\n# t")).toEqual(Option.none());
});

test("whose move it is", () => {
	expect(waitingOn({ ...entry, status: "open" })).toBe("user");
	expect(waitingOn({ ...entry, status: "answered" })).toBe("agent");
	expect(waitingOn({ ...entry, asker: "user", status: "open" })).toBe("agent");
	expect(waitingOn({ ...entry, status: "resolved" })).toBe(null);
});

test("Next: the next entry waiting on the user by `created`, wrapping round; none left means back to the list", () => {
	const at = (id: string, created: string, status: Entry["status"] = "open") => ({ ...entry, id, created, status, parent: null });
	const [a, b, c] = [at("a", "1"), at("b", "2"), at("c", "3", "answered")]; // c waits on the agents
	expect(next([a, b, c], a)?.id).toBe("b");
	expect(next([a, b, c], b)?.id).toBe("a"); // wraps round
	expect(next([a, b, c], { id: "gone", created: "1.5", priority: "normal" })?.id).toBe("b"); // the current entry already left the desk
	expect(next([a, c], a)).toBeUndefined(); // nothing else waits on the user
	expect(next([c], { id: "gone", created: "9", priority: "normal" })).toBeUndefined();
});

test("Open further down: entries below the direct children that wait on the user, by `created`", () => {
	const e = (id: string, parent: string | null, created: string, status: Entry["status"] = "open", asker: Entry["asker"] = "agent"): Entry => ({
		...entry,
		id,
		parent,
		created,
		status,
		asker,
		answer: null,
	});
	const entries = [
		e("root", null, "1"),
		e("child", "root", "2"), // a direct child: the Follow-ups list shows it
		e("deep2", "child", "6"),
		e("deep1", "child", "3"),
		e("grand", "deep1", "4", "answered"), // waits on the agent
		e("deepest", "grand", "5", "answered", "user"), // waits on the user, four levels down
		e("done", "child", "7", "resolved"),
		e("other", null, "0"),
		e("elsewhere", "other", "1"),
		e("elsewhere2", "elsewhere", "1"),
	];
	expect(openBelow(entries, { id: "root" }).map((x) => x.id)).toEqual(["deep1", "deepest", "deep2"]);
	expect(openBelow(entries, { id: "deepest" })).toEqual([]);
	expect(openBelow(entries, { id: "elsewhere" }).map((x) => x.id)).toEqual([]);
});

test("priority: absent means normal; only critical, high and low are written", () => {
	expect(serialize(entry)).not.toContain("priority");
	expect(parse(id, serialize(entry))).toEqual(Option.some(entry));
	for (const priority of ["critical", "high", "low"] as const) {
		const text = serialize({ ...entry, priority });
		expect(text).toContain(`\npriority: ${priority}\n`);
		expect(parse(id, text)).toEqual(Option.some({ ...entry, priority }));
	}
});

test("order: critical > high > normal > low, then `created`", () => {
	const at = (id: string, created: string, priority: Entry["priority"]) => ({ ...entry, id, created, priority, parent: null, status: "open" as const });
	const all = [at("low", "0", "low"), at("normal", "1", "normal"), at("high", "2", "high"), at("late-critical", "9", "critical"), at("critical", "3", "critical")];
	expect(ordered(all).map((e) => e.id)).toEqual(["critical", "late-critical", "high", "normal", "low"]);
	expect(next(all, all[4]!)?.id).toBe("late-critical");
	expect(next(all, all[3]!)?.id).toBe("high");
});

test("order and Next: priority first, then `created`", () => {
	const at = (id: string, created: string, priority: Entry["priority"]) => ({ ...entry, id, created, priority, parent: null, status: "open" as const });
	const [late, early, low, high] = [at("late-high", "9", "high"), at("early", "1", "normal"), at("low", "0", "low"), at("high", "5", "high")];
	expect(ordered([low, early, late, high]).map((e) => e.id)).toEqual(["high", "late-high", "early", "low"]);
	expect(next([low, early, late, high], high)?.id).toBe("late-high");
	expect(next([low, early, late, high], low)?.id).toBe("high"); // wraps round to the top
	expect(next([low, early, late, high], { id: "gone", created: "6", priority: "high" })?.id).toBe("late-high");
});
