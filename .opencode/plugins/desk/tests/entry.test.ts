import { expect, test } from "bun:test";
import * as Option from "effect/Option";
import { type Entry, fileName, idOf, parse, serialize, shortId, waitingOn } from "../entry";

const id = "019a5f3c-0000-7000-8000-00000000a7a7";
const parent = "019a5f3b-0000-7000-8000-000000003333";
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

test("the file has a YAML header; the UUIDv7 id is the file name", () => {
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
