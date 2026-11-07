import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as Scope from "effect/Scope";
import * as Desk from "../desk";
import * as Http from "../http";
import { DeskError, make } from "../store";

/** Sessions as a parent map: ses_child → ses_root. */
function fakeSessions(parents: Record<string, string> = {}) {
	const prompts: Array<{ sessionID: string; text: string; delivery: Desk.Delivery }> = [];
	const known = new Set(["ses_root", "ses_other", ...Object.keys(parents)]);
	const sessions: Desk.Sessions = {
		root: (sessionID) => Effect.succeed(parents[sessionID] ?? sessionID),
		get: (sessionID) =>
			known.has(sessionID) ? Effect.succeed({ id: sessionID, title: `Title of ${sessionID}` }) : Effect.fail(new DeskError({ message: `no ${sessionID}` })),
		list: () => Effect.succeed([{ id: "ses_root", title: "Root" }, { id: "ses_other", title: "Other" }]),
		prompt: (sessionID, text, delivery) => Effect.sync(() => void prompts.push({ sessionID, text, delivery })),
	};
	return { sessions, prompts };
}

let directory: string;
let scope: Scope.Closeable;
const run = <A, E>(effect: Effect.Effect<A, E>) => Effect.runPromise(effect);
const fail = async <A>(effect: Effect.Effect<A, DeskError>) => {
	const exit = await Effect.runPromiseExit(effect);
	return Exit.isFailure(exit) ? String(exit.cause) : "succeeded";
};

async function setup(parents?: Record<string, string>, options?: Desk.Options) {
	const store = await run(Scope.provide(scope)(make(directory)));
	const { sessions, prompts } = fakeSessions(parents);
	return { desk: Desk.make(store, sessions, options), prompts, store };
}

beforeEach(async () => {
	directory = await mkdtemp(join(tmpdir(), "desk-"));
	scope = Effect.runSync(Scope.make());
});
afterEach(async () => {
	await run(Scope.close(scope, Exit.void));
	await rm(directory, { recursive: true, force: true });
});

describe("main session", () => {
	test("is the top-level session of the first desk tool caller, and sticks", async () => {
		const { desk } = await setup({ ses_sub: "ses_root" });
		expect(await run(desk.claim("ses_sub"))).toBe("ses_root");
		expect(await run(desk.claim("ses_other"))).toBe("ses_root");
	});

	test("a configured main session wins", async () => {
		const { desk } = await setup({}, { mainSession: "ses_other" });
		expect(await run(desk.claim("ses_root"))).toBe("ses_other");
	});

	test("the user can pick another one on the page", async () => {
		const { desk } = await setup({ ses_sub: "ses_other" });
		await run(desk.claim("ses_root"));
		await run(desk.user.selectMain("ses_sub"));
		expect(desk.mainSession()).toBe("ses_other");
	});

	test("without one, sending explains how it gets known", async () => {
		const { desk } = await setup();
		await run(desk.user.putDraft({ kind: "ask", parent: null, title: "Q", text: "" }));
		expect(await fail(desk.user.submit())).toContain("No main session yet");
	});
});

describe("agent side", () => {
	test("ask, then the user answers, then the agents resolve: pruned", async () => {
		const { desk, store, prompts } = await setup();
		await run(desk.claim("ses_root"));
		const entry = await run(desk.ask({ title: "Which icon?", body: "a or b", labels: ["design"] }));
		expect(entry).toMatchObject({ asker: "main", answerer: "user", status: "open" });
		await run(desk.user.putDraft({ kind: "answer", entry: entry.id, text: "b" }));
		await run(desk.user.submit());
		expect(store.snapshot().entries[0]).toMatchObject({ status: "answered", answer: "b" });
		expect(prompts).toHaveLength(1);
		await run(desk.resolve(entry.id));
		expect(store.snapshot().entries).toEqual([]);
	});

	test("agents can't answer their own question or resolve the user's", async () => {
		const { desk } = await setup();
		const mine = await run(desk.ask({ title: "Mine", body: "" }));
		expect(await fail(desk.answer(mine.id, "x"))).toContain("the user answers it");
		const theirs = await run(desk.store.create({ asker: "user", title: "Theirs", question: "" }));
		expect(await fail(desk.resolve(theirs.id))).toContain("only the asker marks it done");
		expect(await run(desk.answer(theirs.id, "here"))).toMatchObject({ status: "answered", answer: "here" });
	});

	test("describe outlines unresolved entries, or one thread in full", async () => {
		const { desk } = await setup();
		const root = await run(desk.ask({ title: "Root question", body: "Details" }));
		await run(desk.ask({ title: "Follow-up", body: "More", parent: root.id, labels: ["review"] }));
		expect(await run(desk.describe())).toBe(
			"- #1 Root question — open, waiting on user (asked by agents)\n  - #2 Follow-up — open, waiting on user (asked by agents) [review]\n\nWaiting on the user: 2. Waiting on the agents: 0.",
		);
		const thread = await run(desk.describe(2));
		expect(thread).toContain("# Root question");
		expect(thread).toContain("# Follow-up");
		expect(thread.indexOf("Root question")).toBeLessThan(thread.indexOf("Follow-up"));
	});
});

