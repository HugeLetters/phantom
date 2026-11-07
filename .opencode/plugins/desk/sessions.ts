/** OpenCode sessions, as far as the desk needs them. Tests provide their own `Sessions`. */
import type { Session } from "@opencode/schema/session";
import type { AbsolutePath } from "@opencode/schema/schema";
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import { DeskConfig, DeskError, PluginContext } from "./config";

export type Delivery = "queue" | "steer";

export interface SessionInfo {
	readonly id: string;
	readonly title: string;
}

export class Sessions extends Context.Service<
	Sessions,
	{
		/** The top-level session above `sessionID` (itself when it has no parent). */
		readonly root: (sessionID: string) => Effect.Effect<string, DeskError>;
		readonly get: (sessionID: string) => Effect.Effect<SessionInfo, DeskError>;
		/** Recent top-level sessions in this checkout, newest first. */
		readonly list: Effect.Effect<ReadonlyArray<SessionInfo>, DeskError>;
		readonly prompt: (sessionID: string, text: string, delivery: Delivery) => Effect.Effect<void, DeskError>;
	}
>()("desk/Sessions") {
	static readonly layer = Layer.effect(
		this,
		Effect.gen(function* () {
			const ctx = yield* PluginContext;
			const { root } = yield* DeskConfig;
			const failed = (what: string) => (cause: unknown) => new DeskError({ message: `Could not ${what}: ${String(cause)}`, cause });
			const get = (sessionID: string) =>
				ctx.session.get({ sessionID: sessionID as Session.ID }).pipe(Effect.mapError(failed(`find session ${sessionID}`)));
			return Sessions.of({
				root: Effect.fn("Desk.Sessions.root")(function* (sessionID) {
					let session = yield* get(sessionID);
					for (let depth = 0; session.parentID && depth < 32; depth++) session = yield* get(session.parentID);
					return session.id;
				}),
				get: (sessionID) => get(sessionID).pipe(Effect.map((session) => ({ id: session.id, title: session.title ?? session.id }))),
				list: ctx.session.list({ directory: root as AbsolutePath, parentID: null, limit: 30, order: "desc" }).pipe(
					Effect.map((page) => page.data.map((session) => ({ id: session.id, title: session.title ?? session.id }))),
					Effect.mapError(failed("list sessions")),
				),
				prompt: (sessionID, text, delivery) =>
					ctx.session.prompt({ sessionID: sessionID as Session.ID, text, delivery }).pipe(
						Effect.asVoid,
						Effect.mapError(failed("message the main session")),
					),
			});
		}),
	);
}
