/**
 * desk — questions, answers and reviews between the user and the agents.
 *
 * Agents use the desk_* tools; the user uses the page this plugin serves; the TUI footer shows how many entries
 * wait on the user. Entries live in the checkout's gitignored `coordination/` directory. See `app.ts` for the layers.
 *
 * This file imports only what OpenCode provides, so a checkout whose `.opencode/` has no `bun install` yet gets one
 * clear log line instead of a failed plugin.
 */
import { resolve } from "node:path";
import { Plugin } from "@opencode/plugin/effect";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import { PluginContext } from "./config";
import { missingPackages } from "./install-check";

/** `.opencode/plugins/desk/` → the checkout it belongs to. */
const checkout = resolve(import.meta.dir, "../../..");
const packageRoot = resolve(import.meta.dir, "../..");

export const missingDependencies = `desk: dependencies are not installed. Run \`bun install\` in ${packageRoot}, then restart OpenCode's service (\`opencode service restart\`); the desk stays off until then.`;
/** Bun caches module resolution per process: an install made while OpenCode runs is only seen after a restart. */
const importFailed = `desk: could not load its dependencies from ${packageRoot}/node_modules. If \`bun install\` ran while OpenCode was running, restart OpenCode's service (\`opencode service restart\`).`;

export default Plugin.define({
	id: "desk",
	effect(ctx) {
		return Effect.gen(function* () {
			// Check before importing, so a missing install is one clear line rather than a resolve error.
			if (missingPackages(packageRoot).length > 0) return yield* Effect.logError(missingDependencies);
			const imported = yield* Effect.tryPromise(() => import("./app")).pipe(Effect.result);
			if (imported._tag === "Failure") return yield* Effect.logError(importFailed, String(imported.failure.cause));
			const App = imported.success;
			const mainSession = typeof ctx.options.mainSession === "string" ? ctx.options.mainSession : undefined;
			const Live = App.Plugin.pipe(Layer.provide(Layer.merge(App.Platform(checkout, { mainSession }), Layer.succeed(PluginContext, ctx))));
			// Built in the plugin's scope: unloading the plugin stops the page server, the watcher and the RPC.
			const services = yield* Layer.build(Live);
			yield* Effect.logInfo("desk page", App.pageUrl(services));
		}).pipe(Effect.orDie);
	},
});
