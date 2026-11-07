import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as Effect from "effect/Effect";
import { Store } from "../store";
import { type Harness, open } from "./support/harness";

let root: string;
let directory: string;
let desk: Harness;

const store = <A, E>(f: (store: Store["Service"]) => Effect.Effect<A, E>) => desk.run(Store.use(f));

beforeEach(async () => {
	root = await mkdtemp(join(tmpdir(), "desk-store-"));
	directory = join(root, "coordination");
	desk = await open(root);
});
afterEach(async () => {
	await desk.close();
	await rm(root, { recursive: true, force: true });
});

const entryFiles = async () => (await readdir(directory)).filter((name) => name.endsWith(".md")).sort();

describe("store", () => {
	test("create writes one Markdown file per entry, numbered", async () => {
		const first = await store((s) => s.create({ asker: "main", title: "Which icon?", question: "Pick one.", labels: ["Design"] }));
		const second = await store((s) => s.create({ asker: "user", title: "Why?", question: "Because.", parent: first.id }));
		expect([first.id, second.id]).toEqual([1, 2]);
		expect(second).toMatchObject({ asker: "user", answerer: "main", status: "open", parent: 1 });
		expect(first.labels).toEqual(["design"]);
		expect(await entryFiles()).toEqual(["0001-which-icon.md", "0002-why.md"]);
		expect(await readFile(join(directory, "0001-which-icon.md"), "utf8")).toContain("# Which icon?\n\nPick one.");
	});

	test("ignores files that are not entries and never touches them", async () => {
		await writeFile(join(directory, "QUESTIONS.md"), "# Questions\n\nnotes");
		await writeFile(join(directory, "0009-looks-like-one.md"), "but has no header");
		await store((s) => s.reload);
		expect((await store((s) => s.snapshot)).entries).toEqual([]);
		await store((s) => s.create({ asker: "main", title: "Q", question: "" }));
		expect(await entryFiles()).toEqual(["0001-q.md", "0009-looks-like-one.md", "QUESTIONS.md"]);
	});

	test("ids are never reused after pruning", async () => {
		const first = await store((s) => s.create({ asker: "main", title: "One", question: "" }));
		await store((s) => s.update(first.id, (entry) => ({ ...entry, status: "resolved" })));
		await desk.until((snapshot) => snapshot.entries.length === 0);
		expect(await entryFiles()).toEqual([]);
		expect((await store((s) => s.create({ asker: "main", title: "Two", question: "" }))).id).toBe(2);
	});

	test("resolving prunes bottom-up, but an unresolved follow-up holds its ancestors", async () => {
		const root = await store((s) => s.create({ asker: "main", title: "Root", question: "" }));
		const child = await store((s) => s.create({ asker: "user", title: "Child", question: "", parent: root.id }));
		await store((s) => s.update(root.id, (entry) => ({ ...entry, status: "resolved" })));
		await Bun.sleep(100);
		expect((await store((s) => s.snapshot)).entries.map((entry) => entry.id)).toEqual([1, 2]);
		await store((s) => s.update(child.id, (entry) => ({ ...entry, status: "resolved" })));
		await desk.until((snapshot) => snapshot.entries.length === 0);
		expect(await entryFiles()).toEqual([]);
	});

	test("a follow-up needs an existing parent; a missing entry is a readable error", async () => {
		expect(await desk.failure(Store.use((s) => s.create({ asker: "main", title: "x", question: "", parent: 5 })))).toContain("No desk entry #5");
		expect(await desk.failure(Store.use((s) => s.get(3)))).toContain("No desk entry #3");
	});

	test("drafts persist across reopening and go away with their entry", async () => {
		const entry = await store((s) => s.create({ asker: "main", title: "Q", question: "" }));
		await store((s) => s.putDraft({ kind: "answer", entry: entry.id, text: "draft answer" }));
		await store((s) => s.putDraft({ kind: "ask", parent: null, title: "New one", text: "" }));
		await desk.close();
		desk = await open(root);
		expect(Object.keys((await store((s) => s.snapshot)).drafts).sort()).toEqual(["answer-1", "ask-top"]);
		await store((s) => s.putDraft({ kind: "ask", parent: null, title: " ", text: "" }));
		expect(Object.keys((await store((s) => s.snapshot)).drafts)).toEqual(["answer-1"]);
		await store((s) => s.update(entry.id, (e) => ({ ...e, status: "resolved" })));
		expect((await desk.until((snapshot) => snapshot.entries.length === 0)).drafts).toEqual({});
	});

	test("recreates its directories when they are deleted underneath it", async () => {
		await rm(directory, { recursive: true, force: true });
		expect((await store((s) => s.create({ asker: "main", title: "Again", question: "" }))).id).toBe(1);
		expect(await entryFiles()).toEqual(["0001-again.md"]);
	});

	test("main session persists", async () => {
		await store((s) => s.setMainSession("ses_1"));
		await desk.close();
		desk = await open(root);
		expect((await store((s) => s.snapshot)).mainSession).toBe("ses_1");
	});

	test("outside edits reach subscribers through the watcher", async () => {
		const entry = await store((s) => s.create({ asker: "main", title: "Edit me", question: "" }));
		const path = join(directory, "0001-edit-me.md");
		await writeFile(path, (await readFile(path, "utf8")).replace("status: open", "status: answered"));
		const snapshot = await desk.until((s) => s.entries[0]?.status === "answered");
		expect(snapshot.entries[0]).toMatchObject({ id: entry.id, status: "answered" });
	});

	test("the watcher follows a deleted and recreated directory", async () => {
		await store((s) => s.create({ asker: "main", title: "Before", question: "" }));
		await rm(directory, { recursive: true, force: true });
		await desk.until((s) => s.entries.length === 0);
		await Bun.sleep(300);
		const other = await mkdtemp(join(tmpdir(), "desk-elsewhere-"));
		await writeFile(
			join(other, "0007-dropped-in.md"),
			"---\nid: 7\nparent:\nasker: main\nanswerer: user\nstatus: open\nlabels: []\ncreated: 2026-01-01T00:00:00.000Z\n---\n\n# Dropped in\n",
		);
		await Bun.write(join(directory, "0007-dropped-in.md"), Bun.file(join(other, "0007-dropped-in.md")));
		expect((await desk.until((s) => s.entries.length === 1)).entries[0]?.title).toBe("Dropped in");
		await rm(other, { recursive: true, force: true });
	});
});
