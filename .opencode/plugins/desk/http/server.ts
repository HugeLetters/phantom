/**
 * Effect's `HttpServer` on `Bun.serve`, bound to 127.0.0.1 on the checkout's port.
 *
 * `@effect/platform-bun` has the full adapter, but OpenCode doesn't provide it to plugins without an install; this is
 * the part the desk needs. Requests run through `HttpEffect.toWebHandlerWith`, and the server stops with the layer.
 */
import { createHash } from "node:crypto";
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as HttpEffect from "effect/http/HttpEffect";
import * as HttpRouter from "effect/http/HttpRouter";
import * as HttpServer from "effect/http/HttpServer";
import * as NetAddress from "effect/net/NetAddress";
import * as Result from "effect/Result";
import type * as Scope from "effect/Scope";
import type { HttpServerRequest } from "effect/http/HttpServerRequest";
import { DeskConfig } from "../config";

/** Where the desk page is, and whether this plugin instance serves it. */
export class PageUrl extends Context.Service<PageUrl, { readonly url: string; readonly owned: boolean }>()("desk/PageUrl") {}

/** A stable port per checkout, so a bookmarked page keeps working: 47000–47999. */
export function preferredPort(root: string): number {
	return 47000 + (createHash("sha256").update(root).digest().readUInt16BE(0) % 1000);
}

const notFound = () => new Response("not found", { status: 404 });

/** Listens on the port, or reports it taken. */
const listen = (port: number) =>
	Effect.acquireRelease(
		Effect.sync(() => {
			try {
				return Bun.serve({ port, hostname: "127.0.0.1", idleTimeout: 0, fetch: notFound });
			} catch {
				return undefined;
			}
		}),
		(server) => Effect.promise(async () => server?.stop(true)),
	);

/** Asks whoever holds `port` which checkout they serve. */
const servedRoot = (port: number) =>
	Effect.tryPromise(() =>
		fetch(`http://127.0.0.1:${port}/api/info`, { signal: AbortSignal.timeout(1000) }).then(
			(response) => response.json() as Promise<{ readonly value?: { readonly root?: string } }>,
		),
	).pipe(
		Effect.map((info) => info.value?.root),
		Effect.orElseSucceed(() => undefined),
	);

function make(server: Bun.Server<undefined>): HttpServer.HttpServer["Service"] {
	return HttpServer.make({
		address: Result.getOrThrow(NetAddress.inetAddressFromIpString(server.hostname ?? "127.0.0.1", server.port ?? 0)),
		serve: (app, middleware) =>
			Effect.gen(function* () {
				const handler = HttpEffect.toWebHandlerWith<never, HttpServerRequest | Scope.Scope>(yield* Effect.context<never>())(app, middleware);
				server.reload({ fetch: (request) => handler(request) });
				yield* Effect.addFinalizer(() => Effect.sync(() => server.reload({ fetch: notFound })));
			}),
	});
}

/**
 * Serves `routes` on the preferred port or the next free one. When a desk for the same checkout already listens
 * there (another location of this checkout in the same OpenCode server), serves nothing and points at that page.
 */
export function layer<E, R>(routes: Layer.Layer<never, E, R | HttpRouter.HttpRouter>, port?: number) {
	return Layer.unwrap(
		Effect.gen(function* () {
			const { root } = yield* DeskConfig;
			const first = port ?? preferredPort(root);
			for (let candidate = first; candidate < first + 20; candidate++) {
				const server = yield* listen(candidate);
				if (server) {
					const url = `http://127.0.0.1:${server.port}/`;
					return HttpRouter.serve(routes, { disableLogger: true, disableListenLog: true }).pipe(
						Layer.provide(Layer.succeed(HttpServer.HttpServer, make(server))),
						Layer.merge(Layer.succeed(PageUrl, { url, owned: true })),
					);
				}
				if ((yield* servedRoot(candidate)) === root) {
					return Layer.succeed(PageUrl, { url: `http://127.0.0.1:${candidate}/`, owned: false });
				}
			}
			return yield* Effect.die(new Error(`No free port for the desk near ${first}`));
		}),
	);
}
