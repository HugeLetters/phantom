/**
 * The plugin inside a real, throwaway OpenCode server: isolated config, data and state directories, a scripted
 * model, and a temporary project laid out like a checkout (`.opencode/package.json`, `bun.lock`, `plugins/desk/`)
 * after `bun install --frozen-lockfile`, so the plugin resolves its dependencies from `.opencode/node_modules`.
 *
 * Skipped when `opencode` is not installed.
 */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { cp, mkdtemp, readdir, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import * as Effect from "effect/Effect";
import * as FetchHttpClient from "effect/http/FetchHttpClient";
import * as Layer from "effect/Layer";
import * as RpcClient from "effect/rpc/RpcClient";
import * as RpcSerialization from "effect/rpc/RpcSerialization";
import { DeskApi } from "../api";
import { type MockLlm, startMockLlm } from "./support/mock-llm";

const opencode = Bun.which("opencode");
const plugin = resolve(import.meta.dir, "..");
const opencodeDir = resolve(plugin, "../..");
const password = "desk-integration";

let temp: string;
let installed: string;
let llm: MockLlm;
let server: Bun.Subprocess<"ignore", "ignore", "pipe"> | undefined;
let logs = "";
let base: string;

async function api<T = any>(method: string, path: string, body?: unknown): Promise<T> {
	const response = await fetch(`${base}${path}`, {
		method,
		headers: { authorization: `Basic ${btoa(`opencode:${password}`)}`, "content-type": "application/json" },
		body: body === undefined ? undefined : JSON.stringify(body),
	});
	const text = await response.text();
	if (!response.ok) throw new Error(`${method} ${path}: ${response.status} ${text}`);
	return (text ? JSON.parse(text) : null) as T;
}

const where = (project: string) => `location%5Bdirectory%5D=${encodeURIComponent(project)}`;

async function until<T>(read: () => Promise<T>, done: (value: T) => boolean, what: string): Promise<T> {
	for (let attempt = 0; attempt < 150; attempt++) {
		const value = await read().catch(() => undefined);
		if (value !== undefined && done(value)) return value;
		await Bun.sleep(150);
	}
	throw new Error(`Timed out waiting for ${what}`);
}

async function messages(sessionID: string) {
	return (await api<{ data: Array<{ type: string; text?: string; content?: Array<any> }> }>("GET", `/api/session/${sessionID}/message`)).data;
}

/** A checkout-shaped project: opencode.json with the scripted model, and `.opencode/` with the desk package. */
async function makeProject(name: string) {
	const project = join(temp, name);
	await Bun.write(
		join(project, "opencode.json"),
		JSON.stringify({
			$schema: "https://opencode.ai/config.json",
			model: "mock/mock",
			providers: { mock: { name: "Mock", package: "@opencode/ai/providers/openai-compatible", settings: { baseURL: llm.url, apiKey: "x" }, models: { mock: {} } } },
		}),
	);
	for (const file of ["package.json", "bun.lock", "bunfig.toml", "tsconfig.json"]) await cp(join(opencodeDir, file), join(project, ".opencode", file));
	await cp(plugin, join(project, ".opencode", "plugins", "desk"), { recursive: true, filter: (source) => !source.includes("/tests") });
	Bun.spawnSync(["git", "init", "-q"], { cwd: project });
	return project;
}

async function session(project: string) {
	return (await api<{ data: { id: string } }>("POST", "/api/session", { title: "desk integration", location: { directory: project } })).data.id;
}

describe.skipIf(!opencode)("desk in a standalone OpenCode server", () => {
	beforeAll(async () => {
		temp = await mkdtemp(join(tmpdir(), "desk-integration-"));
		llm = startMockLlm();
		installed = await makeProject("installed");
		const install = Bun.spawnSync(["bun", "install", "--frozen-lockfile"], { cwd: join(installed, ".opencode"), stdout: "pipe", stderr: "pipe" });
		if (install.exitCode !== 0) throw new Error(`bun install failed: ${install.stderr.toString()}`);

		const port = (() => {
			const probe = Bun.serve({ port: 0, fetch: () => new Response() });
			const free = probe.port;
			probe.stop(true);
			return free;
		})();
		base = `http://127.0.0.1:${port}`;
		const env = {
			...process.env,
			XDG_CONFIG_HOME: join(temp, "config"),
			XDG_DATA_HOME: join(temp, "data"),
			XDG_STATE_HOME: join(temp, "state"),
			XDG_CACHE_HOME: join(temp, "cache"),
			OPENCODE_DB: join(temp, "data", "opencode.db"),
			OPENCODE_PASSWORD: password,
		};
		server = Bun.spawn([opencode!, "serve", "--port", String(port), "--hostname", "127.0.0.1", "--print-logs", "--log-level", "info"], {
			cwd: installed,
			env,
			stdout: "ignore",
			stderr: "pipe",
		});
		void (async () => {
			for await (const chunk of server.stderr.pipeThrough(new TextDecoderStream())) logs += chunk;
		})();
		await until(() => api("GET", "/api/info"), Boolean, "the server");
	}, 120_000);

	afterAll(async () => {
		server?.kill();
		await server?.exited;
		llm?.stop();
		if (temp) await rm(temp, { recursive: true, force: true });
	});

	test(
		"installed: an agent asks, the user answers on the page, the answer is queued into the main session",
		async () => {
			const sessionID = await session(installed);
			const ask = { title: "Ship icon b?", body: "![b](media/b.png)", labels: ["design"] };
			await api("POST", `/api/session/${sessionID}/prompt`, { text: `@call desk_ask ${JSON.stringify(ask)}` });

			const coordination = join(installed, "coordination");
			await until(() => readdir(coordination), (names) => names.includes("0001.md"), "the entry file");
			expect(await readFile(join(coordination, "0001.md"), "utf8")).toContain(`asker: agent\nstatus: open\nsession: ${sessionID}\nlabels: [design]`);

			const status = await until(
				() => api<{ output: { url: string; waiting: number } }>("POST", `/api/rpc/desk/status?${where(installed)}`, { input: {} }),
				(result) => result.output.waiting === 1,
				"the badge count",
			);
			const page = status.output.url.replace(/\/$/, "");
			// The page script is compiled when the plugin loads.
			expect((await (await fetch(`${page}/page.js`)).text()).length).toBeGreaterThan(10_000);
			const client = RpcClient.make(DeskApi);
			const protocol = RpcClient.layerProtocolHttp({ url: `${page}/rpc` }).pipe(Layer.provide([FetchHttpClient.layer, RpcSerialization.layerNdjson]));
			const submitted = Effect.scoped(Effect.flatMap(client, (desk) => desk.submit({ drafts: [{ entry: 1, text: "Yes, b." }] })));
			expect(await Effect.runPromise(submitted.pipe(Effect.provide(protocol)))).toBe(1);

			const delivered = await until(
				() => messages(sessionID),
				(list) => list.some((message) => message.type === "user" && message.text?.startsWith("Desk: the user replied")),
				"the desk message in the main session",
			);
			expect(delivered.find((message) => message.text?.startsWith("Desk:"))?.text).toContain("## #1 Answered: Ship icon b?\n\nYes, b.");
			expect(await readFile(join(coordination, "0001.md"), "utf8")).toContain("status: answered");

			await api("POST", `/api/session/${sessionID}/prompt`, { text: `@call desk_resolve {"id":1}` });
			await until(() => readdir(coordination), (names) => !names.some((name) => name.endsWith(".md")), "pruning");
		},
		90_000,
	);
});
