/** OpenCode sessions, as far as the desk needs them: the one seam to OpenCode, so demos and tests swap it. */
import type { Plugin } from "@opencode/plugin/effect";
import type { Session } from "@opencode/schema/session";
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import { DeskError } from "./api";

export type Delivery = "queue" | "steer";

export interface Prompt {
	readonly sessionID: string;
	readonly text: string;
	readonly delivery: Delivery;
}

export class Sessions extends Context.Service<
	Sessions,
	{
		/** The top-level session above `sessionID` (itself when it has no parent). */
		readonly root: (sessionID: string) => Effect.Effect<string, DeskError>;
		readonly prompt: (prompt: Prompt) => Effect.Effect<void, DeskError>;
	}
>()("desk/Sessions") {
	static opencode(ctx: Plugin.Context) {
		const failed = (cause: unknown) => new DeskError({ message: String(cause) });
		const get = (sessionID: string) => ctx.session.get({ sessionID: sessionID as Session.ID }).pipe(Effect.mapError(failed));
		return Layer.succeed(this, {
			root: Effect.fn(function* (sessionID) {
				let session = yield* get(sessionID);
				while (session.parentID) session = yield* get(session.parentID);
				return session.id;
			}),
			prompt: ({ sessionID, text, delivery }) =>
				ctx.session.prompt({ sessionID: sessionID as Session.ID, text, delivery }).pipe(Effect.asVoid, Effect.mapError(failed)),
		});
	}

	/** Sessions without OpenCode: every session is top-level (unless `parents` says otherwise); prompts are recorded. */
	static memory(prompts: Array<Prompt>, parents: Record<string, string> = {}) {
		return Layer.succeed(this, {
			root: (sessionID) => Effect.succeed(parents[sessionID] ?? sessionID),
			prompt: (prompt) => Effect.sync(() => void prompts.push(prompt)),
		});
	}
}
