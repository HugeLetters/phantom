/**
 * desk — questions, answers and reviews between the user and the agents.
 *
 * Agents use the desk_* tools; the user uses the page this plugin serves; the TUI footer shows how many entries
 * wait on the user. Entries live in the checkout's gitignored `coordination/` directory. See `app.ts` for the layers.
 *
 * This file imports only what OpenCode provides, so a checkout whose `.opencode/` has no `bun install` yet gets one
 * clear log line instead of a failed plugin.
 */
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { Plugin } from "@opencode/plugin/effect";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import { PluginContext } from "./config";

/** `.opencode/plugins/desk/` → the checkout it belongs to. */
const checkout = resolve(import.meta.dir, "../../..");
const packageRoot = resolve(import.meta.dir, "../..");

export const missingDependencies = `desk: dependencies are not installed. Run \`bun install\` in ${packageRoot}; the desk stays off until then.`;

export default Plugin.define({
	id: "desk",
	effect(ctx) {
		return Effect.gen(function* () {
			const app = yield* Effect.tryPromise(() => import("./app")).pipe(Effect.option);
			if (app._tag === "None") {
				if (!existsSync(resolve(packageRoot, "node_modules"))) return yield* Effect.logError(missingDependencies);
				// Installed, yet the import failed: a real bug; let OpenCode report it.
				return yield* Effect.die(yield* Effect.tryPromise(() => import("./app")).pipe(Effect.flip));
			}
			const App = app.value;
			const mainSession = typeof ctx.options.mainSession === "string" ? ctx.options.mainSession : undefined;
			const Live = App.Plugin.pipe(Layer.provide(Layer.merge(App.Platform(checkout, { mainSession }), Layer.succeed(PluginContext, ctx))));
			// Built in the plugin's scope: unloading the plugin stops the page server, the watcher and the RPC.
			const services = yield* Layer.build(Live);
			yield* Effect.logInfo("desk page", App.pageUrl(services));
		}).pipe(Effect.orDie);
	},
});
