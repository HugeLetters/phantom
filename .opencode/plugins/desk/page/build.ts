/**
 * Bundles the page (Solid, compiled with babel-preset-solid) into `dist/page.js`.
 *
 * The bundle is committed: OpenCode loads the plugin without `bun install`, so the plugin can't compile Solid at
 * runtime. `tests/page.test.ts` fails when the committed bundle is out of date. Run: `bun plugins/desk/page/build.ts`.
 */
import { transformAsync } from "@babel/core";
// @ts-expect-error: no types
import solid from "babel-preset-solid";
// @ts-expect-error: no types
import typescript from "@babel/preset-typescript";

const entry = new URL("./main.tsx", import.meta.url).pathname;
const output = new URL("./dist/page.js", import.meta.url).pathname;

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

/** The bundle's text, built fresh. */
export async function bundle(): Promise<string> {
	const result = await Bun.build({ entrypoints: [entry], target: "browser", format: "esm", minify: true, plugins: [solidPlugin] });
	if (!result.success) throw new AggregateError(result.logs, "page build failed");
	return result.outputs[0]!.text();
}

if (import.meta.main) {
	await Bun.write(output, await bundle());
	console.log(`wrote ${output}`);
}
