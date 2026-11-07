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
const ask = (title: string, extra: { parent?: number; session?: string } = {}) => desk((d) => d.ask(extra.session ?? "ses_a", { title, body: "", parent: extra.parent }));

beforeEach(async () => {
	root = await mkdtemp(join(tmpdir(), "desk-"));
	await open();
});
afterEach(async () => {
	await close();
	await rm(root, { recursive: true, force: true });
});

describe("entries", () => {
	test("one file per entry, named by id; loaded again on restart; other files ignored", async () => {
		await ask("First");
		await ask("Second", { parent: 1 });
		await writeFile(join(root, "coordination", "QUESTIONS.md"), "# notes");
		expect(await files()).toEqual(["0001.md", "0002.md", "QUESTIONS.md"]);
		await close();
		await open();
		expect((await entries()).map((entry) => [entry.id, entry.title, entry.parent])).toEqual([
			[1, "First", null],
			[2, "Second", 1],
		]);
	});

	test("an entry belongs to the top-level session that asked", async () => {
		expect((await ask("From a subagent", { session: "ses_sub" })).session).toBe("ses_a");
	});

	test("a follow-up needs an existing parent", async () => {
		expect(await failure((d) => d.ask("ses_a", { title: "x", body: "", parent: 5 }))).toContain("No desk entry #5");
	});

	test("only the answerer answers and only the asker resolves", async () => {
		await ask("Agents'");
		expect(await failure((d) => d.answer(1, "x"))).toContain("the user answers it");
		expect(await failure((d) => d.resolve(1, "user"))).toContain("only the asker");
	});

	test("resolving prunes bottom-up; an unresolved follow-up holds its ancestors", async () => {
		await ask("Root");
		await ask("Child", { parent: 1 });
		await ask("Grandchild", { parent: 2 });
		await desk((d) => d.resolve(1, "agent"));
		await desk((d) => d.resolve(3, "agent"));
		expect(await files()).toEqual(["0001.md", "0002.md"]);
		await desk((d) => d.resolve(2, "agent"));
		expect(await files()).toEqual([]);
	});

	test("describe: unresolved entries flat, or one thread", async () => {
		await desk((d) => d.ask("ses_a", { title: "Root", body: "Details" }));
		await desk((d) => d.ask("ses_a", { title: "Child", body: "More", parent: 1 }));
		expect(await desk((d) => d.describe())).toBe(
			"- #1 Root — open, waiting on the user, asked by the agent\n- #2 Child (follow-up to #1) — open, waiting on the user, asked by the agent",
		);
		const thread = await desk((d) => d.describe(2));
		expect(thread.indexOf("# #1 Root")).toBeLessThan(thread.indexOf("# #2 Child"));
	});
});

describe("the user's drafts", () => {
	test("Submit answers, with one queued message per session", async () => {
		await ask("A?");
		await ask("B?", { session: "ses_b" });
		expect(await desk((d) => d.deliver([{ entry: 1, text: "yes" }, { entry: 2, text: "no" }], "queue"))).toBe(2);
		expect(prompts.map((prompt) => [prompt.sessionID, prompt.delivery])).toEqual([
			["ses_a", "queue"],
			["ses_b", "queue"],
		]);
		expect(prompts[0]!.text).toContain("## #1 Answered: A?\n\nyes");
		expect((await entries()).map((entry) => [entry.status, entry.answer])).toEqual([
			["answered", "yes"],
			["answered", "no"],
		]);
	});

	test("a reply on an entry not waiting for an answer is a follow-up, in the same session", async () => {
		await ask("Agents' question", { session: "ses_b" });
		await desk((d) => d.deliver([{ entry: 1, text: "Answer." }], "queue"));
		await desk((d) => d.deliver([{ entry: 1, text: "And a question back?\n\nDetails." }], "queue"));
		expect((await entries())[1]).toMatchObject({ id: 2, parent: 1, asker: "user", status: "open", session: "ses_b", title: "And a question back?" });
		expect(prompts[1]!.text).toContain("## #2 New question from the user (follow-up to #1)");
		await desk((d) => d.answer(2, "Here."));
		await desk((d) => d.resolve(2, "user"));
		expect(await files()).toEqual(["0001.md"]);
	});

	test("Submit is all or nothing", async () => {
		await ask("A?");
		const error = await failure((d) => d.deliver([{ entry: 1, text: "fine" }, { entry: 9, text: "gone" }], "queue"));
		expect(error).toContain("No desk entry #9");
		expect(prompts).toEqual([]);
		expect((await entries())[0]!.status).toBe("open");
	});

	test("Send steers one draft to its entry's session", async () => {
		await ask("A?", { session: "ses_b" });
		await desk((d) => d.deliver([{ entry: 1, text: "now" }], "steer"));
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

	test("serves the page shell, its compiled script, styles and Mermaid", async () => {
		expect(await (await fetch(url())).text()).toContain('<script type="module" src="/page.js">');
		expect((await (await fetch(`${url()}/page.js`)).text()).length).toBeGreaterThan(10_000);
		expect((await fetch(`${url()}/style.css`)).headers.get("content-type")).toContain("css");
		expect((await fetch(`${url()}/mermaid/mermaid.esm.min.mjs`)).status).toBe(200);
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
		await ask("Live?");
		const views = rpc((client) => client.view().pipe(Stream.take(2), Stream.runCollect));
		await Bun.sleep(200);
		await ask("Another?");
		expect(Array.from(await views).map((view) => view.entries.length)).toEqual([1, 2]);
		expect(await rpc((client) => client.submit({ drafts: [{ entry: 1, text: "yes" }] }))).toBe(1);
		expect(prompts).toHaveLength(1);
		const refused = await rpc((client) => Effect.flip(client.resolve({ id: 1 })));
		expect(refused.message).toContain("only the asker");
	});
});
