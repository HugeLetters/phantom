/**
 * The page server: `@effect/platform-bun`'s HTTP server on 127.0.0.1, on the checkout's port or the next free one.
 */
import { createHash } from "node:crypto";
import * as BunHttpServer from "@effect/platform-bun/BunHttpServer";
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as HttpClient from "effect/http/HttpClient";
import * as FetchHttpClient from "effect/http/FetchHttpClient";
import * as HttpRouter from "effect/http/HttpRouter";
import * as HttpServer from "effect/http/HttpServer";
import * as Layer from "effect/Layer";
import { DeskConfig } from "../config";

/** Where the desk page is, and whether this plugin instance serves it. */
export class PageUrl extends Context.Service<PageUrl, { readonly url: string; readonly owned: boolean }>()("desk/PageUrl") {}

/** A stable port per checkout, so a bookmarked page keeps working: 47000–47999. */
export function preferredPort(root: string): number {
	return 47000 + (createHash("sha256").update(root).digest().readUInt16BE(0) % 1000);
}

/** Which checkout the desk on `port` serves, if a desk is there at all. */
const servedRoot = (port: number) =>
	HttpClient.get(`http://127.0.0.1:${port}/api/info`).pipe(
		Effect.flatMap((response) => response.json),
		Effect.map((body) => (body as { readonly value?: { readonly root?: string } }).value?.root),
		Effect.timeout("1 second"),
		Effect.orElseSucceed(() => undefined),
		Effect.provide(FetchHttpClient.layer),
	);

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
				// Bun throws when the port is taken; the server otherwise lives as long as this layer.
				const server = yield* BunHttpServer.make({ port: candidate, hostname: "127.0.0.1" }).pipe(Effect.option, Effect.catchDefect(() => Effect.succeedNone));
				if (server._tag === "Some") {
					const address = server.value.address;
					const url = `http://127.0.0.1:${address._tag === "UnixPathAddress" ? candidate : address.port}/`;
					return HttpRouter.serve(routes, { disableLogger: true, disableListenLog: true }).pipe(
						Layer.provide(Layer.succeed(HttpServer.HttpServer, server.value)),
						Layer.provide(BunHttpServer.layerHttpServices),
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
