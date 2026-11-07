import { expect, test } from "bun:test";
import * as Option from "effect/Option";
import { type Entry, fileName, idOf, parse, serialize, waitingOn } from "../entry";

const entry: Entry = {
	id: 7,
	parent: 3,
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
	expect(parse(7, serialize(entry))).toEqual(Option.some(entry));
	const open = { ...entry, parent: null, status: "open" as const, answer: null, labels: [] };
	expect(parse(7, serialize(open))).toEqual(Option.some(open));
});

test("the file has a YAML header; the id is the file name", () => {
	expect(serialize(entry)).toStartWith(
		'---\nparent: 3\nasker: agent\nstatus: answered\nsession: ses_1\nlabels: [design, icons]\ncreated: "2026-11-07T16:30:00.000Z"\n---\n\n# Which icon?\n',
	);
	expect(fileName(7)).toBe("0007.md");
	expect([idOf("0007.md"), idOf("12345.md"), idOf("QUESTIONS.md"), idOf("0007-x.md")]).toEqual([7, 12345, undefined, undefined]);
});

test("files that aren't entries parse to None", () => {
	expect(parse(1, "# Notes\n\nfree-form")).toEqual(Option.none());
	expect(parse(1, "---\nasker: someone\n---\n\n# t")).toEqual(Option.none());
});

test("whose move it is", () => {
	expect(waitingOn({ ...entry, status: "open" })).toBe("user");
	expect(waitingOn({ ...entry, status: "answered" })).toBe("agent");
	expect(waitingOn({ ...entry, asker: "user", status: "open" })).toBe("agent");
	expect(waitingOn({ ...entry, status: "resolved" })).toBe(null);
});