describe("user side", () => {
	test("Submit queues one message with every draft; new questions become entries", async () => {
		const { desk, store, prompts } = await setup();
		await run(desk.claim("ses_root"));
		const a = await run(desk.ask({ title: "A?", body: "" }));
		const b = await run(desk.ask({ title: "B?", body: "" }));
		await run(desk.user.putDraft({ kind: "answer", entry: a.id, text: "yes" }));
		await run(desk.user.putDraft({ kind: "answer", entry: b.id, text: "no" }));
		await run(desk.user.putDraft({ kind: "ask", parent: a.id, title: "Why A?", text: "Explain." }));
		expect(await run(desk.user.submit())).toBe(3);
		expect(prompts).toHaveLength(1);
		expect(prompts[0]).toMatchObject({ sessionID: "ses_root", delivery: "queue" });
		expect(prompts[0]!.text).toContain("## #1 Answered: A?\n\nyes");
		expect(prompts[0]!.text).toContain("## #2 Answered: B?\n\nno");
		expect(prompts[0]!.text).toContain("## #3 New question from the user: Why A? (follow-up to #1)\n\nExplain.");
		expect(store.snapshot().drafts).toEqual({});
		expect(store.snapshot().entries.find((entry) => entry.id === 3)).toMatchObject({ asker: "user", answerer: "main", parent: 1 });
	});

	test("Send delivers one draft with steer and leaves the others", async () => {
		const { desk, store, prompts } = await setup();
		await run(desk.claim("ses_root"));
		const a = await run(desk.ask({ title: "A?", body: "" }));
		const b = await run(desk.ask({ title: "B?", body: "" }));
		await run(desk.user.putDraft({ kind: "answer", entry: a.id, text: "urgent" }));
		await run(desk.user.putDraft({ kind: "answer", entry: b.id, text: "later" }));
		await run(desk.user.sendKey(`answer-${a.id}`));
		expect(prompts.map((prompt) => prompt.delivery)).toEqual(["steer"]);
		expect(Object.keys(store.snapshot().drafts)).toEqual([`answer-${b.id}`]);
	});

	test("a draft that no longer applies stays a draft and is reported; the rest still go", async () => {
		const { desk, store, prompts } = await setup();
		await run(desk.claim("ses_root"));
		const a = await run(desk.ask({ title: "A?", body: "" }));
		const theirs = await run(desk.store.create({ asker: "user", title: "Theirs", question: "" }));
		await run(desk.user.putDraft({ kind: "answer", entry: a.id, text: "fine" }));
		await run(desk.user.putDraft({ kind: "answer", entry: theirs.id, text: "answering myself" }));
		expect(await fail(desk.user.submit())).toContain("the agents answer it");
		expect(prompts).toHaveLength(1);
		expect(Object.keys(store.snapshot().drafts)).toEqual([`answer-${theirs.id}`]);
	});

	test("the user marks their own question done, not the agents'", async () => {
		const { desk } = await setup();
		const mine = await run(desk.ask({ title: "Agents'", body: "" }));
		expect(await fail(desk.user.resolve(mine.id))).toContain("only the asker");
	});
});

describe("page server", () => {
	test("serves the page, the view and live updates; refuses cross-site posts", async () => {
		const { desk } = await setup();
		await run(desk.claim("ses_root"));
		const server = await run(Http.start(desk, directory, 0));
		try {
			const base = server.url.replace(/\/$/, "");
			expect(await (await fetch(base)).text()).toContain("<title>Desk</title>");
			expect(await (await fetch(`${base}/api/view`)).json()).toMatchObject({ main: { id: "ses_root" }, waiting: 0, entries: [], queue: [] });

			const events = await fetch(`${base}/api/events`);
			const reader = events.body!.pipeThrough(new TextDecoderStream()).getReader();
			const next = async () => {
				let text = "";
				while (!text.includes("\n\n")) text += (await reader.read()).value;
				return JSON.parse(/data: (.*)/.exec(text)![1]!);
			};
			expect((await next()).waiting).toBe(0);
			await run(desk.ask({ title: "Live?", body: "" }));
			expect((await next()).waiting).toBe(1);
			await reader.cancel();

			const post = (headers: Record<string, string>) =>
				fetch(`${base}/api/draft`, { method: "POST", headers: { "content-type": "application/json", ...headers }, body: JSON.stringify({ kind: "answer", entry: 1, text: "x" }) });
			expect((await post({})).status).toBe(403);
			expect((await post({ "x-desk": "1", origin: "https://evil.example" })).status).toBe(403);
			expect((await post({ "x-desk": "1" })).status).toBe(200);
			expect((await fetch(`${base}/api/view`, { headers: { host: "evil.example" } })).status).toBe(421);
		} finally {
			server.stop();
		}
	});

	test("serves files inside the checkout only", async () => {
		const { desk } = await setup();
		await Bun.write(join(directory, "media", "a.txt"), "inside");
		const server = await run(Http.start(desk, join(directory, "media"), 0));
		try {
			const base = server.url.replace(/\/$/, "");
			expect(await (await fetch(`${base}/files/a.txt`)).text()).toBe("inside");
			expect((await fetch(`${base}/files/..%2F.desk%2Fstate.json`)).status).toBe(404);
		} finally {
			server.stop();
		}
	});

	test("a second desk for the same checkout reuses the first one's page", async () => {
		const { desk } = await setup();
		const first = await run(Http.start(desk, directory, 0));
		try {
			const port = Number(new URL(first.url).port);
			const second = await run(Http.start(desk, directory, port));
			expect(second).toMatchObject({ url: first.url, owned: false });
		} finally {
			first.stop();
		}
	});
});
