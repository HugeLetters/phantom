import { describe, expect, test } from "bun:test";
import * as Option from "effect/Option";
import { type Entry, fileName, isEntryFileName, parse, serialize, waitingOn } from "../entry";

const entry: Entry = {
	id: 7,
	parent: 3,
	asker: "main",
	answerer: "user",
	status: "answered",
	labels: ["design", "icons"],
	created: "2026-11-07T16:30:00.000Z",
	title: "Which icon?",
	question: "Pick one:\n\n![a](design/icons/a.png)\n\n## Options\n\n- a\n- b",
	answer: "The angular one.",
};

describe("entry file", () => {
	test("round-trips through Markdown", () => {
		expect(parse(serialize(entry))).toEqual(Option.some(entry));
	});

	test("has the documented header", () => {
		expect(serialize(entry)).toStartWith(
			"---\nid: 7\nparent: 3\nasker: main\nanswerer: user\nstatus: answered\nlabels: [design, icons]\ncreated: 2026-11-07T16:30:00.000Z\n---\n\n# Which icon?\n",
		);
	});

	test("a question without an answer and parent round-trips", () => {
		const open = { ...entry, parent: null, status: "open" as const, answer: null, labels: [] };
		expect(parse(serialize(open))).toEqual(Option.some(open));
	});

	test("an Answer heading inside the question stays in the question", () => {
		const tricky = { ...entry, question: "## Answer\n\nnot really" };
		expect(parse(serialize(tricky))).toEqual(Option.some(tricky));
	});

	test("files that are not entries parse to None", () => {
		expect(parse("# QUESTIONS\n\nfree-form notes")).toEqual(Option.none());
		expect(parse("---\nid: x\n---\n# t")).toEqual(Option.none());
		expect(parse("---\nid: 1\nasker: user\nanswerer: main\nstatus: maybe\ncreated: x\n---\n# t")).toEqual(Option.none());
	});

	test("file names are numbered slugs; other names are not entries", () => {
		expect(fileName(entry)).toBe("0007-which-icon.md");
		expect(fileName({ id: 12345, title: "Über ünïcode!!" })).toBe("12345-ber-n-code.md");
		expect(isEntryFileName("0007-which-icon.md")).toBe(true);
		expect(isEntryFileName("QUESTIONS.md")).toBe(false);
		expect(isEntryFileName("0007-which-icon.md.123.tmp")).toBe(false);
	});

	test("whose move it is", () => {
		expect(waitingOn({ ...entry, status: "open" })).toBe("user");
		expect(waitingOn({ ...entry, status: "answered" })).toBe("main");
		expect(waitingOn({ ...entry, status: "resolved" })).toBe(null);
	});
});
