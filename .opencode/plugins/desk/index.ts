/**
 * desk — questions, answers and reviews between the user and the agents.
 *
 * Agents use the desk_* tools; the user uses the page this plugin serves; the TUI footer shows how many entries
 * wait on the user. Entries live in the checkout's gitignored `coordination/` directory. Needs `bun install` in
 * `.opencode/`.
 */
import { resolve } from "node:path";
import { Plugin } from "@opencode/plugin/effect";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as App from "./app";

export default Plugin.define({
	id: "desk",
	// Built in the plugin's scope: unloading the plugin stops the page server and the RPC.
	effect: (ctx) => Layer.build(App.layer(ctx, resolve(import.meta.dir, "../../.."))).pipe(Effect.asVoid, Effect.orDie),
});
