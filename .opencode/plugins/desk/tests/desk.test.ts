import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as Layer from "effect/Layer";
import * as Scope from "effect/Scope";
import * as App from "../app";
import { Desk } from "../desk";
import { PageUrl } from "../http/server";
import { MainSession } from "../main-session";
import { Store } from "../store";
import { fakeSessions, type Harness, open } from "./support/harness";

let root: string;
let desk: Harness | undefined;

const setup = async (options?: Parameters<typeof open>[1]) => (desk = await open(root, options));
const ask = (title: string, extra: { parent?: number; labels?: Array<string> } = {}) => Desk.use((d) => d.ask({ title, body: "", ...extra }));

beforeEach(async () => {
	root = await mkdtemp(join(tmpdir(), "desk-"));
});
afterEach(async () => {
	await desk?.close();
	desk = undefined;
	await rm(root, { recursive: true, force: true });
});

describe("main session", () => {
	test("is the top-level session of the first desk tool caller, and sticks", async () => {
		const { run } = await setup({ parents: { ses_sub: "ses_root" } });
		expect(await run(MainSession.use((main) => main.claim("ses_sub")))).toBe("ses_root");
		expect(await run(MainSession.use((main) => main.claim("ses_other")))).toBe("ses_root");
	});

	test("a configured main session wins", async () => {
		const { run } = await setup({ mainSession: "ses_other" });
		expect(await run(MainSession.use((main) => main.claim("ses_root")))).toBe("ses_other");
	});

	test("the user can pick another one on the page", async () => {
		const { run } = await setup({ parents: { ses_sub: "ses_other" } });
		await run(MainSession.use((main) => main.claim("ses_root")));
		await run(MainSession.use((main) => main.select("ses_sub")));
		expect(await run(MainSession.use((main) => main.current))).toBe("ses_other");
	});

	test("without one, sending explains how it gets known", async () => {
		const { run, failure } = await setup();
		await run(Desk.use((d) => d.putDraft({ kind: "ask", parent: null, title: "Q", text: "" })));
		expect(await failure(Desk.use((d) => d.submit))).toContain("No main session yet");
	});
});

describe("agent side", () => {
	test("ask, then the user answers, then the agents resolve: pruned", async () => {
		const { run, prompts, until } = await setup();
		await run(MainSession.use((main) => main.claim("ses_root")));
		const entry = await run(Desk.use((d) => d.ask({ title: "Which icon?", body: "a or b", labels: ["design"] })));
		expect(entry).toMatchObject({ asker: "main", answerer: "user", status: "open" });
		await run(Desk.use((d) => d.putDraft({ kind: "answer", entry: entry.id, text: "b" })));
		await run(Desk.use((d) => d.submit));
		expect((await run(Store.use((s) => s.snapshot))).entries[0]).toMatchObject({ status: "answered", answer: "b" });
		expect(prompts).toHaveLength(1);
		await run(Desk.use((d) => d.resolve(entry.id)));
		await until((snapshot) => snapshot.entries.length === 0);
	});

	test("agents can't answer their own question or resolve the user's", async () => {
		const { run, failure } = await setup();
		const mine = await run(ask("Mine"));
		expect(await failure(Desk.use((d) => d.answer(mine.id, "x")))).toContain("the user answers it");
		const theirs = await run(Store.use((s) => s.create({ asker: "user", title: "Theirs", question: "" })));
		expect(await failure(Desk.use((d) => d.resolve(theirs.id)))).toContain("only the asker marks it done");
		expect(await run(Desk.use((d) => d.answer(theirs.id, "here")))).toMatchObject({ status: "answered", answer: "here" });
	});

	test("describe outlines unresolved entries, or one thread in full", async () => {
		const { run } = await setup();
		const first = await run(Desk.use((d) => d.ask({ title: "Root question", body: "Details" })));
		await run(Desk.use((d) => d.ask({ title: "Follow-up", body: "More", parent: first.id, labels: ["review"] })));
		expect(await run(Desk.use((d) => d.describe()))).toBe(
			"- #1 Root question — open, waiting on user (asked by agents)\n  - #2 Follow-up — open, waiting on user (asked by agents) [review]\n\nWaiting on the user: 2. Waiting on the agents: 0.",
		);
		const thread = await run(Desk.use((d) => d.describe(2)));
		expect(thread.indexOf("# Root question")).toBeGreaterThanOrEqual(0);
		expect(thread.indexOf("# Root question")).toBeLessThan(thread.indexOf("# Follow-up"));
	});
});

