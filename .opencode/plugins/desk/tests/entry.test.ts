import { expect, test } from "bun:test";
import * as Option from "effect/Option";
import { type Entry, fileName, idOf, next, parse, serialize, shortId, waitingOn } from "../entry";

const id = "5c0f9a1e-2b3d-4e5f-8a9b-0c1d2e00a7a7";
const parent = "0d9e8f7a-6b5c-4d3e-9f2a-1b0c9d003333";
const entry: Entry = {
	id,
	parent,
	asker: "agent",
	status: "answered",
	session: "ses_1",
	labels: ["design", "icons"],
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
	expect(next([a, b, c], { id: "gone", created: "1.5" })?.id).toBe("b"); // the current entry already left the desk
	expect(next([a, c], a)).toBeUndefined(); // nothing else waits on the user
	expect(next([c], { id: "gone", created: "9" })).toBeUndefined();
});
