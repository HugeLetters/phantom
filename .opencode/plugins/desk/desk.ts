/**
 * The desk: its entries and their files in `<root>/coordination/`. Loaded once (nobody edits the files by hand),
 * kept in a `SubscriptionRef`, and every change is written to its file inside the update.
 */
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import { pathToFileURL } from "node:url";
import * as Path from "effect/Path";
import * as Stream from "effect/Stream";
import * as SubscriptionRef from "effect/SubscriptionRef";
import { DeskError, type Draft } from "./api";
import { answerer, attachmentsIn, descendants, type Entry, fileName, idOf, newId, openUnder, ordered, type Party, type Priority, parse, serialize, waitingOn } from "./entry";
import { type Delivery, Sessions } from "./sessions";

type Entries = ReadonlyArray<Entry>;

export interface Question {
	readonly title: string;
	readonly body: string;
	readonly labels?: ReadonlyArray<string> | undefined;
	/** Omitted with a `pr:<n>` label: the PR's review root, if there is one (see `reviewRoot`). */
	readonly parent?: string | undefined;
	/** Default: the parent's, else normal. */
	readonly priority?: Priority | undefined;
}

/**
 * The newest unresolved top-level entry labelled `review` and the question's `pr:<n>`: where a PR's questions belong.
 * An entry labelled `review` is itself a review root, so it has none.
 */
export function reviewRoot(all: Entries, labels: ReadonlyArray<string>): Entry | undefined {
	if (labels.includes("review")) return undefined;
	return labels
		.filter((label) => /^pr:\d+$/.test(label))
		.flatMap((pr) => all.filter((entry) => entry.parent === null && entry.status !== "resolved" && entry.labels.includes("review") && entry.labels.includes(pr)))
		.sort((a, b) => (a.created < b.created ? 1 : a.created > b.created ? -1 : 0))[0];
}

