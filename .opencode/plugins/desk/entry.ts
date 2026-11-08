/**
 * An entry: one question and its answer, stored as `coordination/<id>.md`. Ids are random UUIDs, never reused and
 * meaning nothing beyond identity; entries are ordered by `created`.
 *
 * ```md
 * ---
 * parent: 3f2b8c1e-…
 * asker: agent
 * status: open
 * session: ses_…
 * labels: [design, icons]
 * priority: high   # optional: high | normal | low; absent means normal
 * created: 2026-11-07T16:30:00.000Z
 * ---
 *
 * # Which icon?
 *
 * The question, in Markdown.
 *
 * <!-- desk:answer -->
 *
 * The answer, in Markdown.
 * ```
 */
import * as Effect from "effect/Effect";
import * as Option from "effect/Option";
import * as Schema from "effect/Schema";

/** A random UUID. */
export const Id = Schema.String;

export const Party = Schema.Literals(["user", "agent"]);
export type Party = typeof Party.Type;

/** What the agents set to bump an entry up (or down) the queue; follow-ups start with their parent's. */
export const Priority = Schema.Literals(["high", "normal", "low"]);
export type Priority = typeof Priority.Type;
const rank = { high: 0, normal: 1, low: 2 } as const;

/** The desk's order: priority first, then `created`. */
export const compare = (a: Pick<Entry, "priority" | "created">, b: Pick<Entry, "priority" | "created">) =>
	rank[a.priority] - rank[b.priority] || (a.created < b.created ? -1 : a.created > b.created ? 1 : 0);
export const ordered = <E extends Pick<Entry, "priority" | "created">>(entries: ReadonlyArray<E>): Array<E> => [...entries].sort(compare);

/** open: waiting on the answerer. answered: waiting on the asker. resolved: the asker marked it done. */
export const Status = Schema.Literals(["open", "answered", "resolved"]);

const Header = Schema.Struct({
	/** Set when the entry is created, to an entry that exists then, and never changed: no cycles, no orphans. */
	parent: Schema.NullOr(Id),
	asker: Party,
	status: Status,
	/** The top-level OpenCode session that asked, or that the user's reply belongs to; it hears about the entry. */
	session: Schema.String,
	labels: Schema.Array(Schema.String),
	priority: Priority.pipe(Schema.withDecodingDefaultKey(Effect.succeed("normal" as const))),
	created: Schema.String,
});

export const Entry = Schema.Struct({
	...Header.fields,
	id: Id,
	title: Schema.String,
	question: Schema.String,
	answer: Schema.NullOr(Schema.String),
});
export type Entry = typeof Entry.Type;

const decodeHeader = Schema.decodeUnknownOption(Header);
const answerMarker = "<!-- desk:answer -->";

export const answerer = (entry: Pick<Entry, "asker">): Party => (entry.asker === "user" ? "agent" : "user");

/** Whose move it is: the answerer while open, the asker while answered, nobody once resolved. */
export function waitingOn(entry: Pick<Entry, "status" | "asker">): Party | null {
	if (entry.status === "open") return answerer(entry);
	return entry.status === "answered" ? entry.asker : null;
}

/**
 * Next on the page: the first entry waiting on the user after `current` in the desk's order (priority, then
 * `created`), wrapping round; undefined when nothing else waits on them (the page then goes back to the list).
 */
export function next(entries: ReadonlyArray<Entry>, current: Pick<Entry, "id" | "created" | "priority">): Entry | undefined {
	const waiting = ordered(entries.filter((entry) => waitingOn(entry) === "user" && entry.id !== current.id));
	return waiting.find((entry) => compare(entry, current) > 0) ?? waiting[0];
}

/**
 * Entries deeper than the direct children of `root` (grandchildren and below) that wait on the user, in the desk's order:
 * the unanswered questions a PR review's direct follow-ups don't show.
 */
export function openBelow(entries: ReadonlyArray<Entry>, root: Pick<Entry, "id">): ReadonlyArray<Entry> {
	const below = (ids: ReadonlySet<string>, depth: number): Array<Entry> => {
		const children = entries.filter((entry) => entry.parent !== null && ids.has(entry.parent));
		return children.length === 0 ? [] : [...(depth >= 2 ? children : []), ...below(new Set(children.map((child) => child.id)), depth + 1)];
	};
	return ordered(below(new Set([root.id]), 1).filter((entry) => waitingOn(entry) === "user"));
}

export const newId = () => crypto.randomUUID();

/** The last six hex digits: enough to tell entries apart where the full id is too long to show. */
export const shortId = (id: string) => id.slice(-6);

export const fileName = (id: string) => `${id}.md`;

/** The id an entry file name stands for; other files in `coordination/` aren't entries. */
export function idOf(name: string): string | undefined {
	return /^([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.md$/.exec(name)?.[1];
}

export function serialize(entry: Entry): string {
	const header = [
		`parent: ${entry.parent ?? ""}`.trimEnd(),
		`asker: ${entry.asker}`,
		`status: ${entry.status}`,
		`session: ${entry.session}`,
		`labels: [${entry.labels.join(", ")}]`,
		...(entry.priority === "normal" ? [] : [`priority: ${entry.priority}`]),
		`created: "${entry.created}"`,
	];
	const answer = entry.answer === null ? [] : [answerMarker, entry.answer];
	return `${["---", ...header, "---"].join("\n")}\n\n${[`# ${entry.title}`, entry.question, ...answer].filter(Boolean).join("\n\n")}\n`;
}

export function parse(id: string, text: string): Option.Option<Entry> {
	const match = /^---\n([\s\S]*?)\n---\n+# (.*)\n?([\s\S]*)$/.exec(text);
	if (!match) return Option.none();
	const [, head, title, body] = match as unknown as [string, string, string, string];
	const [question, answer] = body.split(answerMarker);
	return Option.map(decodeHeader(Bun.YAML.parse(head)), (header) => ({
		...header,
		id,
		title: title.trim(),
		question: question!.trim(),
		answer: answer === undefined ? null : answer.trim(),
	}));
}
