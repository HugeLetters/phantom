/**
 * The page and its API.
 *
 * Every request must name the server by a loopback host (a DNS-rebound name pointing here gets nothing), and
 * mutations need the `x-desk` header and a same-origin `Origin`, so other pages open in the browser can neither read
 * the desk nor post into the main session.
 */
import { isAbsolute, relative } from "node:path";
import * as Data from "effect/Data";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Path from "effect/Path";
import * as Schedule from "effect/Schedule";
import * as Schema from "effect/Schema";
import * as Stream from "effect/Stream";
import * as HttpRouter from "effect/http/HttpRouter";
import * as HttpServerRequest from "effect/http/HttpServerRequest";
import * as HttpServerResponse from "effect/http/HttpServerResponse";
import { DeskConfig, DeskError } from "../config";
import { Desk } from "../desk";
import { MainSession } from "../main-session";
import { type SessionInfo, Sessions } from "../sessions";
import { Draft, type Snapshot, Store } from "../store";
import { queue, waitingCount } from "../tree";

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

class Forbidden extends Data.TaggedError("Forbidden")<{ readonly status: number; readonly reason: string }> {}

const page = new URL("../page/index.html", import.meta.url).pathname;
const script = new URL("../page/dist/page.js", import.meta.url).pathname;

/** Loopback names only. */
const checkHost = Effect.gen(function* () {
	const request = yield* HttpServerRequest.HttpServerRequest;
	const host = request.headers.host ?? "";
	if (!/^(127\.0\.0\.1|localhost):\d+$/.test(host)) return yield* new Forbidden({ status: 421, reason: "wrong host" });
	return host;
});

/** Same-origin writes from the page only. */
const checkWrite = Effect.gen(function* () {
	const host = yield* checkHost;
	const request = yield* HttpServerRequest.HttpServerRequest;
	const origin = request.headers.origin;
	if (request.headers["x-desk"] !== "1" || (origin !== undefined && origin !== `http://${host}`)) {
		return yield* new Forbidden({ status: 403, reason: "forbidden" });
	}
});

const refuse = (error: Forbidden) => Effect.succeed(HttpServerResponse.text(error.reason, { status: error.status }));

/** `{ ok, value }`, or `{ ok: false, error }` with 400: the page shows the error as is. */
const reply = <A, R>(effect: Effect.Effect<A, DeskError | Forbidden, R>) =>
	effect.pipe(
		Effect.flatMap((value) => HttpServerResponse.json({ ok: true, value: value ?? null })),
		Effect.catchTags({
			DeskError: (error) => HttpServerResponse.json({ ok: false, error: error.message }, { status: 400 }),
			Forbidden: refuse,
		}),
		Effect.orDie,
	);

/** A JSON API read: loopback host, then `effect`. */
const read = <A, R>(effect: Effect.Effect<A, DeskError, R>) => reply(Effect.andThen(checkHost, effect));
/** A JSON API write: same-origin page request, then `effect` on the decoded body. */
const write = <A, B, R>(schema: Schema.Codec<A, any>, effect: (input: A) => Effect.Effect<B, DeskError, R>) =>
	reply(
		Effect.gen(function* () {
			yield* checkWrite;
			const input = yield* HttpServerRequest.schemaBodyJson(schema).pipe(
				Effect.mapError((cause) => new DeskError({ message: `Bad request: ${String(cause)}`, cause })),
			);
			return yield* effect(input);
		}),
	);

const view = Effect.gen(function* () {
	const { root } = yield* DeskConfig;
	const store = yield* Store;
	const sessions = yield* Sessions;
	const sessionID = yield* (yield* MainSession).current;
	const snapshot = yield* store.snapshot;
	const main = sessionID
		? yield* sessions.get(sessionID).pipe(Effect.orElseSucceed(() => ({ id: sessionID, title: "(session not found)" })))
		: null;
	return {
		root,
		main,
		waiting: waitingCount(snapshot.entries, "user"),
		entries: snapshot.entries,
		queue: queue(snapshot.entries),
		drafts: snapshot.drafts,
	} satisfies View;
});

