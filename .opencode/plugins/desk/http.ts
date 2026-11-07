/**
 * The desk's web page and its API, served on 127.0.0.1 from inside the plugin.
 *
 * Mutations need a JSON body with the `x-desk` header and a same-origin Host/Origin, so other web pages the browser
 * has open can neither read the desk nor post into the main session.
 */
import { createHash } from "node:crypto";
import { realpath } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";
import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";
import type { Desk, SessionInfo } from "./desk";
import { DeskError, Draft, type Snapshot } from "./store";
import { queue, waitingCount } from "./tree";

export interface View {
	readonly root: string;
	readonly main: SessionInfo | null;
	readonly waiting: number;
	/** Every entry on the desk, resolved ones included (they stay while something below them is open). */
	readonly entries: Snapshot["entries"];
	/** The page's list and the order Next walks: see `queue`. */
	readonly queue: ReadonlyArray<number>;
	readonly drafts: Snapshot["drafts"];
}

export interface Server {
	readonly url: string;
	/** False when another desk for the same checkout already serves `url`. */
	readonly owned: boolean;
	readonly stop: () => void;
}

/** A stable port per checkout, so a bookmarked page keeps working: 47000–47999. */
export function preferredPort(root: string): number {
	return 47000 + (createHash("sha256").update(root).digest().readUInt16BE(0) % 1000);
}

const page = new URL("./page.html", import.meta.url);
const Resolve = Schema.Struct({ id: Schema.Int });
const SendKey = Schema.Struct({ key: Schema.String });
const Main = Schema.Struct({ sessionID: Schema.String });

export const start = Effect.fn("Desk.Server.start")(function* (desk: Desk, root: string, port = preferredPort(root)) {
	const clients = new Set<(view: string) => void>();
	let mainInfo: SessionInfo | null = null;

	const view = Effect.gen(function* () {
		const snapshot = desk.store.snapshot();
		const sessionID = desk.mainSession();
		if (sessionID && mainInfo?.id !== sessionID) {
			mainInfo = yield* desk.sessions.get(sessionID).pipe(Effect.orElseSucceed(() => ({ id: sessionID, title: "(session not found)" })));
		}
		const result: View = {
			root,
			main: sessionID ? mainInfo : null,
			waiting: waitingCount(snapshot.entries, "user"),
			entries: snapshot.entries,
			queue: queue(snapshot.entries),
			drafts: snapshot.drafts,
		};
		return result;
	});

	const broadcast = () =>
		void Effect.runPromise(view).then((value) => {
			const text = JSON.stringify(value);
			for (const send of clients) send(text);
		});
	const unsubscribe = desk.store.subscribe(broadcast);

	const handle = async (request: Request, origin: string): Promise<Response> => {
		const url = new URL(request.url);
		// Loopback names only: a DNS-rebound name pointing here must not reach the desk.
		const port = new URL(origin).port;
		const host = request.headers.get("host");
		if (host !== `127.0.0.1:${port}` && host !== `localhost:${port}`) return new Response("wrong host", { status: 421 });
		const route = `${request.method} ${url.pathname}`;

		if (route === "GET /") return new Response(Bun.file(page), { headers: { "content-type": "text/html; charset=utf-8" } });
		if (route === "GET /api/info") return Response.json({ desk: true, root });
		if (route === "GET /api/view") return Response.json(await Effect.runPromise(view));
		if (route === "GET /api/events") return events(request, clients, await Effect.runPromise(view));
		if (route === "GET /api/sessions") return run(desk.sessions.list());
		if (request.method === "GET" && url.pathname.startsWith("/files/")) return file(root, decodeURIComponent(url.pathname.slice(7)));

		if (request.method !== "POST") return new Response("not found", { status: 404 });
		const requestOrigin = request.headers.get("origin");
		if (request.headers.get("x-desk") !== "1" || (requestOrigin !== null && requestOrigin !== `http://${host}`)) {
			return new Response("forbidden", { status: 403 });
		}
		const body: unknown = await request.json().catch(() => null);
		switch (url.pathname) {
			case "/api/draft":
				return run(decode(Draft, body).pipe(Effect.flatMap(desk.user.putDraft)));
			case "/api/submit":
				return run(desk.user.submit());
			case "/api/send":
				return run(decode(SendKey, body).pipe(Effect.flatMap(({ key }) => desk.user.sendKey(key))));
			case "/api/resolve":
				return run(decode(Resolve, body).pipe(Effect.flatMap(({ id }) => desk.user.resolve(id))));
			case "/api/main":
				return run(decode(Main, body).pipe(Effect.flatMap(({ sessionID }) => desk.user.selectMain(sessionID)), Effect.tap(() => Effect.sync(broadcast))));
		}
		return new Response("not found", { status: 404 });
	};

	const server = yield* bind(port, root, handle);
	if (!server) {
		unsubscribe();
		const url = `http://127.0.0.1:${port}/`;
		return { url, owned: false, stop: () => {} } satisfies Server;
	}
	const url = `http://127.0.0.1:${server.port}/`;
	return {
		url,
		owned: true,
		stop: () => {
			unsubscribe();
			clients.clear();
			server.stop(true);
		},
	} satisfies Server;
});

