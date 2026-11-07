/**
 * The desk: its entries and their files in `<root>/coordination/`. Loaded once (nobody edits the files by hand),
 * kept in a `SubscriptionRef`, and every change is written to its file inside the update.
 */
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Path from "effect/Path";
import * as Stream from "effect/Stream";
import * as SubscriptionRef from "effect/SubscriptionRef";
import { DeskError, type Draft } from "./api";
import { answerer, type Entry, fileName, idOf, newId, type Party, parse, serialize, waitingOn } from "./entry";
import { type Delivery, Sessions } from "./sessions";

type Entries = ReadonlyArray<Entry>;

export interface Question {
	readonly title: string;
	readonly body: string;
	readonly labels?: ReadonlyArray<string> | undefined;
	readonly parent?: string | undefined;
}

export class Desk extends Context.Service<
	Desk,
	{
		readonly root: string;
		/** The entries now, then after every change, by `created`. */
		readonly changes: Stream.Stream<Entries>;
		/** An agent asks; the entry belongs to the top-level session above `sessionID`. */
		readonly ask: (sessionID: string, question: Question) => Effect.Effect<Entry, DeskError>;
		/** An agent answers the user's question. */
		readonly answer: (id: string, body: string) => Effect.Effect<Entry, DeskError>;
		/** The asker marks an entry done; it leaves the desk once it has no follow-ups, and so on up its parents. */
		readonly resolve: (id: string, by: Party) => Effect.Effect<void, DeskError>;
		/** Unresolved entries, one per line, or one entry's thread in full. */
		readonly describe: (id?: string) => Effect.Effect<string, DeskError>;
		/** The user's drafts, all or nothing, as one message per session. Returns how many went. */
		readonly deliver: (drafts: ReadonlyArray<Draft>, delivery: Delivery) => Effect.Effect<number, DeskError>;
	}
