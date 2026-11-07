/**
 * The entry store: one Markdown file per entry in `coordination/`, plus the desk's own state in `coordination/.desk/`.
 * Other files in the directory are left alone. Every write prunes what is finished and notifies subscribers;
 * edits made outside the desk arrive through the directory watcher.
 */
import { type FSWatcher, watch as fsWatch } from "node:fs";
import * as fs from "node:fs/promises";
import { dirname, join } from "node:path";
import * as Data from "effect/Data";
import * as Effect from "effect/Effect";
import * as Option from "effect/Option";
import * as Schema from "effect/Schema";
import * as Semaphore from "effect/Semaphore";
import { type Entry, fileName, isEntryFileName, normalizeLabels, type Party, parse, serialize } from "./entry";
import { prunable } from "./tree";

export class DeskError extends Data.TaggedError("DeskError")<{ readonly message: string; readonly cause?: unknown }> {}

/** An answer to an entry, or a new question (under `parent`, or at the top when null). */
export const Draft = Schema.Union([
	Schema.Struct({ kind: Schema.Literal("answer"), entry: Schema.Int, text: Schema.String }),
	Schema.Struct({ kind: Schema.Literal("ask"), parent: Schema.NullOr(Schema.Int), title: Schema.String, text: Schema.String }),
]);
export type Draft = typeof Draft.Type;

export function draftKey(draft: Draft): string {
	return draft.kind === "answer" ? `answer-${draft.entry}` : `ask-${draft.parent ?? "top"}`;
}

const State = Schema.Struct({ nextId: Schema.Int, mainSession: Schema.NullOr(Schema.String) });
type State = typeof State.Type;
const Drafts = Schema.Record(Schema.String, Draft);
type Drafts = typeof Drafts.Type;

export interface Snapshot {
	readonly entries: ReadonlyArray<Entry>;
	readonly drafts: Drafts;
	readonly mainSession: string | null;
}

export interface NewEntry {
	readonly asker: Party;
	readonly title: string;
	readonly question: string;
	readonly labels?: ReadonlyArray<string>;
	readonly parent?: number | null;
}

export interface Store {
	readonly directory: string;
	readonly snapshot: () => Snapshot;
	/** Called with every changed snapshot until the returned function is called. */
	readonly subscribe: (listener: (snapshot: Snapshot) => void) => () => void;
	readonly get: (id: number) => Effect.Effect<Entry, DeskError>;
	readonly create: (entry: NewEntry) => Effect.Effect<Entry, DeskError>;
	readonly update: (id: number, change: (entry: Entry) => Entry | DeskError) => Effect.Effect<Entry, DeskError>;
	readonly setMainSession: (sessionID: string | null) => Effect.Effect<void, DeskError>;
	readonly putDraft: (draft: Draft) => Effect.Effect<void, DeskError>;
	readonly removeDrafts: (keys: ReadonlyArray<string>) => Effect.Effect<void, DeskError>;
	/** Re-read the directory; the watcher does this after outside edits. */
	readonly reload: Effect.Effect<void, DeskError>;
}