const file = (path: string, contentType: string) =>
	Effect.gen(function* () {
		const fs = yield* FileSystem.FileSystem;
		const data = yield* fs.readFile(path);
		return HttpServerResponse.uint8Array(data, { contentType });
	}).pipe(Effect.orElseSucceed(() => HttpServerResponse.text("not found", { status: 404 })));

/** A file under the checkout, for Markdown's images, GIFs and video; nothing outside it. */
const repoFile = Effect.gen(function* () {
	yield* checkHost;
	const { root } = yield* DeskConfig;
	const fs = yield* FileSystem.FileSystem;
	const path = yield* Path.Path;
	const request = yield* HttpServerRequest.HttpServerRequest;
	const wanted = decodeURIComponent(new URL(request.url, "http://desk").pathname.slice("/files/".length));
	const realRoot = yield* fs.realPath(root).pipe(Effect.orElseSucceed(() => root));
	const target = yield* fs.realPath(path.resolve(root, wanted)).pipe(Effect.option);
	const inside = Option.filter(target, (real) => {
		const offset = relative(realRoot, real);
		return !offset.startsWith("..") && !isAbsolute(offset);
	});
	if (Option.isNone(inside)) return HttpServerResponse.text("not found", { status: 404 });
	return yield* file(inside.value, Bun.file(inside.value).type);
}).pipe(Effect.catchTag("Forbidden", refuse));

/** The view now and after every change, as server-sent events, with a keep-alive comment. */
const events = Effect.gen(function* () {
	yield* checkHost;
	const store = yield* Store;
	const context = yield* Effect.context<Effect.Services<typeof view>>();
	const views = store.changes.pipe(
		Stream.mapEffect(() => Effect.provideContext(view, context)),
		Stream.map((value) => `event: view\ndata: ${JSON.stringify(value)}\n\n`),
	);
	const pings = Stream.fromSchedule(Schedule.spaced("25 seconds")).pipe(Stream.map(() => ": ping\n\n"));
	return HttpServerResponse.stream(Stream.encodeText(Stream.merge(views, pings)), {
		contentType: "text/event-stream",
		headers: { "cache-control": "no-cache" },
	});
}).pipe(Effect.catchTag("Forbidden", refuse));

const SendKey = Schema.Struct({ key: Schema.String });
const ResolveId = Schema.Struct({ id: Schema.Int });
const MainId = Schema.Struct({ sessionID: Schema.String });

export const layer = Layer.mergeAll(
	HttpRouter.add("GET", "/", Effect.andThen(checkHost, file(page, "text/html; charset=utf-8")).pipe(Effect.catchTag("Forbidden", refuse))),
	HttpRouter.add("GET", "/page.js", file(script, "text/javascript; charset=utf-8")),
	HttpRouter.add("GET", "/files/*", repoFile),
	HttpRouter.add("GET", "/api/events", events),
	HttpRouter.add("GET", "/api/info", read(DeskConfig.useSync(({ root }) => ({ desk: true, root })))),
	HttpRouter.add("GET", "/api/view", read(view)),
	HttpRouter.add("GET", "/api/sessions", read(Sessions.use((sessions) => sessions.list))),
	HttpRouter.add("POST", "/api/draft", write(Draft, (draft) => Desk.use((desk) => desk.putDraft(draft)))),
	HttpRouter.add("POST", "/api/submit", write(Schema.Unknown, () => Desk.use((desk) => desk.submit))),
	HttpRouter.add("POST", "/api/send", write(SendKey, ({ key }) => Desk.use((desk) => desk.send(key)))),
	HttpRouter.add("POST", "/api/resolve", write(ResolveId, ({ id }) => Desk.use((desk) => desk.resolveOwn(id)))),
	HttpRouter.add("POST", "/api/main", write(MainId, ({ sessionID }) => MainSession.use((main) => main.select(sessionID)))),
);
