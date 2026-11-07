/**
 * The plugin inside a real, throwaway OpenCode server: isolated config, data and state directories, a scripted
 * model, and a temporary project. Skipped when `opencode` is not installed.
 */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdtemp, readdir, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { type MockLlm, startMockLlm } from "./support/mock-llm";

const opencode = Bun.which("opencode");
const plugin = resolve(import.meta.dir, "..");
const password = "desk-integration";

let temp: string;
let project: string;
let llm: MockLlm;
let server: Bun.Subprocess | undefined;
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

const where = () => `location%5Bdirectory%5D=${encodeURIComponent(project)}`;

async function until<T>(read: () => Promise<T>, done: (value: T) => boolean, what: string): Promise<T> {
	for (let attempt = 0; attempt < 100; attempt++) {
		const value = await read().catch(() => undefined);
		if (value !== undefined && done(value)) return value;
		await Bun.sleep(150);
	}
	throw new Error(`Timed out waiting for ${what}`);
}

async function messages(sessionID: string) {
	return (await api<{ data: Array<{ type: string; text?: string; content?: Array<any> }> }>("GET", `/api/session/${sessionID}/message`)).data;
}

describe.skipIf(!opencode)("desk in a standalone OpenCode server", () => {
	beforeAll(async () => {
		temp = await mkdtemp(join(tmpdir(), "desk-integration-"));
		project = join(temp, "project");
		llm = startMockLlm();
		await Bun.write(
			join(project, "opencode.json"),
			JSON.stringify({
				$schema: "https://opencode.ai/config.json",
				model: "mock/mock",
				providers: {
					mock: { name: "Mock", package: "@opencode/ai/providers/openai-compatible", settings: { baseURL: llm.url, apiKey: "x" }, models: { mock: {} } },
				},
				plugins: [{ package: plugin, options: { directory: project } }],
			}),
		);
		Bun.spawnSync(["git", "init", "-q"], { cwd: project });
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
		server = Bun.spawn([opencode!, "serve", "--port", String(port), "--hostname", "127.0.0.1"], { cwd: project, env, stdout: "ignore", stderr: "ignore" });
		await until(() => api("GET", "/api/info"), Boolean, "the server");
	}, 60_000);

	afterAll(async () => {
		server?.kill();
		await server?.exited;
		llm?.stop();
		if (temp) await rm(temp, { recursive: true, force: true });
	});

	test(
		"an agent asks, the user answers on the page, the answer is queued into the main session",
		async () => {
			const session = await api<{ data: { id: string } }>("POST", "/api/session", { title: "desk integration", location: { directory: project } });
			const sessionID = session.data.id;
			const ask = { title: "Ship icon b?", body: "![b](media/b.png)", labels: ["design"] };
			await api("POST", `/api/session/${sessionID}/prompt`, { text: `@call desk_ask ${JSON.stringify(ask)}` });

			const files = await until(() => readdir(join(project, "coordination")), (names) => names.includes("0001-ship-icon-b.md"), "the entry file");
			expect(files).toContain("0001-ship-icon-b.md");
			expect(await readFile(join(project, "coordination", "0001-ship-icon-b.md"), "utf8")).toContain("asker: main\nanswerer: user\nstatus: open\nlabels: [design]");

			const status = await until(
				() => api<{ output: { url: string; waiting: number } }>("POST", `/api/rpc/desk/status?${where()}`, { input: {} }),
				(result) => result.output.waiting === 1,
				"the badge count",
			);
			const page = status.output.url.replace(/\/$/, "");
			const view = await (await fetch(`${page}/api/view`)).json();
			expect(view).toMatchObject({ main: { id: sessionID }, waiting: 1 });

			const post = (path: string, body: unknown) =>
				fetch(`${page}${path}`, { method: "POST", headers: { "content-type": "application/json", "x-desk": "1" }, body: JSON.stringify(body) }).then((r) => r.json());
			expect(await post("/api/draft", { kind: "answer", entry: 1, text: "Yes, b." })).toEqual({ ok: true, value: null });
			expect(await post("/api/submit", {})).toEqual({ ok: true, value: 1 });

			const delivered = await until(
				() => messages(sessionID),
				(list) => list.some((message) => message.type === "user" && message.text?.startsWith("Desk: the user replied")),
				"the desk message in the main session",
			);
			expect(delivered.find((message) => message.text?.startsWith("Desk:"))?.text).toContain("## #1 Answered: Ship icon b?\n\nYes, b.");
			expect(await readFile(join(project, "coordination", "0001-ship-icon-b.md"), "utf8")).toContain("status: answered");

			await api("POST", `/api/session/${sessionID}/prompt`, { text: `@call desk_resolve {"id":1}` });
			await until(() => readdir(join(project, "coordination")), (names) => !names.some((name) => name.endsWith(".md")), "pruning");
		},
		60_000,
	);
});