describe("user side", () => {
	test("Submit queues one message with every draft; new questions become entries", async () => {
		const { run, prompts } = await setup();
		await run(MainSession.use((main) => main.claim("ses_root")));
		const a = await run(ask("A?"));
		const b = await run(ask("B?"));
		await run(Desk.use((d) => d.putDraft({ kind: "answer", entry: a.id, text: "yes" })));
		await run(Desk.use((d) => d.putDraft({ kind: "answer", entry: b.id, text: "no" })));
		await run(Desk.use((d) => d.putDraft({ kind: "ask", parent: a.id, title: "Why A?", text: "Explain." })));
		expect(await run(Desk.use((d) => d.submit))).toBe(3);
		expect(prompts).toHaveLength(1);
		expect(prompts[0]).toMatchObject({ sessionID: "ses_root", delivery: "queue" });
		expect(prompts[0]!.text).toContain("## #1 Answered: A?\n\nyes");
		expect(prompts[0]!.text).toContain("## #2 Answered: B?\n\nno");
		expect(prompts[0]!.text).toContain("## #3 New question from the user: Why A? (follow-up to #1)\n\nExplain.");
		const snapshot = await run(Store.use((s) => s.snapshot));
		expect(snapshot.drafts).toEqual({});
		expect(snapshot.entries.find((entry) => entry.id === 3)).toMatchObject({ asker: "user", answerer: "main", parent: 1 });
	});

	test("Send delivers one draft with steer and leaves the others", async () => {
		const { run, prompts } = await setup();
		await run(MainSession.use((main) => main.claim("ses_root")));
		const a = await run(ask("A?"));
		const b = await run(ask("B?"));
		await run(Desk.use((d) => d.putDraft({ kind: "answer", entry: a.id, text: "urgent" })));
		await run(Desk.use((d) => d.putDraft({ kind: "answer", entry: b.id, text: "later" })));
		await run(Desk.use((d) => d.send(`answer-${a.id}`)));
		expect(prompts.map((prompt) => prompt.delivery)).toEqual(["steer"]);
		expect(Object.keys((await run(Store.use((s) => s.snapshot))).drafts)).toEqual([`answer-${b.id}`]);
	});

	test("a draft that no longer applies stays a draft and is reported; the rest still go", async () => {
		const { run, failure, prompts } = await setup();
		await run(MainSession.use((main) => main.claim("ses_root")));
		const a = await run(ask("A?"));
		const theirs = await run(Store.use((s) => s.create({ asker: "user", title: "Theirs", question: "" })));
		await run(Desk.use((d) => d.putDraft({ kind: "answer", entry: a.id, text: "fine" })));
		await run(Desk.use((d) => d.putDraft({ kind: "answer", entry: theirs.id, text: "answering myself" })));
		expect(await failure(Desk.use((d) => d.submit))).toContain("the agents answer it");
		expect(prompts).toHaveLength(1);
		expect(Object.keys((await run(Store.use((s) => s.snapshot))).drafts)).toEqual([`answer-${theirs.id}`]);
	});

	test("the user marks their own question done, not the agents'", async () => {
		const { run, failure } = await setup();
		const mine = await run(ask("Agents'"));
		expect(await failure(Desk.use((d) => d.resolveOwn(mine.id)))).toContain("only the asker");
	});
});

describe("page server", () => {
	const base = async () => (await desk!.run(PageUrl.useSync((page) => page.url))).replace(/\/$/, "");

	test("serves the page, the view and live updates; refuses cross-site posts", async () => {
		const { run } = await setup({ page: true });
		await run(MainSession.use((main) => main.claim("ses_root")));
		const url = await base();
		expect(await (await fetch(url)).text()).toContain("<title>Desk</title>");
		expect((await fetch(`${url}/page.js`)).headers.get("content-type")).toContain("javascript");
		expect(await (await fetch(`${url}/api/view`)).json()).toMatchObject({ ok: true, value: { main: { id: "ses_root" }, waiting: 0, entries: [], queue: [] } });

		const events = await fetch(`${url}/api/events`);
		const reader = events.body!.pipeThrough(new TextDecoderStream()).getReader();
		let buffer = "";
		const next = async () => {
			while (!buffer.includes("\n\n")) buffer += (await reader.read()).value;
			const [message, ...rest] = buffer.split("\n\n");
			buffer = rest.join("\n\n");
			return JSON.parse(/data: (.*)/.exec(message!)![1]!);
		};
		expect((await next()).waiting).toBe(0);
		await run(ask("Live?"));
		expect((await next()).waiting).toBe(1);
		await reader.cancel();

		const post = (headers: Record<string, string>) =>
			fetch(`${url}/api/draft`, { method: "POST", headers: { "content-type": "application/json", ...headers }, body: JSON.stringify({ kind: "answer", entry: 1, text: "x" }) });
		expect((await post({})).status).toBe(403);
		expect((await post({ "x-desk": "1", origin: "https://evil.example" })).status).toBe(403);
		expect(await (await post({ "x-desk": "1" })).json()).toEqual({ ok: true, value: null });
		expect((await fetch(`${url}/api/view`, { headers: { host: "evil.example" } })).status).toBe(421);
		const bad = await fetch(`${url}/api/resolve`, { method: "POST", headers: { "content-type": "application/json", "x-desk": "1" }, body: "{}" });
		expect(bad.status).toBe(400);
		expect((await bad.json()).error).toContain("Bad request");
	});

	test("serves files inside the checkout only", async () => {
		await Bun.write(join(root, "media", "a.txt"), "inside");
		await Bun.write(join(root, "..", `outside-${root.split("/").pop()}.txt`), "outside");
		await setup({ page: true });
		const url = await base();
		const inside = await fetch(`${url}/files/media/a.txt`);
		expect(await inside.text()).toBe("inside");
		expect(inside.headers.get("content-type")).toContain("text/plain");
		expect((await fetch(`${url}/files/..%2Foutside-${root.split("/").pop()}.txt`)).status).toBe(404);
		await rm(join(root, "..", `outside-${root.split("/").pop()}.txt`));
	});

	test("a second desk for the same checkout reuses the first one's page", async () => {
		await setup({ page: true });
		const url = await base();
		const scope = Effect.runSync(Scope.make());
		const second = App.Page(Number(new URL(url).port)).pipe(
			Layer.provideMerge(App.Core),
			Layer.provide(fakeSessions().layer),
			Layer.provide(App.Platform(root)),
		);
		const context = await Effect.runPromise(Layer.buildWithScope(second, scope));
		expect(await Effect.runPromise(Effect.provideContext(PageUrl.useSync((page) => page), context))).toEqual({ url: `${url}/`, owned: false });
		await Effect.runPromise(Scope.close(scope, Exit.void));
	});

	test("the page server stops with its scope", async () => {
		await setup({ page: true });
		const url = await base();
		await desk!.close();
		desk = undefined;
		expect(await fetch(url).then(() => "answered", () => "refused")).toBe("refused");
	});
});