>()("desk/Desk") {
	static layer(root: string) {
		return Layer.effect(
			this,
			Effect.gen(function* () {
				const fs = yield* FileSystem.FileSystem;
				const path = yield* Path.Path;
				const sessions = yield* Sessions;
				const directory = path.join(root, "coordination");
				const io = <A, E>(effect: Effect.Effect<A, E>) => effect.pipe(Effect.mapError((cause) => new DeskError({ message: String(cause) })));
				const write = (entry: Entry) => io(fs.writeFileString(path.join(directory, fileName(entry.id)), serialize(entry)));

				yield* fs.makeDirectory(directory, { recursive: true });
				const loaded: Array<Entry> = [];
				for (const name of yield* fs.readDirectory(directory)) {
					const id = idOf(name);
					if (id === undefined) continue;
					const entry = parse(id, yield* fs.readFileString(path.join(directory, name)));
					if (Option.isSome(entry)) loaded.push(entry.value);
				}
				const entries = yield* SubscriptionRef.make<Entries>(loaded.sort((a, b) => a.created.localeCompare(b.created)));

				const find = (all: Entries, id: string) => {
					const entry = all.find((candidate) => candidate.id === id);
					return entry ? Effect.succeed(entry) : Effect.fail(new DeskError({ message: `No desk entry ${id}.` }));
				};
				const replace = (all: Entries, entry: Entry) =>
					all.some((other) => other.id === entry.id) ? all.map((other) => (other.id === entry.id ? entry : other)) : [...all, entry];

				/** The user's draft: the answer when the entry waits on them, otherwise a follow-up question below it. */
				const reply = (all: Entries, draft: Draft) =>
					Effect.gen(function* () {
						const entry = yield* find(all, draft.entry);
						const text = draft.text.trim();
						if (!text) return yield* new DeskError({ message: `Entry ${entry.id}: the text is empty.` });
						if (entry.status === "resolved") return yield* new DeskError({ message: `Entry ${entry.id} is already resolved.` });
						if (entry.asker === "agent" && entry.status === "open") {
							const answered: Entry = { ...entry, answer: text, status: "answered" };
							return { entry: answered, section: `## ${entry.id} Answered: ${entry.title}\n\n${text}` };
						}
						const followUp: Entry = {
							id: newId(),
							parent: entry.id,
							asker: "user",
							status: "open",
							session: entry.session,
							labels: entry.labels,
							created: new Date().toISOString(),
							title: text.split("\n")[0]!.slice(0, 100),
							question: text,
							answer: null,
						};
						return { entry: followUp, section: `## ${followUp.id} New question from the user (follow-up to ${entry.id})\n\n${text}` };
					});

				return Desk.of({
					root,
					changes: SubscriptionRef.changes(entries),
					ask: (sessionID, question) =>
						Effect.gen(function* () {
							const session = yield* sessions.root(sessionID);
							return yield* SubscriptionRef.modifyEffect(entries, (all) =>
								Effect.gen(function* () {
									if (question.parent !== undefined) yield* find(all, question.parent);
									const entry: Entry = {
										id: newId(),
										parent: question.parent ?? null,
										asker: "agent",
										status: "open",
										session,
										labels: question.labels ?? [],
										created: new Date().toISOString(),
										title: question.title.trim(),
										question: question.body.trim(),
										answer: null,
									};
									yield* write(entry);
									return [entry, [...all, entry]] as const;
								}),
							);
						}),
					answer: (id, body) =>
						SubscriptionRef.modifyEffect(entries, (all) =>
							Effect.gen(function* () {
								const entry = yield* find(all, id);
								if (entry.asker !== "user") return yield* new DeskError({ message: `Entry ${id} is the agents' own question; the user answers it.` });
								if (entry.status === "resolved") return yield* new DeskError({ message: `Entry ${id} is already resolved.` });
								const answered: Entry = { ...entry, answer: body.trim(), status: "answered" };
								yield* write(answered);
								return [answered, replace(all, answered)] as const;
							}),
						),
					resolve: (id, by) =>
						SubscriptionRef.modifyEffect(entries, (all) =>
							Effect.gen(function* () {
								const entry = yield* find(all, id);
								if (entry.asker !== by) return yield* new DeskError({ message: `Entry ${id} was asked by the ${entry.asker}; only the asker marks it done.` });
								const resolved: Entry = { ...entry, status: "resolved" };
								yield* write(resolved);
								// A resolved entry without follow-ups leaves the desk; then its parent may, and so on up.
								let kept = replace(all, resolved);
								for (let current: Entry | undefined = resolved; current?.status === "resolved" && !kept.some((other) => other.parent === current!.id); ) {
									yield* io(fs.remove(path.join(directory, fileName(current.id))));
									kept = kept.filter((other) => other.id !== current!.id);
									current = kept.find((other) => other.id === current!.parent);
								}
								return [undefined, kept] as const;
							}),
						),
					describe: (id) =>
						Effect.gen(function* () {
							const all = yield* SubscriptionRef.get(entries);
							if (id === undefined) {
								const open = all.filter((entry) => entry.status !== "resolved");
								if (open.length === 0) return "The desk is empty: nothing open or waiting.";
								return open
									.map((entry) => {
										const parent = entry.parent === null ? "" : ` (follow-up to ${entry.parent})`;
										return `- ${entry.id} ${entry.title}${parent} — ${entry.status}, waiting on the ${waitingOn(entry)}, asked by the ${entry.asker}`;
									})
									.join("\n");
							}
							const thread: Array<Entry> = [];
							for (let current: Entry | undefined = yield* find(all, id); current; current = all.find((entry) => entry.id === current!.parent)) thread.unshift(current);
							const followUps = all.filter((entry) => entry.parent === id).map((entry) => `${entry.id} ${entry.title} (${entry.status})`);
							return [
								...thread.map((entry) =>
									[`# ${entry.id} ${entry.title}`, `(${entry.status}; asked by the ${entry.asker}, answered by the ${answerer(entry)})`, entry.question, ...(entry.answer === null ? [] : ["## Answer", entry.answer])].join("\n\n"),
								),
								...(followUps.length ? [`Follow-ups: ${followUps.join("; ")}`] : []),
							].join("\n\n---\n\n");
						}),
					deliver: (drafts, delivery) =>
						Effect.gen(function* () {
							const messages = yield* SubscriptionRef.modifyEffect(entries, (all) =>
								Effect.gen(function* () {
									// Apply every draft before writing any, so a refusal leaves the desk as it was.
									let next = all;
									const sections = new Map<string, Array<string>>();
									for (const draft of drafts) {
										const { entry, section } = yield* reply(next, draft);
										next = replace(next, entry);
										sections.set(entry.session, [...(sections.get(entry.session) ?? []), section]);
									}
									yield* Effect.forEach(next.filter((entry) => !all.includes(entry)), write);
									return [sections, next] as const;
								}),
							);
							for (const [sessionID, sections] of messages) {
								yield* sessions.prompt({ sessionID, text: message(sections), delivery });
							}
							return drafts.length;
						}),
				});
			}),
		);
	}
}

function message(sections: ReadonlyArray<string>): string {
	return [
		"Desk: the user replied on the desk.",
		...sections,
		"---",
		"Answer the user's questions with desk_answer, ask follow-ups with desk_ask (parent: the entry), and mark your own questions done with desk_resolve once the outcome is applied in the repo (glossary, ADR, issue, code). desk_list shows the desk.",
	].join("\n\n");
}
