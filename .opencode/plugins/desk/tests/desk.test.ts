import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
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
import { pruneAttachments } from "../prune-attachments";
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

	test("an entry can't be resolved while anything below it is unresolved; resolved leaves leave with their parents", async () => {
		const root_ = await ask("Root");
		const child = await ask("Child", { parent: root_.id });
		const grandchild = await ask("Grandchild", { parent: child.id });
		const sibling = await ask("Sibling", { parent: root_.id });
		const error = await failure((d) => d.resolve(root_.id, "agent"));
		for (const id of [child.id, grandchild.id, sibling.id]) expect(error).toContain(id);
		expect(error).toContain("3 open below");
		expect(await failure((d) => d.resolve(child.id, "agent"))).toContain(grandchild.id);
		expect((await entries()).map((entry) => entry.status)).toEqual(["open", "open", "open", "open"]);
		await desk((d) => d.resolve(grandchild.id, "agent"));
		await desk((d) => d.resolve(child.id, "agent"));
		expect(await files()).toEqual([file(root_), file(sibling)].sort());
		expect(await failure((d) => d.resolve(root_.id, "agent"))).toContain(sibling.id);
		await desk((d) => d.resolve(sibling.id, "agent"));
		await desk((d) => d.resolve(root_.id, "agent"));
		expect(await files()).toEqual([]);
	});

	test("pruning walks up the parent chain", async () => {
		const root_ = await ask("Root");
		const child = await ask("Child", { parent: root_.id });
		const grandchild = await ask("Grandchild", { parent: child.id });
		await desk((d) => d.resolve(grandchild.id, "agent"));
		expect(await files()).toEqual([file(root_), file(child)].sort());
		await desk((d) => d.resolve(child.id, "agent"));
		expect(await files()).toEqual([file(root_)]);
		await desk((d) => d.resolve(root_.id, "agent"));
		expect(await files()).toEqual([]);
	});

	test("describe: unresolved entries flat with full ids, or one thread", async () => {
		const first = await desk((d) => d.ask("ses_a", { title: "Root", body: "Details" }));
		await Bun.sleep(2);
		const second = await desk((d) => d.ask("ses_a", { title: "Child", body: "More", parent: first.id }));
		// Follow-ups before the entries they hang under.
		expect(await desk((d) => d.describe())).toBe(
			`- ${second.id} Child (follow-up to ${first.id}) — open, waiting on the user, asked by the agent\n- ${first.id} Root — open, waiting on the user, asked by the agent`,
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

	test("desk_ask files a PR's question under its review root; adopt files a top-level entry once, never into its own subtree", async () => {
		const labelled = (title: string, labels: Array<string>, parent?: string) => desk((d) => d.ask("ses_a", { title, body: "", labels, parent }));
		const review = await labelled("Review PR #7", ["pr:7", "review"]);
		expect(review.parent).toBeNull();
		const other = await labelled("Review PR #8", ["pr:8", "review"]);
		const q = await labelled("Q", ["pr:7"]);
		expect(q.parent).toBe(review.id);
		expect((await labelled("Elsewhere", ["pr:7"], other.id)).parent).toBe(other.id);
		expect((await labelled("Plain", ["design"])).parent).toBeNull();
		expect((await labelled("No review", ["pr:9"])).parent).toBeNull();
		expect((await labelled("Not a root", ["pr:7", "review"], q.id)).parent).toBe(q.id);
		expect((await labelled("Deeper", ["pr:7"])).parent).toBe(review.id);
		const old = await labelled("Review PR #7 duplicate", ["pr:7", "review"]);
		expect(old.parent).toBeNull(); // a review entry is a root itself, never filed under another
		expect(await failure((d) => d.resolve(review.id, "agent"))).toContain(q.id);
		await desk((d) => d.resolve(old.id, "agent"));
		const nested = (await entries()).find((entry) => entry.title === "Not a root")!;
		for (const id of [nested.id, q.id, ...(await entries()).filter((entry) => entry.parent === review.id && entry.id !== q.id).map((entry) => entry.id)]) await desk((d) => d.resolve(id, "agent"));
		await desk((d) => d.resolve(review.id, "agent"));
		expect((await labelled("After", ["pr:7"])).parent).toBeNull();
		const again = await labelled("Review PR #7 again", ["pr:7", "review"]);
		expect(again.parent).toBeNull();
		expect((await labelled("Again's", ["pr:7"])).parent).toBe(again.id);

		const loose = await ask("Loose");
		const adopted = await desk((d) => d.adopt(loose.id, other.id));
		expect(adopted.parent).toBe(other.id);
		expect(adopted.priority).toBe("normal");
		expect(await readFile(join(root, "coordination", file(loose)), "utf8")).toContain(`parent: ${other.id}`);
		expect(await failure((d) => d.adopt(loose.id, again.id))).toContain("already has a parent");
		const top = await ask("Top");
		const child = await ask("Child", { parent: top.id });
		const grand = await ask("Grand", { parent: child.id });
		for (const inside of [top.id, child.id, grand.id]) expect(await failure((d) => d.adopt(top.id, inside))).toContain("cycle");
		expect(await failure((d) => d.adopt(top.id, "gone"))).toContain("No desk entry gone");
		expect(await failure((d) => d.adopt("gone", top.id))).toContain("No desk entry gone");
		expect((await entries()).find((e) => e.id === top.id)!.parent).toBeNull();
	});

	test("follow-ups inherit their parent's priority unless given one; prioritize can take a whole subtree", async () => {
		const top = await desk((d) => d.ask("ses_a", { title: "Review", body: "", priority: "high" }));
		const child = await desk((d) => d.ask("ses_a", { title: "Child", body: "", parent: top.id }));
		const own = await desk((d) => d.ask("ses_a", { title: "Own", body: "", parent: top.id, priority: "low" }));
		const grand = await desk((d) => d.ask("ses_a", { title: "Grand", body: "", parent: child.id }));
		expect([child, own, grand].map((e) => e.priority)).toEqual(["high", "low", "high"]);
		const other = await ask("Other");
		expect(other.priority).toBe("normal");
		await desk((d) => d.deliver([{ entry: other.id, text: "Answer." }], "queue"));
		await desk((d) => d.deliver([{ entry: other.id, text: "A question back?" }], "queue"));
		await desk((d) => d.prioritize(top.id, "low", false));
		expect((await entries()).map((e) => [e.title, e.priority])).toEqual([["Review", "low"], ["Child", "high"], ["Own", "low"], ["Grand", "high"], ["Other", "normal"], ["A question back?", "normal"]]);
		await desk((d) => d.prioritize(top.id, "high", true));
		expect((await entries()).slice(0, 4).map((e) => e.priority)).toEqual(["high", "high", "high", "high"]);
		expect(await failure((d) => d.prioritize("gone", "high", false))).toContain("No desk entry gone");
		expect(await readFile(join(root, "coordination", file(own)), "utf8")).toContain("priority: high");
		await close();
		await open();
		expect((await entries()).find((e) => e.title === "Own")!.priority).toBe("high");
		const listing = await desk((d) => d.describe());
		expect(listing.split("\n")[0]).toContain("high priority");
		expect(listing.indexOf("Other")).toBeGreaterThan(listing.indexOf("Grand"));
	});

	describe("attachments", () => {
		const shot = { id: crypto.randomUUID(), name: "shot.PNG", data: Buffer.from("png-bytes").toString("base64") };
		const notes = { id: crypto.randomUUID(), name: "notes.txt", data: Buffer.from("notes").toString("base64") };
		const stored = () => readdir(join(root, "coordination", "attachments")).catch(() => []);

		test("sending stores exactly the referenced ones, points the text at them, passes them to the session, and they're pruned with the entry", async () => {
			const entry = await ask("Look at this", { session: "ses_b" });
			const text = `See ![shot.PNG](attachment:${shot.id}) and ![x](coordination/attachments/../../etc/passwd)`;
			await desk((d) => d.deliver([{ entry: entry.id, text, files: [shot, notes] }], "queue"));
			const names = await stored();
			expect(names).toHaveLength(1);
			const path = `coordination/attachments/${names[0]}`;
			expect(path).toMatch(/^coordination\/attachments\/[0-9a-f-]{36}\.png$/);
			expect(await readFile(join(root, path), "utf8")).toBe("png-bytes");
			expect((await entries())[0]!.answer).toBe(`See ![shot.PNG](${path}) and ![x](coordination/attachments/../../etc/passwd)`);
			expect(prompts[0]!.files).toEqual([{ uri: pathToFileURL(join(root, path)).href, name: "shot.PNG" }]);
			expect(prompts[0]!.text).toContain(`![shot.PNG](${path})`);
			await desk((d) => d.resolve(entry.id, "agent"));
			expect(await stored()).toEqual([]);
		});

		test("one whose link was removed from the text is not stored", async () => {
			const entry = await ask("Look at this");
			await desk((d) => d.deliver([{ entry: entry.id, text: "Never mind the screenshot.", files: [shot] }], "queue"));
			expect(await stored()).toEqual([]);
			expect(prompts[0]!.files).toEqual([]);
		});

		test("a failed send stores nothing", async () => {
			const entry = await ask("Look at this");
			const text = `![shot.PNG](attachment:${shot.id})`;
			// The second draft is refused, so the first one's attachment never reaches the disk either.
			expect(await failure((d) => d.deliver([{ entry: entry.id, text, files: [shot] }, { entry: "gone", text: "x" }], "queue"))).toContain("No desk entry gone");
			// A reference whose file the page couldn't send is refused too.
			expect(await failure((d) => d.deliver([{ entry: entry.id, text }], "queue"))).toContain(`attachment ${shot.id} is missing`);
			expect(await stored()).toEqual([]);
			expect(prompts).toEqual([]);
			expect((await entries())[0]!.status).toBe("open");
		});

		test("the one-off cleanup deletes the attachments no entry references", async () => {
			const entry = await ask("Look at this");
			await desk((d) => d.deliver([{ entry: entry.id, text: `![shot.PNG](attachment:${shot.id})`, files: [shot] }], "queue"));
			const [kept] = await stored();
			const orphan = `${crypto.randomUUID()}.png`;
			await writeFile(join(root, "coordination", "attachments", orphan), "left over");
			expect(await pruneAttachments(root)).toEqual([`coordination/attachments/${orphan}`]);
			expect(await stored()).toEqual([kept]);
		});
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
