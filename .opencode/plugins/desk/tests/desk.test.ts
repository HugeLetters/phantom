import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as FetchHttpClient from "effect/http/FetchHttpClient";
import * as HttpServer from "effect/http/HttpServer";
import * as Layer from "effect/Layer";
import * as RpcClient from "effect/rpc/RpcClient";
import * as RpcSerialization from "effect/rpc/RpcSerialization";
import * as Scope from "effect/Scope";
import * as Stream from "effect/Stream";
import { DeskApi } from "../api";
import * as App from "../app";
import { Desk } from "../desk";
import { type Prompt, Sessions } from "../sessions";
import { expectBuiltPage } from "./support/page";

let root: string;
let scope: Scope.Closeable;
let services: Context.Context<Desk | HttpServer.HttpServer>;
let prompts: Array<Prompt>;

async function open() {
	scope = Effect.runSync(Scope.make());
	prompts = [];
	const layer = App.desk(root, 0).pipe(Layer.provide(Sessions.memory(prompts, { ses_sub: "ses_a" })));
	services = (await Effect.runPromise(Layer.buildWithScope(layer, scope))) as typeof services;
}
const close = () => Effect.runPromise(Scope.close(scope, Exit.void));
const desk = <A, E>(f: (desk: Desk["Service"]) => Effect.Effect<A, E>) => Effect.runPromise(Effect.provideContext(Desk.use(f), services));
const failure = async <A, E>(f: (desk: Desk["Service"]) => Effect.Effect<A, E>) => {
	const exit = await Effect.runPromiseExit(Effect.provideContext(Desk.use(f), services));
	return Exit.isFailure(exit) ? String(exit.cause) : "succeeded";
};
const entries = () => desk((d) => Effect.map(Stream.runHead(d.changes), (head) => (head._tag === "Some" ? head.value : [])));
const files = async () => (await readdir(join(root, "coordination"))).sort();
const ask = (title: string, extra: { parent?: string; session?: string } = {}) => desk((d) => d.ask(extra.session ?? "ses_a", { title, body: "", parent: extra.parent }));
const file = (entry: { id: string }) => `${entry.id}.md`;

beforeEach(async () => {
	root = await mkdtemp(join(tmpdir(), "desk-"));
	await open();
});
afterEach(async () => {
	await close();
	await rm(root, { recursive: true, force: true });
});

describe("entries", () => {
	test("one file per entry, named by its id; loaded again on restart, by `created`; other files ignored", async () => {
		const first = await ask("First");
		await Bun.sleep(2);
		const second = await ask("Second", { parent: first.id });
		expect(first.id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$/);
		await writeFile(join(root, "coordination", "QUESTIONS.md"), "# notes");
		expect(await files()).toEqual([file(first), file(second), "QUESTIONS.md"].sort());
		await close();
		await open();
		expect((await entries()).map((entry) => [entry.id, entry.title, entry.parent])).toEqual([
			[first.id, "First", null],
			[second.id, "Second", first.id],
		]);
	});

	test("ids are never reused after pruning", async () => {
		const first = await ask("First");
		await desk((d) => d.resolve(first.id, "agent"));
		const second = await ask("Second");
		expect(second.id).not.toBe(first.id);
	});

	test("an entry belongs to the top-level session that asked", async () => {
		expect((await ask("From a subagent", { session: "ses_sub" })).session).toBe("ses_a");
	});

	test("a follow-up needs an existing parent", async () => {
		expect(await failure((d) => d.ask("ses_a", { title: "x", body: "", parent: "missing" }))).toContain("No desk entry missing");
	});

	test("only the answerer answers and only the asker resolves", async () => {
		const entry = await ask("Agents'");
		expect(await failure((d) => d.answer(entry.id, "x"))).toContain("the user answers it");
		expect(await failure((d) => d.resolve(entry.id, "user"))).toContain("only the asker");
	});

	test("a resolved entry leaves once it has no follow-ups; an unresolved follow-up holds its ancestors", async () => {
		const root_ = await ask("Root");
		const child = await ask("Child", { parent: root_.id });
		const grandchild = await ask("Grandchild", { parent: child.id });
		const sibling = await ask("Sibling", { parent: root_.id });
		await desk((d) => d.resolve(root_.id, "agent"));
		await desk((d) => d.resolve(grandchild.id, "agent"));
		expect(await files()).toEqual([file(root_), file(child), file(sibling)].sort());
		await desk((d) => d.resolve(sibling.id, "agent"));
		expect(await files()).toEqual([file(root_), file(child)].sort());
	});

	test("pruning walks up the parent chain", async () => {
		const root_ = await ask("Root");
		const child = await ask("Child", { parent: root_.id });
		const grandchild = await ask("Grandchild", { parent: child.id });
		await desk((d) => d.resolve(root_.id, "agent"));
		await desk((d) => d.resolve(child.id, "agent"));
		expect(await files()).toEqual([file(root_), file(child), file(grandchild)].sort());
		await desk((d) => d.resolve(grandchild.id, "agent"));
		expect(await files()).toEqual([]);
	});

	test("describe: unresolved entries flat with full ids, or one thread", async () => {
		const first = await desk((d) => d.ask("ses_a", { title: "Root", body: "Details" }));
		await Bun.sleep(2);
		const second = await desk((d) => d.ask("ses_a", { title: "Child", body: "More", parent: first.id }));
		expect(await desk((d) => d.describe())).toBe(
			`- ${first.id} Root — open, waiting on the user, asked by the agent\n- ${second.id} Child (follow-up to ${first.id}) — open, waiting on the user, asked by the agent`,
		);
		const thread = await desk((d) => d.describe(second.id));
		expect(thread.indexOf(`# ${first.id} Root`)).toBeLessThan(thread.indexOf(`# ${second.id} Child`));
	});
});

