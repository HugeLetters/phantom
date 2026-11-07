/**
 * The desk page on 127.0.0.1: the page itself, its RPC API, repo files for Markdown media, and Mermaid.
 *
 * One guard covers every request: the Host must be a loopback name (a DNS-rebound name gets nothing), and writes
 * must come from the page itself (no Origin, or this origin), so other sites open in the browser can't post into a
 * session.
 *
 * Not named `server.ts`: OpenCode loads a plugin directory's `server.ts` as its entry, ahead of `index.ts`.
 */
import { transformAsync } from "@babel/core";
import * as BunHttpServer from "@effect/platform-bun/BunHttpServer";
// @ts-expect-error: the preset ships no types
import typescript from "@babel/preset-typescript";
// @ts-expect-error: the preset ships no types
import solid from "babel-preset-solid";
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

const shell = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><link rel="icon" href="data:,"><link rel="stylesheet" href="/style.css"></head><body><script type="module" src="/page.js"></script></body></html>`;
const page = (file: string) => new URL(`./page/${file}`, import.meta.url).pathname;

/** The page's script: Solid compiled by babel-preset-solid, bundled by Bun when the plugin loads. */
const bundle = Effect.promise(async () => {
	const result = await Bun.build({
		entrypoints: [page("main.tsx")],
		target: "browser",
		minify: true,
		// "solid": Solid libraries (the router, meta) ship JSX sources there, compiled below like the page itself.
		conditions: ["solid", "browser", "production"],
		define: { "process.env.NODE_ENV": JSON.stringify("production") },
		plugins: [
			{
				name: "solid",
				setup(build) {
					build.onLoad({ filter: /\.[jt]sx$/ }, async ({ path }) => {
						const presets = [[solid, { generate: "dom" }], [typescript, { onlyRemoveTypeImports: true }]];
						const result = await transformAsync(await Bun.file(path).text(), { filename: path, babelrc: false, configFile: false, presets });
						return { contents: result?.code ?? "", loader: "js" };
					});
				},
			},
		],
	});
	if (!result.success) throw new AggregateError(result.logs, "desk page build failed");
	return result.outputs[0]!.text();
});

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

const Page = HttpRouter.use((router) =>
	Effect.gen(function* () {
		const script = yield* bundle;
		yield* router.add("GET", "/", HttpServerResponse.html(shell));
		yield* router.add("GET", "/page.js", HttpServerResponse.text(script, { contentType: "text/javascript" }));
		yield* router.add("GET", "/style.css", HttpServerResponse.file(page("style.css")));
	}),
);

/** Serves the desk for `root` on `port`; a taken port fails the layer. */
export const layer = (root: string, port: number) =>
	HttpRouter.serve(
		Layer.mergeAll(
			Guard,
			Api,
			Page,
			HttpStaticServer.layer({ root, prefix: "/files" }),
			HttpStaticServer.layer({ root: new URL("../../node_modules/mermaid/dist", import.meta.url).pathname, prefix: "/mermaid" }),
		),
		{ disableLogger: true, disableListenLog: true },
	).pipe(Layer.provideMerge(BunHttpServer.layer({ port, hostname: "127.0.0.1" })));
