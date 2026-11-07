/** The desk's operations for both sides: the agents (through tools, as the main session) and the user (the page). */
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import { DeskError } from "./config";
import { type Entry, type Party, waitingOn } from "./entry";
import { MainSession } from "./main-session";
import { type Delivery, Sessions } from "./sessions";
import { type Draft, Store } from "./store";
import { build, type Node, thread, waitingCount } from "./tree";

export interface Question {
	readonly title: string;
	readonly body: string;
	readonly labels?: ReadonlyArray<string> | undefined;
	readonly parent?: number | undefined;
}

export class Desk extends Context.Service<
	Desk,
	{
		// Agents
		readonly ask: (question: Question) => Effect.Effect<Entry, DeskError>;
		readonly answer: (id: number, body: string) => Effect.Effect<Entry, DeskError>;
		readonly resolve: (id: number) => Effect.Effect<Entry, DeskError>;
		/** Unresolved entries as an outline, or one entry's thread in full. */
		readonly describe: (id?: number) => Effect.Effect<string, DeskError>;
		// User
		readonly putDraft: (draft: Draft) => Effect.Effect<void, DeskError>;
		/** Every draft in one message, after the main session's current turn. Returns how many went. */
		readonly submit: Effect.Effect<number, DeskError>;
		/** One draft, right away (steers the current turn). */
		readonly send: (key: string) => Effect.Effect<number, DeskError>;
		readonly resolveOwn: (id: number) => Effect.Effect<Entry, DeskError>;
	}
>()("desk/Desk") {
	static readonly layer = Layer.effect(
		this,
		Effect.gen(function* () {
			const store = yield* Store;
			const sessions = yield* Sessions;
			const main = yield* MainSession;

			const resolveAs = (party: Party) => (id: number) =>
				store.update(id, (entry) => {
					if (entry.asker !== party) {
						return new DeskError({
							message: `#${id} was asked by the ${party === "main" ? "user" : "agents"}; only the asker marks it done.`,
						});
					}
					return { ...entry, status: "resolved" };
				});

			/** Answers land in their entries, new questions become entries; returns this draft's part of the message. */
			const apply = (draft: Draft): Effect.Effect<string, DeskError> =>
				draft.kind === "ask"
					? Effect.map(
							store.create({ asker: "user", title: draft.title || firstLine(draft.text), question: draft.text, parent: draft.parent }),
							(entry) =>
								[
									`## #${entry.id} New question from the user: ${entry.title}${entry.parent !== null ? ` (follow-up to #${entry.parent})` : ""}`,
									entry.question || "(no details)",
								].join("\n\n"),
						)
					: Effect.map(
							store.update(draft.entry, (entry) => {
								if (entry.answerer !== "user") return new DeskError({ message: `#${entry.id} is the user's own question; the agents answer it.` });
								if (entry.status === "resolved") return new DeskError({ message: `#${entry.id} is already resolved.` });
								return { ...entry, answer: draft.text.trim(), status: "answered" };
							}),
							(entry) => [`## #${entry.id} Answered: ${entry.title}`, entry.answer ?? ""].join("\n\n"),
						);

			/** Drafts that no longer apply stay drafts and are reported; the others still go. */
			const deliver = Effect.fn("Desk.deliver")(function* (keys: ReadonlyArray<string> | "all", delivery: Delivery) {
				const sessionID = yield* main.require;
				yield* sessions.get(sessionID);
				const { drafts } = yield* store.snapshot;
				const chosen = Object.entries(drafts).filter(([key]) => keys === "all" || keys.includes(key));
				if (chosen.length === 0) return yield* new DeskError({ message: "Nothing to send: there are no drafts." });
				const results = yield* Effect.forEach(chosen, ([key, draft]) =>
					apply(draft).pipe(
						Effect.map((section) => ({ key, section })),
						Effect.result,
					),
				);
				const sent = results.flatMap((result) => (result._tag === "Success" ? [result.success] : []));
				const failures = results.flatMap((result) => (result._tag === "Failure" ? [result.failure.message] : []));
				yield* store.removeDrafts(sent.map(({ key }) => key));
				if (sent.length > 0) yield* sessions.prompt(sessionID, message(sent.map(({ section }) => section)), delivery);
				if (failures.length > 0) return yield* new DeskError({ message: failures.join("\n") });
				return sent.length;
			});

			const describe = Effect.fn("Desk.describe")(function* (id?: number) {
				yield* store.reload;
				const { entries } = yield* store.snapshot;
				if (id !== undefined) {
					const path = thread(entries, id);
					if (path.length === 0) return yield* new DeskError({ message: `No desk entry #${id}.` });
					const below = entries.filter((entry) => entry.parent === id);
					return [
						...path.map(render),
						...(below.length ? [`Follow-ups: ${below.map((entry) => `#${entry.id} ${entry.title} (${entry.status})`).join("; ")}`] : []),
					].join("\n\n---\n\n");
				}
				const lines = outline(build(entries), 0);
				if (lines.length === 0) return "The desk is empty: nothing open or waiting.";
				return `${lines.join("\n")}\n\nWaiting on the user: ${waitingCount(entries, "user")}. Waiting on the agents: ${waitingCount(entries, "main")}.`;
			});

			return Desk.of({
				ask: (question) =>
					store.create({ asker: "main", title: question.title, question: question.body, labels: question.labels, parent: question.parent }),
				answer: (id, body) =>
					store.update(id, (entry) => {
						if (entry.answerer !== "main") return new DeskError({ message: `#${id} is the agents' own question; the user answers it.` });
						if (entry.status === "resolved") return new DeskError({ message: `#${id} is already resolved.` });
						return { ...entry, answer: body.trim(), status: "answered" };
					}),
				resolve: resolveAs("main"),
				describe,
				putDraft: store.putDraft,
				submit: deliver("all", "queue"),
				send: (key) => deliver([key], "steer"),
				resolveOwn: resolveAs("user"),
			});
		}),
	);
}

function outline(nodes: ReadonlyArray<Node>, depth: number): Array<string> {
	return nodes.flatMap((node) => {
		if (!node.active) return [];
		const { entry } = node;
		const turn = waitingOn(entry);
		const labels = entry.labels.length ? ` [${entry.labels.join(", ")}]` : "";
		const line = `${"  ".repeat(depth)}- #${entry.id} ${entry.title} — ${entry.status}${turn ? `, waiting on ${turn === "main" ? "agents" : "user"}` : ""} (asked by ${entry.asker === "main" ? "agents" : "user"})${labels}`;
		return [line, ...outline(node.children, depth + 1)];
	});
}

function message(sections: ReadonlyArray<string>): string {
	return [
		"Desk: the user replied on the desk.",
		...sections,
		"---",
		"Answer the user's questions with desk_answer, ask follow-ups with desk_ask (parent: the entry), and mark your own questions done with desk_resolve once the outcome is applied in the repo (glossary, ADR, issue, code). desk_list shows the desk.",
	].join("\n\n");
}

function render(entry: Entry): string {
	const meta = [
		`#${entry.id}`,
		entry.status,
		`asked by ${entry.asker === "main" ? "agents" : "user"}`,
		entry.parent !== null ? `follow-up to #${entry.parent}` : null,
		entry.labels.length ? `labels: ${entry.labels.join(", ")}` : null,
		entry.created,
	].filter(Boolean);
	return [`# ${entry.title}`, `(${meta.join(" · ")})`, entry.question || "(no details)", ...(entry.answer !== null ? ["## Answer", entry.answer] : [])].join("\n\n");
}

function firstLine(text: string): string {
	return text.trim().split("\n")[0]?.slice(0, 80) || "Question";
}