/** Opens the store in `directory` (created when missing) and watches it until the scope closes. */
export const make = Effect.fn("Desk.Store.make")(function* (directory: string) {
	const stateDir = join(directory, ".desk");
	const statePath = join(stateDir, "state.json");
	const draftsPath = join(stateDir, "drafts.json");
	yield* attempt("create the desk directory", () => fs.mkdir(stateDir, { recursive: true }));

	const lock = yield* Semaphore.make(1);
	const files = new Map<number, string>();
	let current: Snapshot = { entries: [], drafts: {}, mainSession: null };
	let published = "";
	const listeners = new Set<(snapshot: Snapshot) => void>();

	const readState = attempt("read the desk state", async (): Promise<State> => {
		const text = await fs.readFile(statePath, "utf8").catch(() => "");
		return Option.getOrElse(decodeJson(State)(text), () => ({ nextId: 1, mainSession: null }));
	});
	const writeState = (state: State) => writeAtomic(statePath, `${JSON.stringify(state, null, "\t")}\n`);
	const readDrafts = attempt("read the drafts", async (): Promise<Drafts> => {
		const text = await fs.readFile(draftsPath, "utf8").catch(() => "");
		return Option.getOrElse(decodeJson(Drafts)(text), () => ({}));
	});
	const writeDrafts = (drafts: Drafts) => writeAtomic(draftsPath, `${JSON.stringify(drafts, null, "\t")}\n`);

	/**
	 * Outside edits arrive through fs.watch, coalesced into one reload per burst. The directory may be deleted and
	 * recreated under the store, so every load checks that the watcher still follows the directory that exists.
	 */
	const watch = (() => {
		let watcher: FSWatcher | undefined;
		let watched: bigint | undefined;
		let timer: ReturnType<typeof setTimeout> | undefined;
		const close = () => {
			clearTimeout(timer);
			watcher?.close();
			watcher = undefined;
		};
		const ensure = attempt("watch the desk directory", async () => {
			await fs.mkdir(stateDir, { recursive: true });
			const { ino } = await fs.stat(directory, { bigint: true });
			if (watcher && watched === ino) return;
			close();
			watched = ino;
			watcher = fsWatch(directory, { recursive: true }, () => {
				clearTimeout(timer);
				timer = setTimeout(() => void Effect.runFork(Effect.ignore(store.reload)), 75);
			});
			watcher.on("error", close);
		});
		return { ensure, close };
	})();

	const load = Effect.gen(function* () {
		yield* watch.ensure;
		const names = yield* attempt("list the desk directory", () => fs.readdir(directory));
		files.clear();
		const entries: Array<Entry> = [];
		for (const name of names.filter(isEntryFileName).sort()) {
			const text = yield* attempt(`read ${name}`, () => fs.readFile(join(directory, name), "utf8")).pipe(
				Effect.orElseSucceed(() => ""),
			);
			const entry = parse(text);
			// A duplicate id keeps the first file; the other stays on disk untouched.
			if (Option.isSome(entry) && !files.has(entry.value.id)) {
				files.set(entry.value.id, name);
				entries.push(entry.value);
			}
		}
		const state = yield* readState;
		const drafts = yield* readDrafts;
		current = { entries: entries.sort((a, b) => a.id - b.id), drafts, mainSession: state.mainSession };
	});

	const prune = Effect.gen(function* () {
		const ids = prunable(current.entries);
		if (ids.length === 0) return false;
		for (const id of ids) {
			const name = files.get(id);
			if (name) yield* attempt(`delete ${name}`, () => fs.rm(join(directory, name), { force: true }));
		}
		const gone = new Set(ids);
		const drafts = Object.fromEntries(
			Object.entries(current.drafts).filter(([, draft]) => !gone.has(draft.kind === "answer" ? draft.entry : (draft.parent ?? NaN))),
		);
		if (Object.keys(drafts).length !== Object.keys(current.drafts).length) yield* writeDrafts(drafts);
		return true;
	});

	const publish = Effect.sync(() => {
		const text = JSON.stringify(current);
		if (text === published) return;
		published = text;
		for (const listener of listeners) listener(current);
	});

	/** Load, prune, load again if anything went, then notify: the one way the snapshot changes. */
	const refresh = Effect.gen(function* () {
		yield* load;
		if (yield* prune) yield* load;
		yield* publish;
	});

	const locked = <A>(effect: Effect.Effect<A, DeskError>) => lock.withPermits(1)(Effect.tap(effect, () => refresh));

	const find = (id: number) =>
		Effect.gen(function* () {
			yield* load;
			const entry = current.entries.find((entry) => entry.id === id);
			if (!entry) return yield* new DeskError({ message: `No desk entry #${id}. It may have been resolved and pruned.` });
			return entry;
		});

	const write = (entry: Entry) =>
		Effect.gen(function* () {
			const name = files.get(entry.id) ?? fileName(entry);
			yield* writeAtomic(join(directory, name), serialize(entry));
			files.set(entry.id, name);
		});

	const store: Store = {
		directory,
		snapshot: () => current,
		subscribe(listener) {
			listeners.add(listener);
			return () => listeners.delete(listener);
		},
		get: (id) => lock.withPermits(1)(find(id)),
		create: (input) =>
			locked(
				Effect.gen(function* () {
					yield* load;
					const title = input.title.replace(/\s+/g, " ").trim();
					if (!title) return yield* new DeskError({ message: "An entry needs a title." });
					const parent = input.parent ?? null;
					if (parent !== null && !current.entries.some((entry) => entry.id === parent)) {
						return yield* new DeskError({ message: `No desk entry #${parent} to follow up on.` });
					}
					const state = yield* readState;
					const id = Math.max(state.nextId, ...current.entries.map((entry) => entry.id + 1));
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
					yield* write(entry);
					return entry;
				}),
			),
		update: (id, change) =>
			locked(
				Effect.gen(function* () {
					const next = change(yield* find(id));
					if (next instanceof DeskError) return yield* next;
					yield* write(next);
					return next;
				}),
			),
		setMainSession: (mainSession) =>
			locked(Effect.flatMap(readState, (state) => writeState({ ...state, mainSession }))),
		putDraft: (draft) =>
			locked(
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
			locked(
				Effect.gen(function* () {
					const drafts = { ...(yield* readDrafts) };
					for (const key of keys) delete drafts[key];
					yield* writeDrafts(drafts);
				}),
			),
		reload: lock.withPermits(1)(refresh),
	};

	yield* Effect.addFinalizer(() => Effect.sync(() => {
		watch.close();
		listeners.clear();
	}));
	yield* store.reload;
	return store;
});

function attempt<A>(what: string, run: () => Promise<A>): Effect.Effect<A, DeskError> {
	return Effect.tryPromise({ try: run, catch: (cause) => new DeskError({ message: `Could not ${what}: ${String(cause)}`, cause }) });
}

/** Write through a temporary file so the watcher (and other readers) never see half an entry. */
function writeAtomic(path: string, text: string): Effect.Effect<void, DeskError> {
	return attempt(`write ${path}`, async () => {
		const temporary = `${path}.${process.pid}.tmp`;
		// The directory may have been removed by hand since the store opened.
		await fs.mkdir(dirname(path), { recursive: true });
		await fs.writeFile(temporary, text);
		await fs.rename(temporary, path);
	});
}

function decodeJson<S extends Schema.Top>(schema: S) {
	const decode = Schema.decodeUnknownOption(schema as never) as (input: unknown) => Option.Option<S["Type"]>;
	return (text: string): Option.Option<S["Type"]> => {
		try {
			return decode(JSON.parse(text));
		} catch {
			return Option.none();
		}
	};
}
