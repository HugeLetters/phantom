/**
 * The desk's operations for both sides: the agents (through tools, as the main session) and the user (through the page).
 * OpenCode is reached only through `Sessions`, so the logic runs in tests without a server.
 */
import * as Effect from "effect/Effect";
import { type Entry, waitingOn } from "./entry";
import { DeskError, type Draft, draftKey, type Store } from "./store";
import { build, type Node, thread, waitingCount } from "./tree";

export type Delivery = "queue" | "steer";

export interface SessionInfo {
	readonly id: string;
	readonly title: string;
}

export interface Sessions {
	/** The top-level session above `sessionID` (itself when it has no parent). */
	readonly root: (sessionID: string) => Effect.Effect<string, DeskError>;
	readonly get: (sessionID: string) => Effect.Effect<SessionInfo, DeskError>;
	/** Recent top-level sessions in this checkout, newest first. */
	readonly list: () => Effect.Effect<ReadonlyArray<SessionInfo>, DeskError>;
	readonly prompt: (sessionID: string, text: string, delivery: Delivery) => Effect.Effect<void, DeskError>;
}

export interface Options {
	/** A fixed main session; otherwise the first top-level session to use a desk tool becomes it. */
	readonly mainSession?: string;
}

export function make(store: Store, sessions: Sessions, options: Options = {}) {
	const mainSession = () => options.mainSession ?? store.snapshot().mainSession;

	/** Every desk tool call goes through here; the first caller's top-level session becomes the main session. */
	const claim = Effect.fn("Desk.claim")(function* (sessionID: string) {
		const current = mainSession();
		if (current) return current;
		const root = yield* sessions.root(sessionID);
		yield* store.setMainSession(root);
		return root;
	});

	const requireMain = Effect.gen(function* () {
		const sessionID = mainSession();
		if (!sessionID) {
			return yield* new DeskError({
				message: "No main session yet. It becomes known when a session first uses a desk tool, or pick one on the page.",
			});
		}
		return sessionID;
	});

	// ─── Agent side ─────────────────────────────────────────────────────────────

	const ask = (input: { title: string; body: string; labels?: ReadonlyArray<string>; parent?: number }) =>
		store.create({ asker: "main", title: input.title, question: input.body, labels: input.labels, parent: input.parent });

	const answer = (id: number, body: string) =>
		store.update(id, (entry) => {
			if (entry.answerer !== "main") return new DeskError({ message: `#${id} is the agents' own question; the user answers it.` });
			if (entry.status === "resolved") return new DeskError({ message: `#${id} is already resolved.` });
			return { ...entry, answer: body.trim(), status: "answered" };
		});

	const resolveAs = (party: Entry["asker"]) => (id: number) =>
		store.update(id, (entry) => {
			if (entry.asker !== party) {
				return new DeskError({ message: `#${id} was asked by the ${party === "main" ? "user" : "agents"}; only the asker marks it done.` });
			}
			return { ...entry, status: "resolved" };
		});

	/** Unresolved entries as an indented outline, or one entry's thread in full. */
	const describe = Effect.fn("Desk.describe")(function* (id?: number) {
		yield* store.reload;
		const entries = store.snapshot().entries;
		if (id !== undefined) {
			const path = thread(entries, id);
			if (path.length === 0) return yield* new DeskError({ message: `No desk entry #${id}.` });
			const below = entries.filter((entry) => entry.parent === id);
			return [
				...path.map(render),
				...(below.length ? [`Follow-ups: ${below.map((entry) => `#${entry.id} ${entry.title} (${entry.status})`).join("; ")}`] : []),
			].join("\n\n---\n\n");
		}
		const lines: Array<string> = [];
		const walk = (nodes: ReadonlyArray<Node>, depth: number) => {
			for (const node of nodes) {
				if (!node.active) continue;
				const { entry } = node;
				const turn = waitingOn(entry);
				const labels = entry.labels.length ? ` [${entry.labels.join(", ")}]` : "";
				lines.push(`${"  ".repeat(depth)}- #${entry.id} ${entry.title} — ${entry.status}${turn ? `, waiting on ${turn === "main" ? "agents" : "user"}` : ""} (asked by ${entry.asker === "main" ? "agents" : "user"})${labels}`);
				walk(node.children, depth + 1);
			}
		};
		walk(build(entries), 0);
		if (lines.length === 0) return "The desk is empty: nothing open or waiting.";
		return `${lines.join("\n")}\n\nWaiting on the user: ${waitingCount(entries, "user")}. Waiting on the agents: ${waitingCount(entries, "main")}.`;
	});

	// ─── User side ──────────────────────────────────────────────────────────────

	/**
	 * Applies the drafts (answers land in their entries, new questions become entries) and sends one message with all
	 * of them to the main session. Drafts that fail to apply stay as drafts.
	 */
	const deliver = Effect.fn("Desk.deliver")(function* (keys: ReadonlyArray<string> | "all", delivery: Delivery) {
		const sessionID = yield* requireMain;
		yield* sessions.get(sessionID);
		const drafts = Object.entries(store.snapshot().drafts).filter(([key]) => keys === "all" || keys.includes(key));
		if (drafts.length === 0) return yield* new DeskError({ message: "Nothing to send: there are no drafts." });
		const sections: Array<string> = [];
		const done: Array<string> = [];
		const failures: Array<string> = [];
		for (const [key, draft] of drafts) {
			const applied = yield* apply(draft).pipe(Effect.result);
			if (applied._tag === "Failure") {
				failures.push(applied.failure.message);
				continue;
			}
			sections.push(applied.success);
			done.push(key);
		}
		yield* store.removeDrafts(done);
		if (sections.length > 0) yield* sessions.prompt(sessionID, message(sections), delivery);
		if (failures.length > 0) return yield* new DeskError({ message: failures.join("\n") });
		return sections.length;
	});

	const apply = (draft: Draft): Effect.Effect<string, DeskError> => {
		if (draft.kind === "ask") {
			return Effect.map(
				store.create({ asker: "user", title: draft.title || firstLine(draft.text), question: draft.text, parent: draft.parent }),
				(entry) =>
					[
						`## #${entry.id} New question from the user: ${entry.title}${entry.parent !== null ? ` (follow-up to #${entry.parent})` : ""}`,
						entry.question || "(no details)",
					].join("\n\n"),
			);
		}
		return Effect.map(
			store.update(draft.entry, (entry) => {
				if (entry.answerer !== "user") return new DeskError({ message: `#${entry.id} is the user's own question; the agents answer it.` });
				if (entry.status === "resolved") return new DeskError({ message: `#${entry.id} is already resolved.` });
				return { ...entry, answer: draft.text.trim(), status: "answered" };
			}),
			(entry) => [`## #${entry.id} Answered: ${entry.title}`, entry.answer ?? ""].join("\n\n"),
		);
	};

	const selectMain = Effect.fn("Desk.selectMain")(function* (sessionID: string) {
		const root = yield* sessions.root(sessionID);
		yield* store.setMainSession(root);
	});

	return {
		store,
		sessions,
		mainSession,
		claim,
		ask,
		answer,
		resolve: resolveAs("main"),
		describe,
		user: {
			putDraft: store.putDraft,
			submit: () => deliver("all", "queue"),
			send: (draft: Draft) => deliver([draftKey(draft)], "steer"),
			sendKey: (key: string) => deliver([key], "steer"),
			resolve: resolveAs("user"),
			selectMain,
		},
	};
}

export type Desk = ReturnType<typeof make>;

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
