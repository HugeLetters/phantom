/**
 * The desk page on 127.0.0.1: the page (built by Vite into `page/dist/`), its RPC API, and repo files for Markdown media.
 *
 * One guard covers every request: the Host must be a loopback name (a DNS-rebound name gets nothing), and writes
 * must come from the page itself (no Origin, or this origin), so other sites open in the browser can't post into a
 * session.
 *
 * Not named `server.ts`: OpenCode loads a plugin directory's `server.ts` as its entry, ahead of `index.ts`.
 */
import { fileURLToPath } from "node:url";
import * as BunHttpServer from "@effect/platform-bun/BunHttpServer";
import * as Effect from "effect/Effect";
import * as HttpRouter from "effect/http/HttpRouter";
import * as HttpServerRequest from "effect/http/HttpServerRequest";
import * as HttpServerResponse from "effect/http/HttpServerResponse";
import * as HttpStaticServer from "effect/http/HttpStaticServer";
import * as Layer from "effect/Layer";
import * as RpcSerialization from "effect/rpc/RpcSerialization";
import * as RpcServer from "effect/rpc/RpcServer";
import * as Stream from "effect/Stream";
import { DeskApi } from "./api";
import { Desk } from "./desk";

const Guard = HttpRouter.middleware(
	(app) =>
		Effect.gen(function* () {
			const request = yield* HttpServerRequest.HttpServerRequest;
			const host = request.headers.host ?? "";
			const origin = request.headers.origin;
			if (!/^(127\.0\.0\.1|localhost):\d+$/.test(host)) return HttpServerResponse.text("wrong host", { status: 421 });
			if (request.method !== "GET" && origin !== undefined && origin !== `http://${host}`) return HttpServerResponse.text("forbidden", { status: 403 });
			return yield* app;
		}),
	{ global: true },
);

const Api = RpcServer.layerHttp({ group: DeskApi, path: "/rpc", protocol: "http" }).pipe(
	Layer.provide(
		DeskApi.toLayer(
			Effect.gen(function* () {
				const desk = yield* Desk;
				return {
					view: () => Stream.map(desk.changes, (entries) => ({ root: desk.root, entries })),
					submit: ({ drafts }) => desk.deliver(drafts, "queue"),
					send: (draft) => Effect.asVoid(desk.deliver([draft], "steer")),
					resolve: ({ id }) => desk.resolve(id, "user"),
				};
			}),
		),
	),
	Layer.provide(RpcSerialization.layerNdjson),
);

/** Serves the desk for `root` on `port`; a taken port fails the layer. */
export const layer = (root: string, port: number) =>
	HttpRouter.serve(
		Layer.mergeAll(
			Guard,
			Api,
			HttpStaticServer.layer({ root, prefix: "/files" }),
			// The page as built by Vite (`bun install`, or `bun run page` while editing it); unbuilt, it's a 404.
			HttpStaticServer.layer({ root: fileURLToPath(new URL("./page/dist", import.meta.url)) }),
		),
		{ disableLogger: true, disableListenLog: true },
	).pipe(Layer.provideMerge(BunHttpServer.layer({ port, hostname: "127.0.0.1" })));
