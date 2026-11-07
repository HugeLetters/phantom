import type { Plugin } from "@opencode/plugin/effect";
import * as Context from "effect/Context";
import * as Data from "effect/Data";
import * as Layer from "effect/Layer";
import { resolve } from "node:path";

/** Every failure the desk reports to agents and the page: a sentence a person can act on. */
export class DeskError extends Data.TaggedError("DeskError")<{ readonly message: string; readonly cause?: unknown }> {}

/** The OpenCode plugin context, for the layers that talk to OpenCode. */
export class PluginContext extends Context.Service<PluginContext, Plugin.Context>()("desk/PluginContext") {}

export interface DeskConfigShape {
	/** The checkout the desk belongs to; the page serves files under it. */
	readonly root: string;
	/** Where entries live: `<root>/coordination`. */
	readonly directory: string;
	/** A main session fixed by configuration, instead of the first desk tool caller. */
	readonly mainSession: string | undefined;
}

export class DeskConfig extends Context.Service<DeskConfig, DeskConfigShape>()("desk/DeskConfig") {
	static layer(root: string, options: { readonly mainSession?: string } = {}) {
		return Layer.succeed(this, { root, directory: resolve(root, "coordination"), mainSession: options.mainSession });
	}
}
