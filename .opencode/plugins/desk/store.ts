/**
 * The entry store: one Markdown file per entry in `coordination/`, plus the desk's own state in
 * `coordination/.desk/` (next id, main session, drafts). Other files in the directory are left alone.
 *
 * The current contents are a `SubscriptionRef`: every write, and every `reload` (the watcher calls it after outside
 * edits), publishes a new snapshot to `changes`.
 */
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import * as Semaphore from "effect/Semaphore";
import * as Stream from "effect/Stream";
import * as SubscriptionRef from "effect/SubscriptionRef";
import { DeskConfig, DeskError } from "./config";
import { type Entry, fileName, isEntryFileName, normalizeLabels, type Party, parse, serialize } from "./entry";

/** An answer to an entry, or a new question (under `parent`, or at the top when null). */
export const Draft = Schema.Union([
	Schema.Struct({ kind: Schema.Literal("answer"), entry: Schema.Int, text: Schema.String }),
	Schema.Struct({ kind: Schema.Literal("ask"), parent: Schema.NullOr(Schema.Int), title: Schema.String, text: Schema.String }),
]);
export type Draft = typeof Draft.Type;

export function draftKey(draft: Draft): string {
	return draft.kind === "answer" ? `answer-${draft.entry}` : `ask-${draft.parent ?? "top"}`;
}

/** The entry a draft hangs on, if any. */
function draftEntry(draft: Draft): number | null {
	return draft.kind === "answer" ? draft.entry : draft.parent;
}

const State = Schema.Struct({ nextId: Schema.Int, mainSession: Schema.NullOr(Schema.String) });
type State = typeof State.Type;
const StateJson = Schema.fromJsonString(State);
const Drafts = Schema.Record(Schema.String, Draft);
type Drafts = typeof Drafts.Type;
const DraftsJson = Schema.fromJsonString(Drafts);

export interface Snapshot {
	readonly entries: ReadonlyArray<Entry>;
	readonly drafts: Drafts;
	readonly mainSession: string | null;
}

export interface NewEntry {
	readonly asker: Party;
	readonly title: string;
	readonly question: string;
	readonly labels?: ReadonlyArray<string> | undefined;
	readonly parent?: number | null | undefined;
}

export class Store extends Context.Service<
	Store,
	{
		readonly directory: string;
		readonly snapshot: Effect.Effect<Snapshot>;
		/** The current snapshot, then every change. */
		readonly changes: Stream.Stream<Snapshot>;
		/** Re-read the directory, after edits made outside the desk. */
		readonly reload: Effect.Effect<void, DeskError>;
		readonly get: (id: number) => Effect.Effect<Entry, DeskError>;
		readonly create: (entry: NewEntry) => Effect.Effect<Entry, DeskError>;
		/** `change` returns the new entry, or a `DeskError` to refuse. */
		readonly update: (id: number, change: (entry: Entry) => Entry | DeskError) => Effect.Effect<Entry, DeskError>;
		/** Deletes entries and the drafts that hang on them. */
		readonly remove: (ids: ReadonlyArray<number>) => Effect.Effect<void, DeskError>;
		readonly setMainSession: (sessionID: string | null) => Effect.Effect<void, DeskError>;
		readonly putDraft: (draft: Draft) => Effect.Effect<void, DeskError>;
		readonly removeDrafts: (keys: ReadonlyArray<string>) => Effect.Effect<void, DeskError>;
	}
