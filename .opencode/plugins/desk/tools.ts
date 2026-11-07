/** The agents' side of the desk: `desk_ask`, `desk_answer`, `desk_resolve`, `desk_list`. */
import * as Data from "effect/Data";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Schema from "effect/Schema";
import { type DeskError, PluginContext } from "./config";
import { Desk } from "./desk";
import { MainSession } from "./main-session";
import { PageUrl } from "./http/server";

/**
 * Shaped like OpenCode's `Tool.Error`, which lives in `@opencode/schema`: a package OpenCode does not provide to
 * plugins without their own install.
 */
class ToolError extends Data.TaggedError("Tool.Error")<{ readonly message: string }> {}

const markdownHelp =
	"Markdown: text, code fences, ```mermaid diagrams, images and GIFs (![alt](path)), video (![alt](clip.mp4)). Link repo files by path relative to the repo root, e.g. ![icon](design/icons/01b.png).";

export const layer = Layer.effectDiscard(
	Effect.gen(function* () {
		const ctx = yield* PluginContext;
		const desk = yield* Desk;
		const main = yield* MainSession;
		const page = yield* PageUrl;

		/** Every call first settles the main session, then runs; failures reach the model as their message. */
		const run = (sessionID: string, effect: Effect.Effect<string, DeskError>) =>
			main.claim(sessionID).pipe(
				Effect.ignore,
				Effect.andThen(effect),
				Effect.map((content) => ({ content })),
				Effect.mapError((error) => new ToolError({ message: error.message }) as never),
			);

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
					run(context.sessionID, desk.ask(input).pipe(Effect.map((entry) => `Asked #${entry.id}: ${entry.title}. ${page.url}`))),
			});
			editor.add({
				name: "desk_answer",
				description: `Answer a question the user asked on the desk. ${markdownHelp}`,
				input: Schema.Struct({ id: Schema.Int, body: Schema.String.annotate({ description: "The answer, in Markdown." }) }),
				options: { codemode: false },
				execute: (input, context) => run(context.sessionID, desk.answer(input.id, input.body).pipe(Effect.map((entry) => `Answered #${entry.id}.`))),
			});
			editor.add({
				name: "desk_resolve",
				description:
					"Mark one of your own desk questions done, once its outcome is applied in the repo (glossary, ADR, issue, code). Resolved entries without open follow-ups are removed from the desk.",
				input: Schema.Struct({ id: Schema.Int }),
				options: { codemode: false },
				execute: (input, context) => run(context.sessionID, desk.resolve(input.id).pipe(Effect.map((entry) => `Resolved #${entry.id}.`))),
			});
			editor.add({
				name: "desk_list",
				description: "Show the desk: every unresolved entry as a tree with whose turn it is, or one entry's full thread when id is given.",
				input: Schema.Struct({ id: Schema.optional(Schema.Int) }),
				options: { codemode: false },
				execute: (input, context) => run(context.sessionID, desk.describe(input.id)),
			});
		});
	}),
);