describe("the user's drafts", () => {
	test("Submit answers, with one queued message per session", async () => {
		const a = await ask("A?");
		await Bun.sleep(2);
		const b = await ask("B?", { session: "ses_b" });
		expect(await desk((d) => d.deliver([{ entry: a.id, text: "yes" }, { entry: b.id, text: "no" }], "queue"))).toBe(2);
		expect(prompts.map((prompt) => [prompt.sessionID, prompt.delivery])).toEqual([
			["ses_a", "queue"],
			["ses_b", "queue"],
		]);
		expect(prompts[0]!.text).toContain(`## ${a.id} Answered: A?\n\nyes`);
		expect((await entries()).map((entry) => [entry.status, entry.answer])).toEqual([
			["answered", "yes"],
			["answered", "no"],
		]);
	});

	test("a reply on an entry not waiting for an answer is a follow-up, in the same session", async () => {
		const entry = await ask("Agents' question", { session: "ses_b" });
		await desk((d) => d.deliver([{ entry: entry.id, text: "Answer." }], "queue"));
		await desk((d) => d.deliver([{ entry: entry.id, text: "And a question back?\n\nDetails." }], "queue"));
		const followUp = (await entries())[1]!;
		expect(followUp).toMatchObject({ parent: entry.id, asker: "user", status: "open", session: "ses_b", title: "And a question back?" });
		expect(prompts[1]!.text).toContain(`## ${followUp.id} New question from the user (follow-up to ${entry.id})`);
		await desk((d) => d.answer(followUp.id, "Here."));
		await desk((d) => d.resolve(followUp.id, "user"));
		expect(await files()).toEqual([file(entry)]);
	});

	test("entries are ordered by `created`, not by id, also after a restart", async () => {
		for (const title of ["one", "two", "three"]) {
			await ask(title);
			await Bun.sleep(2);
		}
		await close();
		await open();
		expect((await entries()).map((entry) => entry.title)).toEqual(["one", "two", "three"]);
	});

	test("Submit is all or nothing", async () => {
		const entry = await ask("A?");
		const error = await failure((d) => d.deliver([{ entry: entry.id, text: "fine" }, { entry: "gone", text: "x" }], "queue"));
		expect(error).toContain("No desk entry gone");
		expect(prompts).toEqual([]);
		expect((await entries())[0]!.status).toBe("open");
	});

	test("Send steers one draft to its entry's session", async () => {
		const entry = await ask("A?", { session: "ses_b" });
		await desk((d) => d.deliver([{ entry: entry.id, text: "now" }], "steer"));
		expect(prompts.map((prompt) => [prompt.sessionID, prompt.delivery])).toEqual([["ses_b", "steer"]]);
	});
});

describe("the page server", () => {
	const url = () => {
		const address = Context.get(services, HttpServer.HttpServer).address;
		return `http://127.0.0.1:${address._tag === "UnixPathAddress" ? 0 : address.port}`;
	};
	const client = RpcClient.make(DeskApi);
	const rpc = <A, E>(f: (desk: Effect.Success<typeof client>) => Effect.Effect<A, E>) =>
		Effect.runPromise(
			Effect.scoped(Effect.flatMap(client, f)).pipe(
				Effect.provide(RpcClient.layerProtocolHttp({ url: `${url()}/rpc` }).pipe(Layer.provide([FetchHttpClient.layer, RpcSerialization.layerNdjson]))),
			),
		);

	test("serves the page as built by Vite", async () => {
		await expectBuiltPage(url());
	});

	test("serves repo files, nothing outside the checkout", async () => {
		await Bun.write(join(root, "media", "a.txt"), "inside");
		expect(await (await fetch(`${url()}/files/media/a.txt`)).text()).toBe("inside");
		expect((await fetch(`${url()}/files/..%2F..%2Fetc%2Fhosts`)).status).toBe(404);
	});

	test("refuses foreign hosts and cross-site writes", async () => {
		expect((await fetch(url(), { headers: { host: "evil.example" } })).status).toBe(421);
		expect((await fetch(`${url()}/rpc`, { method: "POST", headers: { origin: "https://evil.example" }, body: "{}" })).status).toBe(403);
	});

	test("the typed API: a live view, submit and resolve", async () => {
		const live = await ask("Live?");
		const views = rpc((client) => client.view().pipe(Stream.take(2), Stream.runCollect));
		await Bun.sleep(200);
		await ask("Another?");
		expect(Array.from(await views).map((view) => view.entries.length)).toEqual([1, 2]);
		expect(await rpc((client) => client.submit({ drafts: [{ entry: live.id, text: "yes" }] }))).toBe(1);
		expect(prompts).toHaveLength(1);
		const refused = await rpc((client) => Effect.flip(client.resolve({ id: live.id })));
		expect(refused.message).toContain("only the asker");
	});
});