export class Desk extends Context.Service<
	Desk,
	{
		readonly root: string;
		/** The entries now, then after every change, by `created`. */
		readonly changes: Stream.Stream<Entries>;
		/** An agent asks; the entry belongs to the top-level session above `sessionID`. */
		readonly ask: (sessionID: string, question: Question) => Effect.Effect<Entry, DeskError>;
		/** Files a top-level entry under `parent` (not in its own subtree); an entry's parent never changes once set. */
		readonly adopt: (id: string, parent: string) => Effect.Effect<Entry, DeskError>;
		/** An agent answers the user's question. */
		readonly answer: (id: string, body: string) => Effect.Effect<Entry, DeskError>;
		/** Sets an entry's priority, and its whole subtree's when `subtree`. */
		readonly prioritize: (id: string, priority: Priority, subtree: boolean) => Effect.Effect<void, DeskError>;
		/** The asker marks an entry done (refused while anything below it is unresolved); it leaves the desk once it has no follow-ups, and so on up its parents. */
		readonly resolve: (id: string, by: Party) => Effect.Effect<void, DeskError>;
		/** Unresolved entries, one per line, or one entry's thread in full. */
		readonly describe: (id?: string) => Effect.Effect<string, DeskError>;
		/**
		 * The user's drafts, all or nothing, as one message per session; the attachments their texts reference are stored
		 * under `coordination/attachments/` on the way, and only if every draft goes. Returns how many went.
		 */
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

				const attachments = path.join(directory, "attachments");
				/**
				 * The draft with its `attachment:<id>` references pointed at the files they'll be stored as, and those files.
				 * Attachments the text no longer references are dropped; one it references but doesn't carry is refused.
				 */
				const store = (draft: Draft) =>
					Effect.gen(function* () {
						let text = draft.text;
						const uploads: Array<{ file: string; data: string }> = [];
						for (const id of new Set([...draft.text.matchAll(/\]\(attachment:([0-9a-f-]{36})\)/g)].map(([, id]) => id!))) {
							const attachment = draft.files?.find((candidate) => candidate.id === id);
							if (!attachment) return yield* new DeskError({ message: `Entry ${draft.entry}: attachment ${id} is missing from the draft; attach the file again.` });
							const extension = /\.([a-z0-9]{1,8})$/i.exec(attachment.name)?.[1]?.toLowerCase() ?? "bin";
							const file = `coordination/attachments/${newId()}.${extension}`;
							uploads.push({ file, data: attachment.data });
							text = text.replaceAll(`](attachment:${id})`, `](${file})`);
						}
						return { draft: { entry: draft.entry, text }, uploads };
					});

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
							priority: entry.priority,
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
									const parent = question.parent === undefined ? reviewRoot(all, question.labels ?? []) : yield* find(all, question.parent);
									const entry: Entry = {
										id: newId(),
										parent: parent?.id ?? null,
										asker: "agent",
										status: "open",
										session,
										labels: question.labels ?? [],
										priority: question.priority ?? parent?.priority ?? "normal",
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
					adopt: (id, parentID) =>
						SubscriptionRef.modifyEffect(entries, (all) =>
							Effect.gen(function* () {
								const entry = yield* find(all, id);
								yield* find(all, parentID);
								if (entry.parent !== null) return yield* new DeskError({ message: `Entry ${id} already has a parent (${entry.parent}); a parent never changes.` });
								if (parentID === id || descendants(all, entry).some((other) => other.id === parentID)) return yield* new DeskError({ message: `Entry ${parentID} is in the subtree of ${id}; adopting it there would make a cycle.` });
								const adopted: Entry = { ...entry, parent: parentID };
								yield* write(adopted);
								return [adopted, replace(all, adopted)] as const;
							}),
						),
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
					prioritize: (id, priority, subtree) =>
						SubscriptionRef.modifyEffect(entries, (all) =>
							Effect.gen(function* () {
								yield* find(all, id);
								const ids = new Set([id, ...(subtree ? descendants(all, { id }).map((entry) => entry.id) : [])]);
								const changed = all.filter((entry) => ids.has(entry.id)).map((entry): Entry => ({ ...entry, priority }));
								yield* Effect.forEach(changed, write);
								return [undefined, all.map((entry) => changed.find((other) => other.id === entry.id) ?? entry)] as const;
							}),
						),
					resolve: (id, by) =>
						SubscriptionRef.modifyEffect(entries, (all) =>
							Effect.gen(function* () {
								const entry = yield* find(all, id);
								if (entry.asker !== by) return yield* new DeskError({ message: `Entry ${id} was asked by the ${entry.asker}; only the asker marks it done.` });
								const open = openUnder(all, entry);
								if (open.length > 0) return yield* new DeskError({ message: `Entry ${id} still has ${open.length} open below it, so it can't be resolved yet: ${open.map((other) => other.id).join(", ")}.` });
								const resolved: Entry = { ...entry, status: "resolved" };
								yield* write(resolved);
								// A resolved entry without follow-ups leaves the desk; then its parent may, and so on up.
								let kept = replace(all, resolved);
								const pruned: Array<Entry> = [];
								for (let current: Entry | undefined = resolved; current?.status === "resolved" && !kept.some((other) => other.parent === current!.id); ) {
									yield* io(fs.remove(path.join(directory, fileName(current.id))));
									kept = kept.filter((other) => other.id !== current!.id);
									pruned.push(current);
									current = kept.find((other) => other.id === current!.parent);
								}
								// Attachments only the pruned entries pointed at go with them.
								const texts = (list: Entries) => list.flatMap((other) => attachmentsIn(`${other.question}\n${other.answer ?? ""}`).map(({ file }) => file));
								const still = new Set(texts(kept));
								for (const file of new Set(texts(pruned))) if (!still.has(file)) yield* io(fs.remove(path.join(root, file), { force: true }));
								return [undefined, kept] as const;
							}),
						),
					describe: (id) =>
						Effect.gen(function* () {
							const all = yield* SubscriptionRef.get(entries);
							if (id === undefined) {
								const open = ordered(all).filter((entry) => entry.status !== "resolved");
								if (open.length === 0) return "The desk is empty: nothing open or waiting.";
								return open
									.map((entry) => {
										const parent = entry.parent === null ? "" : ` (follow-up to ${entry.parent})`;
										const priority = entry.priority === "normal" ? "" : `, ${entry.priority} priority`;
										return `- ${entry.id} ${entry.title}${parent} — ${entry.status}${priority}, waiting on the ${waitingOn(entry)}, asked by the ${entry.asker}`;
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
									// Apply every draft before writing anything, so a refusal leaves the desk (and its attachments) as it was.
									let next = all;
									const uploads: Array<{ file: string; data: string }> = [];
									const sections = new Map<string, { texts: Array<string>; files: Array<{ uri: string; name: string }> }>();
									for (const sent of drafts) {
										const stored = yield* store(sent);
										const { entry, section } = yield* reply(next, stored.draft);
										next = replace(next, entry);
										uploads.push(...stored.uploads);
										const files = attachmentsIn(stored.draft.text).map(({ name, file }) => ({ uri: pathToFileURL(path.join(root, file)).href, name }));
										const before = sections.get(entry.session) ?? { texts: [], files: [] };
										sections.set(entry.session, { texts: [...before.texts, section], files: [...before.files, ...files] });
									}
									if (uploads.length > 0) {
										yield* io(fs.makeDirectory(attachments, { recursive: true }));
										yield* Effect.forEach(uploads, ({ file, data }) => io(fs.writeFile(path.join(root, file), Buffer.from(data, "base64")))).pipe(
											Effect.onError(() => Effect.forEach(uploads, ({ file }) => Effect.ignore(fs.remove(path.join(root, file), { force: true })))),
										);
									}
									yield* Effect.forEach(next.filter((entry) => !all.includes(entry)), write);
									return [sections, next] as const;
								}),
							);
							for (const [sessionID, { texts, files }] of messages) {
								yield* sessions.prompt({ sessionID, text: message(texts), delivery, files });
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
