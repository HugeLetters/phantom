/**
 * Outside edits to `coordination/` (a person or an agent editing an entry file) reach the store: the directory is
 * watched, bursts of events are coalesced into one reload. When the directory is deleted or replaced, the watch is
 * re-attached to the directory that exists.
 */
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Stream from "effect/Stream";
import { DeskConfig } from "./config";
import { Store } from "./store";

export const layer = Layer.effectDiscard(
	Effect.gen(function* () {
		const fs = yield* FileSystem.FileSystem;
		const store = yield* Store;
		const { directory } = yield* DeskConfig;

		const inode = fs.stat(directory).pipe(
			Effect.map((info) => Option.getOrUndefined(info.ino)),
			Effect.orElseSucceed(() => undefined),
		);

		/** Watch the directory as it is now, until it is no longer the same directory. */
		const attach = Effect.gen(function* () {
			yield* store.reload; // also recreates the directory
			const watched = yield* inode;
			yield* fs.watch(directory, { recursive: true }).pipe(
				Stream.debounce("75 millis"),
				Stream.tap(() => Effect.ignore(store.reload)),
				Stream.takeUntilEffect(() => Effect.map(inode, (current) => current !== watched)),
				Stream.runDrain,
			);
		});

		yield* attach.pipe(
			Effect.catchCause((cause) => Effect.logWarning("desk: watching coordination/ failed; retrying", cause)),
			Effect.andThen(Effect.sleep("100 millis")),
			Effect.forever,
			Effect.forkScoped,
		);
	}),
);
