/**
 * desk — questions, answers and reviews between the user and the agents.
 *
 * Agents use the desk_* tools; the user uses the page this plugin serves; the TUI footer shows how many entries
 * wait on the user. Entries live in the checkout's gitignored `coordination/` directory. See `app.ts` for the layers.
 */
import { resolve } from "node:path";
import { Plugin } from "@opencode/plugin/effect";
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as App from "./app";
import { PluginContext } from "./config";
import { PageUrl } from "./http/server";

/** `.opencode/plugins/desk/` → the checkout it belongs to. */
const checkout = resolve(import.meta.dir, "../../..");

export default Plugin.define({
	id: "desk",
	effect(ctx) {
		const mainSession = typeof ctx.options.mainSession === "string" ? ctx.options.mainSession : undefined;
		const Live = App.Plugin.pipe(Layer.provide(Layer.merge(App.Platform(checkout, { mainSession }), Layer.succeed(PluginContext, ctx))));
		// Built in the plugin's scope: unloading the plugin stops the page server, the watcher and the RPC.
		return Layer.build(Live).pipe(
			Effect.tap((services) => Effect.logInfo("desk page", Context.get(services, PageUrl))),
			Effect.asVoid,
			Effect.orDie,
		);
	},
});