/**
 * Binds the preferred port, or the next free one. Returns undefined when a desk for the same checkout already
 * listens there (another location of the same checkout in this OpenCode server).
 */
const bind = Effect.fn("Desk.Server.bind")(function* (
	port: number,
	root: string,
	handle: (request: Request, origin: string) => Promise<Response>,
) {
	for (let candidate = port; candidate < port + 20; candidate++) {
		const origin = `http://127.0.0.1:${candidate}`;
		const server = yield* Effect.sync(() => {
			try {
				const server: Bun.Server<undefined> = Bun.serve({
					port: candidate,
					hostname: "127.0.0.1",
					idleTimeout: 0,
					fetch: (request) => handle(request, `http://127.0.0.1:${server.port}`),
				});
				return server;
			} catch {
				return undefined;
			}
		});
		if (server) return server;
		const other = yield* Effect.promise(() =>
			fetch(`${origin}/api/info`, { signal: AbortSignal.timeout(1000) })
				.then((response) => response.json() as Promise<{ root?: string }>)
				.catch(() => ({ root: undefined })),
		);
		if (other.root === root) return undefined;
	}
	return yield* Effect.die(new Error(`No free port for the desk near ${port}`));
});

function events(request: Request, clients: Set<(view: string) => void>, initial: View): Response {
	let send: ((view: string) => void) | undefined;
	let ping: ReturnType<typeof setInterval> | undefined;
	const stream = new ReadableStream<string>({
		start(controller) {
			send = (view) => controller.enqueue(`event: view\ndata: ${view}\n\n`);
			send(JSON.stringify(initial));
			clients.add(send);
			ping = setInterval(() => controller.enqueue(": ping\n\n"), 25_000);
			request.signal.addEventListener("abort", () => {
				if (send) clients.delete(send);
				clearInterval(ping);
				controller.close();
			});
		},
		cancel() {
			if (send) clients.delete(send);
			clearInterval(ping);
		},
	});
	return new Response(stream, { headers: { "content-type": "text/event-stream", "cache-control": "no-cache" } });
}

/** Files under the checkout only, so Markdown can show repo images, GIFs and video by relative path. */
async function file(root: string, path: string): Promise<Response> {
	const target = resolve(root, path.replace(/^\/+/, ""));
	const real = await realpath(target).catch(() => null);
	const realRoot = await realpath(root).catch(() => root);
	const inside = real !== null && !relative(realRoot, real).startsWith("..") && !isAbsolute(relative(realRoot, real));
	if (!inside || !(await Bun.file(real).exists())) return new Response("not found", { status: 404 });
	return new Response(Bun.file(real));
}

function decode<S extends Schema.Top>(schema: S, body: unknown): Effect.Effect<S["Type"], DeskError> {
	return (Schema.decodeUnknownEffect(schema as never)(body) as Effect.Effect<S["Type"], unknown>).pipe(
		Effect.mapError((cause) => new DeskError({ message: `Bad request: ${String(cause)}`, cause })),
	);
}

async function run(effect: Effect.Effect<unknown, DeskError>): Promise<Response> {
	const result = await Effect.runPromise(Effect.result(effect));
	return result._tag === "Success"
		? Response.json({ ok: true, value: result.success ?? null })
		: Response.json({ ok: false, error: result.failure.message }, { status: 400 });
}
