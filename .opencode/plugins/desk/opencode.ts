/** What the desk adds to OpenCode: the `desk_*` tools for agents, and the `desk` RPC the TUI badge reads. */
import type { Plugin } from "@opencode/plugin/effect";
import * as Data from "effect/Data";
import * as Effect from "effect/Effect";
import * as HttpServer from "effect/http/HttpServer";
import * as Layer from "effect/Layer";
import * as Schema from "effect/Schema";
import * as Stream from "effect/Stream";
import type { DeskError } from "./api";
import { Desk } from "./desk";
import { Id, Priority, waitingOn } from "./entry";
import { DeskRpc } from "./tui-rpc";

/** Shaped like OpenCode's `Tool.Error`, so the model sees the message. */
class ToolError extends Data.TaggedError("Tool.Error")<{ readonly message: string }> {}

const media =
	"Markdown: text, code fences, ```mermaid diagrams, images and GIFs (![alt](path)), video (![alt](clip.mp4)). Link repo files by path relative to the repo root, e.g. ![icon](design/icons/01.png).";

export const layer = (ctx: Plugin.Context) =>
	Layer.effectDiscard(
		Effect.gen(function* () {
			const desk = yield* Desk;
			const server = yield* HttpServer.HttpServer;
			const url = server.address._tag === "UnixPathAddress" ? "" : `http://127.0.0.1:${server.address.port}/`;
			const tool = (effect: Effect.Effect<string, DeskError>) =>
				effect.pipe(
					Effect.map((content) => ({ content })),
					Effect.mapError((error) => new ToolError({ message: error.message }) as never),
				);

			yield* ctx.tool.transform((editor) => {
				editor.add({
					name: "desk_ask",
					description: `Ask the user a question (or request a review) on the desk instead of in chat. The user answers on the desk page; the answer arrives later as a message in this session's top-level session. Use parent to follow up on an entry. A PR's questions (label pr:<n>) are filed under that PR's review entry automatically when parent is omitted. ${media}`,
					input: Schema.Struct({
						title: Schema.String.annotate({ description: "One line: the question itself." }),
						body: Schema.String.annotate({ description: "Context, options and your recommendation, in Markdown." }),
						labels: Schema.optional(Schema.Array(Schema.String)).annotate({ description: "Short topic tags, e.g. glossary, review." }),
						parent: Schema.optional(Id).annotate({ description: "The id of the entry this follows up on." }),
						priority: Schema.optional(Priority).annotate({ description: "high, normal or low; the user sees higher priority first. Default: the parent's." }),
					}),
					options: { codemode: false },
					execute: (input, context) =>
						tool(
							Effect.map(
								desk.ask(context.sessionID, input),
								(entry) => `Asked ${entry.id}, ${entry.parent === null ? "top-level" : `under ${entry.parent}${input.parent === undefined ? " (the PR's review entry)" : ""}`}. ${url}#/${entry.id}`,
							),
						),
				});
				editor.add({
					name: "desk_adopt",
					description: "File a top-level entry (one without a parent) under another entry, e.g. a PR's question asked before its review entry existed. Only once: an entry's parent never changes after that.",
					input: Schema.Struct({ id: Id, parent: Id.annotate({ description: "The entry to file it under; not inside the entry's own subtree." }) }),
					options: { codemode: false },
					execute: (input) => tool(Effect.as(desk.adopt(input.id, input.parent), `Filed ${input.id} under ${input.parent}.`)),
				});
				editor.add({
					name: "desk_answer",
					description: `Answer a question the user asked on the desk. ${media}`,
					input: Schema.Struct({ id: Id, body: Schema.String.annotate({ description: "The answer, in Markdown." }) }),
					options: { codemode: false },
					execute: (input) => tool(Effect.as(desk.answer(input.id, input.body), `Answered ${input.id}.`)),
				});
				editor.add({
					name: "desk_resolve",
					description: "Mark one of your own desk questions done, once its outcome is applied in the repo (glossary, ADR, issue, code). Resolved entries without open follow-ups leave the desk.",
					input: Schema.Struct({ id: Id }),
					options: { codemode: false },
					execute: (input) => tool(Effect.as(desk.resolve(input.id, "agent"), `Resolved ${input.id}.`)),
				});
				editor.add({
					name: "desk_prioritize",
					description: "Change an entry's priority (high, normal, low): the user sees higher priority first, e.g. bump a main PR's review thread. With subtree, the entry and everything below it.",
					input: Schema.Struct({ id: Id, priority: Priority, subtree: Schema.optional(Schema.Boolean) }),
					options: { codemode: false },
					execute: (input) => tool(Effect.as(desk.prioritize(input.id, input.priority, input.subtree ?? false), `Set ${input.id} to ${input.priority}${input.subtree ? " (with everything below it)" : ""}.`)),
				});
				editor.add({
					name: "desk_list",
					description: "Show the desk: every unresolved entry (id, title, whose turn it is), or one entry's full thread when id is given. Entry ids are full UUIDs; the user may paste one.",
					input: Schema.Struct({ id: Schema.optional(Id) }),
					options: { codemode: false },
					execute: (input) => tool(desk.describe(input.id)),
				});
			});

			const status = (entries: ReadonlyArray<{ readonly status: "open" | "answered" | "resolved"; readonly asker: "user" | "agent" }>) => ({
				url,
				waiting: entries.filter((entry) => waitingOn(entry) === "user").length,
			});
			const rpc = yield* ctx.rpc.register(DeskRpc, { status: () => Effect.map(Stream.runHead(desk.changes), (entries) => status(entries._tag === "Some" ? entries.value : [])) });
			yield* desk.changes.pipe(
				Stream.map(status),
				Stream.changesWith((a, b) => a.waiting === b.waiting),
				Stream.runForEach((next) => Effect.ignore(rpc.events.emit("changed", next))),
				Effect.forkScoped,
			);
		}),
	);
