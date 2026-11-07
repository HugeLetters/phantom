import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as Effect from "effect/Effect";
import * as Scope from "effect/Scope";
import * as Exit from "effect/Exit";
import { make, type Store, type Snapshot } from "../store";

let directory: string;
let scope: Scope.Closeable;
let store: Store;

const run = <A, E>(effect: Effect.Effect<A, E>) => Effect.runPromise(effect);
const open = async () => {
	scope = Effect.runSync(Scope.make());
	store = await run(Scope.provide(scope)(make(directory)));
};

beforeEach(async () => {
	directory = await mkdtemp(join(tmpdir(), "desk-store-"));
	await open();
});
afterEach(async () => {
	await run(Scope.close(scope, Exit.void));
	await rm(directory, { recursive: true, force: true });
});

const entryFiles = async () => (await readdir(directory)).filter((name) => name.endsWith(".md")).sort();

describe("store", () => {
	test("create writes one Markdown file per entry, numbered", async () => {
		const first = await run(store.create({ asker: "main", title: "Which icon?", question: "Pick one.", labels: ["Design"] }));
		const second = await run(store.create({ asker: "user", title: "Why?", question: "Because.", parent: first.id }));
		expect([first.id, second.id]).toEqual([1, 2]);
		expect(second).toMatchObject({ asker: "user", answerer: "main", status: "open", parent: 1 });
		expect(first.labels).toEqual(["design"]);
		expect(await entryFiles()).toEqual(["0001-which-icon.md", "0002-why.md"]);
		expect(await readFile(join(directory, "0001-which-icon.md"), "utf8")).toContain("# Which icon?\n\nPick one.");
	});

	test("ignores files that are not entries and never touches them", async () => {
		await writeFile(join(directory, "QUESTIONS.md"), "# Questions\n\nnotes");
		await writeFile(join(directory, "0009-looks-like-one.md"), "but has no header");
		await run(store.reload);
		expect(store.snapshot().entries).toEqual([]);
		await run(store.create({ asker: "main", title: "Q", question: "" }));
		expect(await entryFiles()).toEqual(["0001-q.md", "0009-looks-like-one.md", "QUESTIONS.md"]);
	});

	test("ids are never reused after pruning", async () => {
		const first = await run(store.create({ asker: "main", title: "One", question: "" }));
		await run(store.update(first.id, (entry) => ({ ...entry, status: "resolved" })));
		expect(await entryFiles()).toEqual([]);
		expect((await run(store.create({ asker: "main", title: "Two", question: "" }))).id).toBe(2);
	});

	test("resolving prunes bottom-up, but an unresolved follow-up holds its ancestors", async () => {
		const root = await run(store.create({ asker: "main", title: "Root", question: "" }));
		const child = await run(store.create({ asker: "user", title: "Child", question: "", parent: root.id }));
		await run(store.update(root.id, (entry) => ({ ...entry, status: "resolved" })));
		expect(store.snapshot().entries.map((entry) => entry.id)).toEqual([1, 2]);
		await run(store.update(child.id, (entry) => ({ ...entry, status: "resolved" })));
		expect(store.snapshot().entries).toEqual([]);
		expect(await entryFiles()).toEqual([]);
	});

	test("a follow-up needs an existing parent; a missing entry is a readable error", async () => {
		const failed = await Effect.runPromiseExit(store.create({ asker: "main", title: "x", question: "", parent: 5 }));
		expect(Exit.isFailure(failed) && String(failed.cause)).toContain("No desk entry #5");
		const missing = await Effect.runPromiseExit(store.get(3));
		expect(Exit.isFailure(missing) && String(missing.cause)).toContain("No desk entry #3");
	});

	test("drafts persist across reopening and go away with their entry", async () => {
		const entry = await run(store.create({ asker: "main", title: "Q", question: "" }));
		await run(store.putDraft({ kind: "answer", entry: entry.id, text: "draft answer" }));
		await run(store.putDraft({ kind: "ask", parent: null, title: "New one", text: "" }));
		await run(Scope.close(scope, Exit.void));
		await open();
		expect(Object.keys(store.snapshot().drafts).sort()).toEqual(["answer-1", "ask-top"]);
		await run(store.putDraft({ kind: "ask", parent: null, title: " ", text: "" }));
		expect(Object.keys(store.snapshot().drafts)).toEqual(["answer-1"]);
		await run(store.update(entry.id, (e) => ({ ...e, status: "resolved" })));
		expect(store.snapshot().drafts).toEqual({});
	});

	test("recreates its directories when they are deleted underneath it", async () => {
		await rm(directory, { recursive: true, force: true });
		expect((await run(store.create({ asker: "main", title: "Again", question: "" }))).id).toBe(1);
		expect(await entryFiles()).toEqual(["0001-again.md"]);
	});

	test("main session persists", async () => {
		await run(store.setMainSession("ses_1"));
		await run(Scope.close(scope, Exit.void));
		await open();
		expect(store.snapshot().mainSession).toBe("ses_1");
	});

	test("outside edits reach subscribers through the watcher", async () => {
		const seen: Array<Snapshot> = [];
		const stop = store.subscribe((snapshot) => seen.push(snapshot));
		const entry = await run(store.create({ asker: "main", title: "Edit me", question: "" }));
		const path = join(directory, "0001-edit-me.md");
		await writeFile(path, (await readFile(path, "utf8")).replace("status: open", "status: answered"));
		for (let i = 0; i < 40 && store.snapshot().entries[0]?.status !== "answered"; i++) await Bun.sleep(25);
		stop();
		expect(store.snapshot().entries[0]).toMatchObject({ id: entry.id, status: "answered" });
		expect(seen.length).toBeGreaterThanOrEqual(2);
	});
});
