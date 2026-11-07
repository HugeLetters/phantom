/**
 * An entry: one question and its answer, stored as `coordination/NNNN.md`; the number is the id.
 *
 * ```md
 * ---
 * parent: 3
 * asker: agent
 * status: open
 * session: ses_…
 * labels: [design, icons]
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
import * as Option from "effect/Option";
import * as Schema from "effect/Schema";

export const Party = Schema.Literals(["user", "agent"]);
export type Party = typeof Party.Type;

/** open: waiting on the answerer. answered: waiting on the asker. resolved: the asker marked it done. */
export const Status = Schema.Literals(["open", "answered", "resolved"]);

const Header = Schema.Struct({
	/** Always a smaller id: follow-ups come after what they follow up on. */
	parent: Schema.NullOr(Schema.Int),
	asker: Party,
	status: Status,
	/** The top-level OpenCode session that asked, or that the user's reply belongs to; it hears about the entry. */
	session: Schema.String,
	labels: Schema.Array(Schema.String),
	created: Schema.String,
});

export const Entry = Schema.Struct({
	...Header.fields,
	id: Schema.Int,
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

export const fileName = (id: number) => `${String(id).padStart(4, "0")}.md`;

/** The id an entry file name stands for; other files in `coordination/` aren't entries. */
export function idOf(name: string): number | undefined {
	const match = /^(\d{4,})\.md$/.exec(name);
	return match ? Number(match[1]) : undefined;
}

export function serialize(entry: Entry): string {
	const header = [
		`parent: ${entry.parent ?? ""}`.trimEnd(),
		`asker: ${entry.asker}`,
		`status: ${entry.status}`,
		`session: ${entry.session}`,
		`labels: [${entry.labels.join(", ")}]`,
		`created: "${entry.created}"`,
	];
	const answer = entry.answer === null ? [] : [answerMarker, entry.answer];
	return `${["---", ...header, "---"].join("\n")}\n\n${[`# ${entry.title}`, entry.question, ...answer].filter(Boolean).join("\n\n")}\n`;
}

export function parse(id: number, text: string): Option.Option<Entry> {
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
