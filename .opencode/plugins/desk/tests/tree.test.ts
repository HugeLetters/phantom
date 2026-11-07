import { describe, expect, test } from "bun:test";
import type { Entry, Party, Status } from "../entry";
import { build, type Node, prunable, queue, thread, waitingCount } from "../tree";

function entry(id: number, status: Status, parent: number | null = null, asker: Party = "main"): Entry {
	return {
		id,
		parent,
		asker,
		answerer: asker === "main" ? "user" : "main",
		status,
		labels: [],
		created: "2026-11-07T00:00:00.000Z",
		title: `#${id}`,
		question: "",
		answer: null,
	};
}

const ids = (nodes: ReadonlyArray<Node>): unknown =>
	nodes.map((node) => (node.children.length ? [node.entry.id, ids(node.children)] : node.entry.id));

describe("tree", () => {
	test("follow-ups hang under their parent; missing parents make roots", () => {
		expect(ids(build([entry(1, "open"), entry(2, "open", 1), entry(3, "open", 99), entry(4, "open", 2)]))).toEqual([
			[1, [[2, [4]]]],
			3,
		]);
	});

	test("roots waiting on the user come first, then unresolved, then resolved", () => {
		const entries = [
			entry(1, "resolved"),
			entry(2, "open", null, "user"), // waits on main
			entry(3, "resolved"),
			entry(4, "open", 3), // waits on the user, under a resolved root
			entry(5, "answered", null, "user"), // waits on the user
		];
		expect(ids(build(entries))).toEqual([[3, [4]], 5, 2, 1]);
	});

	test("a parent cycle does not hang or drop entries", () => {
		expect(ids(build([entry(1, "open", 2), entry(2, "open", 1), entry(3, "open", 1)]))).toEqual([[1, [3]], 2]);
	});

	test("thread lists ancestors root first", () => {
		const entries = [entry(1, "open"), entry(2, "open", 1), entry(3, "open", 2)];
		expect(thread(entries, 3).map((e) => e.id)).toEqual([1, 2, 3]);
		expect(thread(entries, 9)).toEqual([]);
	});

	test("the queue is flat at any depth: waiting on the user first, then on the agents, oldest first; no resolved", () => {
		const entries = [
			entry(1, "resolved"),
			entry(2, "answered", 1), // waits on main
			entry(3, "open", 2), // waits on the user, two levels deep
			entry(4, "open", null, "user"), // waits on main
			entry(5, "answered", 4, "user"), // waits on the user
			entry(6, "resolved", 5),
		];
		expect(queue(entries)).toEqual([3, 5, 2, 4]);
	});

	test("waiting counts per side", () => {
		const entries = [entry(1, "open"), entry(2, "answered", null, "user"), entry(3, "answered"), entry(4, "resolved")];
		expect(waitingCount(entries, "user")).toBe(2);
		expect(waitingCount(entries, "main")).toBe(1);
	});
});

describe("pruning", () => {
	test("resolved leaves go, bottom-up through resolved ancestors", () => {
		const entries = [entry(1, "resolved"), entry(2, "resolved", 1), entry(3, "resolved", 2)];
		expect(prunable(entries)).toEqual([3, 2, 1]);
	});

	test("an unresolved follow-up keeps every resolved ancestor", () => {
		const entries = [entry(1, "resolved"), entry(2, "resolved", 1), entry(3, "answered", 2), entry(4, "resolved", 1)];
		expect(prunable(entries)).toEqual([4]);
	});

	test("nothing goes before the asker resolves it", () => {
		expect(prunable([entry(1, "answered"), entry(2, "open"), entry(3, "answered", 1)])).toEqual([]);
	});
});
