/**
 * An entry: one question and its answer, stored as one Markdown file.
 *
 * ```md
 * ---
 * id: 7
 * parent: 3
 * asker: main
 * answerer: user
 * status: open
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

/** The two sides of the desk: the user, and the main session that speaks for the agents. */
export const Party = Schema.Literals(["user", "main"]);
export type Party = typeof Party.Type;

/**
 * open: waiting on the answerer. answered: waiting on the asker to read, follow up or mark done.
 * resolved: the asker marked it done and the outcome is applied in the repo.
 */
export const Status = Schema.Literals(["open", "answered", "resolved"]);
export type Status = typeof Status.Type;

export const Entry = Schema.Struct({
	id: Schema.Int,
	parent: Schema.NullOr(Schema.Int),
	asker: Party,
	answerer: Party,
	status: Status,
	labels: Schema.Array(Schema.String),
	created: Schema.String,
	title: Schema.String,
	question: Schema.String,
	answer: Schema.NullOr(Schema.String),
});
export type Entry = typeof Entry.Type;

const answerMarker = "<!-- desk:answer -->";
const decodeEntry = Schema.decodeUnknownOption(Entry);

export function other(party: Party): Party {
	return party === "user" ? "main" : "user";
}

/** Whose move it is: the answerer while open, the asker while answered, nobody once resolved. */
export function waitingOn(entry: Pick<Entry, "status" | "asker" | "answerer">): Party | null {
	if (entry.status === "open") return entry.answerer;
	if (entry.status === "answered") return entry.asker;
	return null;
}

export function fileName(entry: Pick<Entry, "id" | "title">): string {
	const slug = entry.title
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-+|-+$/g, "")
		.slice(0, 48)
		.replace(/-+$/, "");
	return `${String(entry.id).padStart(4, "0")}${slug ? `-${slug}` : ""}.md`;
}

/** Entry files are `NNNN-slug.md`; anything else in the directory is not an entry. */
export function isEntryFileName(name: string): boolean {
	return /^\d{4,}(-[a-z0-9-]*)?\.md$/.test(name);
}

export function serialize(entry: Entry): string {
	const header = [
		"---",
		`id: ${entry.id}`,
		`parent: ${entry.parent ?? ""}`.trimEnd(),
		`asker: ${entry.asker}`,
		`answerer: ${entry.answerer}`,
		`status: ${entry.status}`,
		`labels: [${entry.labels.join(", ")}]`,
		`created: ${entry.created}`,
		"---",
	];
	const parts = [header.join("\n"), `# ${oneLine(entry.title)}`, entry.question.trim()];
	if (entry.answer !== null) parts.push(answerMarker, entry.answer.trim());
	return `${parts.filter((part) => part !== "").join("\n\n")}\n`;
}

/** `None` for files that are not well-formed entries, so stray notes in the directory are ignored. */
export function parse(text: string): Option.Option<Entry> {
	const match = /^---\n([\s\S]*?)\n---\n?([\s\S]*)$/.exec(text.replace(/\r\n/g, "\n"));
	if (!match) return Option.none();
	const [, head = "", rest = ""] = match;
	const fields = new Map<string, string>();
	for (const line of head.split("\n")) {
		const field = /^(\w+):\s*(.*)$/.exec(line);
		if (field) fields.set(field[1]!, field[2]!.trim());
	}
	const body = rest.replace(/^\s+/, "");
	const title = /^# (.*)(?:\n|$)/.exec(body);
	if (!title) return Option.none();
	const afterTitle = body.slice(title[0].length);
	const split = afterTitle.indexOf(answerMarker);
	const question = (split < 0 ? afterTitle : afterTitle.slice(0, split)).trim();
	const answer = split < 0 ? null : afterTitle.slice(split + answerMarker.length).trim();
	const parent = fields.get("parent") ?? "";
	return decodeEntry({
		id: Number(fields.get("id")),
		parent: parent === "" ? null : Number(parent),
		asker: fields.get("asker"),
		answerer: fields.get("answerer"),
		status: fields.get("status"),
		labels: parseLabels(fields.get("labels") ?? ""),
		created: fields.get("created") ?? "",
		title: title[1]!.trim(),
		question,
		answer,
	});
}

export function normalizeLabels(labels: ReadonlyArray<string>): Array<string> {
	return [...new Set(labels.map((label) => label.trim().toLowerCase().replace(/[\s,[\]]+/g, "-")).filter(Boolean))];
}

function parseLabels(value: string): Array<string> {
	const inner = value.replace(/^\[|\]$/g, "");
	return normalizeLabels(inner.split(","));
}

function oneLine(text: string): string {
	return text.replace(/\s+/g, " ").trim();
}
