/**
 * The page's script, compiled when the plugin loads: Solid (babel-preset-solid) bundled with `marked`, `DOMPurify`
 * and `highlight.js` by `Bun.build`, kept in memory.
 */
import { transformAsync } from "@babel/core";
// @ts-expect-error: the preset ships no types
import typescript from "@babel/preset-typescript";
// @ts-expect-error: the preset ships no types
import solid from "babel-preset-solid";
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import { DeskError } from "../config";

const entry = new URL("./main.tsx", import.meta.url).pathname;

const solidPlugin: Bun.BunPlugin = {
	name: "solid",
	setup(build) {
		build.onLoad({ filter: /\/page\/.*\.tsx$/ }, async ({ path }) => {
			const result = await transformAsync(await Bun.file(path).text(), {
				filename: path,
				babelrc: false,
				configFile: false,
				presets: [[solid, { generate: "dom" }], [typescript, { onlyRemoveTypeImports: true }]],
			});
			return { contents: result?.code ?? "", loader: "js" };
		});
	},
};

export class PageBundle extends Context.Service<PageBundle, { readonly script: string }>()("desk/PageBundle") {
	static readonly layer = Layer.effect(
		this,
		Effect.tryPromise({
			try: async () => {
				const result = await Bun.build({
					entrypoints: [entry],
					target: "browser",
					format: "esm",
					minify: true,
					// Solid's production build: no dev-only warnings or checks.
					conditions: ["browser", "production"],
					define: { "process.env.NODE_ENV": JSON.stringify("production") },
					plugins: [solidPlugin],
				});
				if (!result.success) throw new AggregateError(result.logs, "page build failed");
				return { script: await result.outputs[0]!.text() };
			},
			catch: (cause) => new DeskError({ message: `Could not build the desk page: ${String(cause)}`, cause }),
		}).pipe(Effect.withSpan("Desk.PageBundle.build")),
	);
}
