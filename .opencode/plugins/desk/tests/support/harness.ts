/** Builds the desk's real layers in a scope, with `Sessions` faked, for tests. */
import * as Context from "effect/Context";
import * as Duration from "effect/Duration";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as Layer from "effect/Layer";
import * as Scope from "effect/Scope";
import * as Stream from "effect/Stream";
import * as App from "../../app";
import { DeskError } from "../../config";
import type { Desk } from "../../desk";
import type { PageUrl } from "../../http/server";
import type { MainSession } from "../../main-session";
import { type Delivery, Sessions } from "../../sessions";
import { type Snapshot, Store } from "../../store";

export interface Prompt {
	readonly sessionID: string;
	readonly text: string;
	readonly delivery: Delivery;
}

/** Sessions as a parent map (`ses_child → ses_root`); prompts are recorded. */
export function fakeSessions(parents: Record<string, string> = {}) {
	const prompts: Array<Prompt> = [];
	const known = new Set(["ses_root", "ses_other", ...Object.keys(parents)]);
	const layer = Layer.succeed(Sessions, {
		root: (sessionID) => Effect.succeed(parents[sessionID] ?? sessionID),
		get: (sessionID) =>
			known.has(sessionID) ? Effect.succeed({ id: sessionID, title: `Title of ${sessionID}` }) : Effect.fail(new DeskError({ message: `no ${sessionID}` })),
		list: Effect.succeed([
			{ id: "ses_root", title: "Root" },
			{ id: "ses_other", title: "Other" },
		]),
		prompt: (sessionID, text, delivery) => Effect.sync(() => void prompts.push({ sessionID, text, delivery })),
	});
	return { layer, prompts };
}

type Services = Desk | MainSession | Store | Sessions;

export interface Harness {
	readonly prompts: ReadonlyArray<Prompt>;
	readonly run: <A, E>(effect: Effect.Effect<A, E, Services | PageUrl>) => Promise<A>;
	/** The failure's message, or "succeeded". */
	readonly failure: <A>(effect: Effect.Effect<A, DeskError, Services>) => Promise<string>;
	/** Waits (up to 2 s) for a snapshot that satisfies `predicate`. */
	readonly until: (predicate: (snapshot: Snapshot) => boolean) => Promise<Snapshot>;
	readonly close: () => Promise<void>;
}

export async function open(
	directory: string,
	options: { readonly parents?: Record<string, string>; readonly mainSession?: string; readonly page?: boolean } = {},
): Promise<Harness> {
	const sessions = fakeSessions(options.parents);
	const base = Layer.mergeAll(App.Background, options.page ? App.Page(0) : Layer.empty).pipe(
		Layer.provideMerge(App.Core),
		Layer.provideMerge(sessions.layer),
		Layer.provide(App.Platform(directory, { mainSession: options.mainSession })),
	);
	const scope = Effect.runSync(Scope.make());
	const context = (await Effect.runPromise(Layer.buildWithScope(base, scope))) as Context.Context<Services | PageUrl>;
	const run = <A, E>(effect: Effect.Effect<A, E, Services | PageUrl>) => Effect.runPromise(Effect.provideContext(effect, context));
	return {
		prompts: sessions.prompts,
		run,
		async failure(effect) {
			const exit = await Effect.runPromiseExit(Effect.provideContext(effect, context));
			return Exit.isFailure(exit) ? String(exit.cause) : "succeeded";
		},
		until: (predicate) =>
			run(
				Store.use((store) =>
					store.changes.pipe(
						Stream.filter(predicate),
						Stream.runHead,
						Effect.flatMap(Effect.fromOption),
						Effect.timeout(Duration.seconds(2)),
					),
				),
			),
		close: () => Effect.runPromise(Scope.close(scope, Exit.void)),
	};
}