>()("desk/Store") {
	static readonly layer = Layer.effect(
		this,
		Effect.gen(function* () {
			const fs = yield* FileSystem.FileSystem;
			const path = yield* Path.Path;
			const { directory } = yield* DeskConfig;
			const stateDir = path.join(directory, ".desk");
			const statePath = path.join(stateDir, "state.json");
			const draftsPath = path.join(stateDir, "drafts.json");

			const failed = (what: string) => (cause: { readonly message: string }) =>
				new DeskError({ message: `Could not ${what}: ${cause.message}`, cause });

			/** The directory may have been deleted since the last write; every write recreates it. */
			const ensureDirectory = fs.makeDirectory(stateDir, { recursive: true }).pipe(Effect.mapError(failed("create the desk directory")));

			/** Write through a temporary file so readers (and the watcher) never see half a file. */
			const writeAtomic = (target: string, text: string) =>
				Effect.gen(function* () {
					yield* ensureDirectory;
					const temporary = `${target}.${process.pid}.tmp`;
					yield* fs.writeFileString(temporary, text);
					yield* fs.rename(temporary, target);
				}).pipe(Effect.mapError((error) => (error instanceof DeskError ? error : failed(`write ${target}`)(error))));

			const readJson = <A>(file: string, schema: Schema.Codec<A, string>, fallback: A) =>
				fs.readFileString(file).pipe(
					Effect.flatMap((text) => Schema.decodeUnknownEffect(schema)(text)),
					Effect.orElseSucceed(() => fallback),
				);
			const readState = readJson(statePath, StateJson, { nextId: 1, mainSession: null });
			const readDrafts = readJson(draftsPath, DraftsJson, {});
			const writeState = (state: State) => writeAtomic(statePath, `${JSON.stringify(state, null, "\t")}\n`);
			const writeDrafts = (drafts: Drafts) => writeAtomic(draftsPath, `${JSON.stringify(drafts, null, "\t")}\n`);

			/** Entry files by id. A duplicate id keeps the first file; the other stays on disk untouched. */
			const readEntries = Effect.gen(function* () {
				yield* ensureDirectory;
				const names = yield* fs.readDirectory(directory).pipe(Effect.mapError(failed("list the desk directory")));
				const files = new Map<number, { readonly name: string; readonly entry: Entry }>();
				for (const name of names.filter(isEntryFileName).sort()) {
					const text = yield* fs.readFileString(path.join(directory, name)).pipe(Effect.orElseSucceed(() => ""));
					const entry = parse(text);
					if (Option.isSome(entry) && !files.has(entry.value.id)) files.set(entry.value.id, { name, entry: entry.value });
				}
				return files;
			});

			const read = Effect.gen(function* () {
				const files = yield* readEntries;
				const state = yield* readState;
				const drafts = yield* readDrafts;
				const snapshot: Snapshot = {
					entries: [...files.values()].map((file) => file.entry).sort((a, b) => a.id - b.id),
					drafts,
					mainSession: state.mainSession,
				};
				return { files, snapshot };
			});

			const ref = yield* SubscriptionRef.make<Snapshot>({ entries: [], drafts: {}, mainSession: null });
			const lock = yield* Semaphore.make(1);

			/** Publish what is on disk; unchanged contents are not republished. */
			const refresh = Effect.gen(function* () {
				const { snapshot } = yield* read;
				const current = yield* SubscriptionRef.get(ref);
				if (JSON.stringify(current) !== JSON.stringify(snapshot)) yield* SubscriptionRef.set(ref, snapshot);
			});

			/**
			 * Every read-modify-write runs alone: the agents' tool calls and the page's requests arrive concurrently, and
			 * without the lock two could take the same id or drop each other's drafts.
			 */
			const exclusive = <A>(effect: Effect.Effect<A, DeskError>) =>
				lock.withPermits(1)(Effect.tap(effect, () => refresh));

			const find = (files: Map<number, { readonly name: string; readonly entry: Entry }>, id: number) =>
				Effect.gen(function* () {
					const file = files.get(id);
					if (!file) return yield* new DeskError({ message: `No desk entry #${id}. It may have been resolved and pruned.` });
					return file;
				});

			yield* lock.withPermits(1)(refresh);

			return Store.of({
				directory,
				snapshot: SubscriptionRef.get(ref),
				changes: SubscriptionRef.changes(ref),
				reload: lock.withPermits(1)(refresh),
				get: (id) =>
					lock.withPermits(1)(
						readEntries.pipe(
							Effect.flatMap((files) => find(files, id)),
							Effect.map((file) => file.entry),
						),
					),
				create: (input) =>
					exclusive(
						Effect.gen(function* () {
							const files = yield* readEntries;
							const title = input.title.replace(/\s+/g, " ").trim();
							if (!title) return yield* new DeskError({ message: "An entry needs a title." });
							const parent = input.parent ?? null;
							if (parent !== null && !files.has(parent)) {
								return yield* new DeskError({ message: `No desk entry #${parent} to follow up on.` });
							}
							const state = yield* readState;
							const id = Math.max(state.nextId, ...[...files.keys()].map((existing) => existing + 1));
							const entry: Entry = {
								id,
								parent,
								asker: input.asker,
								answerer: input.asker === "user" ? "main" : "user",
								status: "open",
								labels: normalizeLabels(input.labels ?? []),
								created: new Date().toISOString(),
								title,
								question: input.question.trim(),
								answer: null,
							};
							yield* writeState({ ...state, nextId: id + 1 });
							yield* writeAtomic(path.join(directory, fileName(entry)), serialize(entry));
							return entry;
						}),
					),
				update: (id, change) =>
					exclusive(
						Effect.gen(function* () {
							const file = yield* find(yield* readEntries, id);
							const next = change(file.entry);
							if (next instanceof DeskError) return yield* next;
							yield* writeAtomic(path.join(directory, file.name), serialize(next));
							return next;
						}),
					),
				remove: (ids) =>
					exclusive(
						Effect.gen(function* () {
							const files = yield* readEntries;
							for (const id of ids) {
								const file = files.get(id);
								if (!file) continue;
								yield* fs.remove(path.join(directory, file.name), { force: true }).pipe(Effect.mapError(failed(`delete ${file.name}`)));
							}
							const gone = new Set(ids);
							const drafts = yield* readDrafts;
							const kept = Object.fromEntries(Object.entries(drafts).filter(([, draft]) => !gone.has(draftEntry(draft) ?? NaN)));
							if (Object.keys(kept).length !== Object.keys(drafts).length) yield* writeDrafts(kept);
						}),
					),
				setMainSession: (mainSession) => exclusive(Effect.flatMap(readState, (state) => writeState({ ...state, mainSession }))),
				putDraft: (draft) =>
					exclusive(
						Effect.gen(function* () {
							const drafts = { ...(yield* readDrafts) };
							const key = draftKey(draft);
							const empty = draft.text.trim() === "" && (draft.kind === "answer" || draft.title.trim() === "");
							if (empty) delete drafts[key];
							else drafts[key] = draft;
							yield* writeDrafts(drafts);
						}),
					),
				removeDrafts: (keys) =>
					exclusive(
						Effect.gen(function* () {
							const drafts = { ...(yield* readDrafts) };
							for (const key of keys) delete drafts[key];
							yield* writeDrafts(drafts);
						}),
					),
			});
		}),
	);
}
