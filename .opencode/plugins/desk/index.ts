/**
 * desk — questions, answers and reviews between the user and the agents.
 *
 * Agents use the desk_* tools; the user uses the page this plugin serves; the TUI footer shows how many entries
 * wait on the user. Entries live in the checkout's gitignored `coordination/` directory.
 */
import { resolve } from "node:path";
import { Plugin } from "@opencode/plugin/effect";
import type { Session } from "@opencode/schema/session";
import type { AbsolutePath } from "@opencode/schema/schema";
import * as Data from "effect/Data";
import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";
import * as Desk from "./desk";
import { DeskRpc } from "./rpc";
import * as Http from "./http";
import * as Store from "./store";
import { DeskError } from "./store";
import { waitingCount } from "./tree";

/** `.opencode/plugins/desk/` → the checkout it belongs to. */
const checkout = resolve(import.meta.dir, "../../..");

const markdownHelp =
	"Markdown: text, code fences, ```mermaid diagrams, images and GIFs (![alt](path)), video (![alt](clip.mp4)). Link repo files by path relative to the repo root, e.g. ![icon](design/icons/01b.png).";

export default Plugin.define({
	id: "desk",
	effect(ctx) {
		return Effect.gen(function* () {
			const root = typeof ctx.options.directory === "string" ? resolve(checkout, ctx.options.directory) : checkout;
			const mainSession = typeof ctx.options.mainSession === "string" ? ctx.options.mainSession : undefined;
			const store = yield* Store.make(resolve(root, "coordination"));
			const desk = Desk.make(store, sessions(ctx, root), { mainSession });

			const server = yield* Effect.acquireRelease(Http.start(desk, root), (server) => Effect.sync(server.stop));
			yield* Effect.logInfo("desk page", { url: server.url, owned: server.owned });

			const status = () => ({ url: server.url, waiting: waitingCount(store.snapshot().entries, "user") });
			const rpc = yield* ctx.rpc.register(DeskRpc, { status: () => Effect.succeed(status()) });
			let last = -1;
			const unsubscribe = store.subscribe(() => {
				const next = status();
				if (next.waiting === last) return;
				last = next.waiting;
				Effect.runFork(Effect.ignore(rpc.events.emit("changed", next)));
			});
			yield* Effect.addFinalizer(() => Effect.sync(unsubscribe));

			yield* ctx.tool.transform((editor) => {
				editor.add({
					name: "desk_ask",
					description: `Ask the user a question (or request a review) on the desk instead of in chat. The user answers on the desk page; answers arrive later as a message in the main session. Use parent to follow up on an entry. ${markdownHelp} Returns the entry id.`,
					input: Schema.Struct({
						title: Schema.String.annotate({ description: "One line: the question itself." }),
						body: Schema.String.annotate({ description: "Context, options and your recommendation, in Markdown." }),
						labels: Schema.optional(Schema.Array(Schema.String)).annotate({ description: "Short topic tags, e.g. glossary, review, design." }),
						parent: Schema.optional(Schema.Int).annotate({ description: "The entry this follows up on." }),
					}),
					options: { codemode: false },
					execute: (input, context) =>
						tool(context.sessionID, desk.ask(input).pipe(Effect.map((entry) => `Asked #${entry.id}: ${entry.title}. ${server.url}`))),
				});
				editor.add({
					name: "desk_answer",
					description: `Answer a question the user asked on the desk. ${markdownHelp}`,
					input: Schema.Struct({ id: Schema.Int, body: Schema.String.annotate({ description: "The answer, in Markdown." }) }),
					options: { codemode: false },
					execute: (input, context) =>
						tool(context.sessionID, desk.answer(input.id, input.body).pipe(Effect.map((entry) => `Answered #${entry.id}.`))),
				});
				editor.add({
					name: "desk_resolve",
					description:
						"Mark one of your own desk questions done, once its outcome is applied in the repo (glossary, ADR, issue, code). Resolved entries without open follow-ups are removed from the desk.",
					input: Schema.Struct({ id: Schema.Int }),
					options: { codemode: false },
					execute: (input, context) =>
						tool(context.sessionID, desk.resolve(input.id).pipe(Effect.map((entry) => `Resolved #${entry.id}.`))),
				});
				editor.add({
					name: "desk_list",
					description: "Show the desk: every unresolved entry as a tree with whose turn it is, or one entry's full thread when id is given.",
					input: Schema.Struct({ id: Schema.optional(Schema.Int) }),
					options: { codemode: false },
					execute: (input, context) => tool(context.sessionID, desk.describe(input.id)),
				});
			});

			function tool(sessionID: string, effect: Effect.Effect<string, DeskError>) {
				return desk.claim(sessionID).pipe(
					Effect.ignore,
					Effect.andThen(effect),
					Effect.map((content) => ({ content })),
					Effect.mapError((error) => new ToolError({ message: error.message }) as never),
				);
			}
		}).pipe(Effect.orDie);
	},
});

/**
 * Shaped like OpenCode's `Tool.Error`, which lives in `@opencode/schema`: a package the host does not provide to
 * plugins without their own install.
 */
class ToolError extends Data.TaggedError("Tool.Error")<{ readonly message: string }> {}

function sessions(ctx: Plugin.Context, root: string): Desk.Sessions {
	const failed = (what: string) => (cause: unknown) => new DeskError({ message: `Could not ${what}: ${String(cause)}`, cause });
	const get = (sessionID: string) =>
		ctx.session.get({ sessionID: sessionID as Session.ID }).pipe(Effect.mapError(failed(`find session ${sessionID}`)));
	return {
		root: Effect.fn("Desk.sessions.root")(function* (sessionID) {
			let session = yield* get(sessionID);
			for (let depth = 0; session.parentID && depth < 32; depth++) session = yield* get(session.parentID);
			return session.id;
		}),
		get: (sessionID) => get(sessionID).pipe(Effect.map((session) => ({ id: session.id, title: session.title ?? session.id }))),
		list: () =>
			ctx.session.list({ directory: root as AbsolutePath, parentID: null, limit: 30, order: "desc" }).pipe(
				Effect.map((page) => page.data.map((session) => ({ id: session.id, title: session.title ?? session.id }))),
				Effect.mapError(failed("list sessions")),
			),
		prompt: (sessionID, text, delivery) =>
			ctx.session.prompt({ sessionID: sessionID as Session.ID, text, delivery }).pipe(
				Effect.asVoid,
				Effect.mapError(failed("message the main session")),
			),
	};
}
