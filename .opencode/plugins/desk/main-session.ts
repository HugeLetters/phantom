/**
 * The main session: where the user's answers are delivered. Configured, or else the top-level session above the
 * first desk tool call; the user can switch it on the page. Kept in the store, so it survives restarts.
 */
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import { DeskConfig, DeskError } from "./config";
import { Sessions } from "./sessions";
import { Store } from "./store";

export class MainSession extends Context.Service<
	MainSession,
	{
		readonly current: Effect.Effect<string | null>;
		/** Fails with a how-to when there is none yet. */
		readonly require: Effect.Effect<string, DeskError>;
		/** Every desk tool call goes through here: the first caller's top-level session becomes the main session. */
		readonly claim: (sessionID: string) => Effect.Effect<string, DeskError>;
		/** The user picks one on the page; a child session counts as its top-level session. */
		readonly select: (sessionID: string) => Effect.Effect<void, DeskError>;
	}
>()("desk/MainSession") {
	static readonly layer = Layer.effect(
		this,
		Effect.gen(function* () {
			const config = yield* DeskConfig;
			const store = yield* Store;
			const sessions = yield* Sessions;

			const current = Effect.map(store.snapshot, (snapshot) => config.mainSession ?? snapshot.mainSession);

			return MainSession.of({
				current,
				require: Effect.flatMap(current, (sessionID) =>
					sessionID
						? Effect.succeed(sessionID)
						: Effect.fail(
								new DeskError({
									message: "No main session yet. It becomes known when a session first uses a desk tool, or pick one on the page.",
								}),
							),
				),
				claim: Effect.fn("Desk.MainSession.claim")(function* (sessionID) {
					const known = yield* current;
					if (known) return known;
					const root = yield* sessions.root(sessionID);
					yield* store.setMainSession(root);
					return root;
				}),
				select: Effect.fn("Desk.MainSession.select")(function* (sessionID) {
					yield* store.setMainSession(yield* sessions.root(sessionID));
				}),
			});
		}),
	);
}
