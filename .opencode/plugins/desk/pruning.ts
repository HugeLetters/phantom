/**
 * Resolved entries with nothing unresolved below them leave the desk, bottom-up, whenever the store changes: after a
 * resolve through the tools or the page, and after an outside edit. Nothing goes before its asker resolves it.
 */
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Stream from "effect/Stream";
import { Store } from "./store";
import { prunable } from "./tree";

export const layer = Layer.effectDiscard(
	Effect.gen(function* () {
		const store = yield* Store;
		yield* store.changes.pipe(
			Stream.map((snapshot) => prunable(snapshot.entries)),
			Stream.filter((ids) => ids.length > 0),
			Stream.runForEach((ids) =>
				store.remove(ids).pipe(Effect.catch((error) => Effect.logWarning("desk: pruning failed", error.message))),
			),
			Effect.forkScoped,
		);
	}),
);
