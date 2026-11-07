/** What the TUI badge reads: `status` on request, and `changed` whenever the count waiting on the user changes. */
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Stream from "effect/Stream";
import { PluginContext } from "./config";
import { PageUrl } from "./http/server";
import { DeskRpc, type Status } from "./rpc";
import { Store } from "./store";
import { waitingCount } from "./tree";

export const layer = Layer.effectDiscard(
	Effect.gen(function* () {
		const ctx = yield* PluginContext;
		const store = yield* Store;
		const { url } = yield* PageUrl;
		const status = Effect.map(store.snapshot, (snapshot): Status => ({ url, waiting: waitingCount(snapshot.entries, "user") }));

		const rpc = yield* ctx.rpc.register(DeskRpc, { status: () => status });
		yield* store.changes.pipe(
			Stream.map((snapshot) => waitingCount(snapshot.entries, "user")),
			Stream.changes,
			Stream.runForEach((waiting) => Effect.ignore(rpc.events.emit("changed", { url, waiting }))),
			Effect.forkScoped,
		);
	}),
);
