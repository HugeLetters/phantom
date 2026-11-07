/** The entry tree: follow-ups hang under their parent. Pure, so the store and the page share one view of it. */
import { type Entry, type Party, waitingOn } from "./entry";

export interface Node {
	readonly entry: Entry;
	readonly children: ReadonlyArray<Node>;
	/** This entry or one below it waits on the user. */
	readonly waitingOnUser: boolean;
	/** This entry or one below it is not resolved. */
	readonly active: boolean;
}

/**
 * Roots first by urgency (something waits on the user, then anything unresolved, then the rest), oldest first
 * within each group; children oldest first. An entry whose parent is missing (or that sits in a cycle) is a root.
 */
export function build(entries: ReadonlyArray<Entry>): Array<Node> {
	const byId = new Map(entries.map((entry) => [entry.id, entry]));
	const children = new Map<number, Array<Entry>>();
	const roots: Array<Entry> = [];
	for (const entry of entries) {
		const parent = entry.parent;
		if (parent !== null && byId.has(parent) && !inCycle(entry, byId)) {
			const siblings = children.get(parent) ?? [];
			siblings.push(entry);
			children.set(parent, siblings);
		} else roots.push(entry);
	}
	const node = (entry: Entry): Node => {
		const kids = (children.get(entry.id) ?? []).toSorted(byIdAscending).map(node);
		return {
			entry,
			children: kids,
			waitingOnUser: waitingOn(entry) === "user" || kids.some((kid) => kid.waitingOnUser),
			active: entry.status !== "resolved" || kids.some((kid) => kid.active),
		};
	};
	return roots
		.toSorted(byIdAscending)
		.map(node)
		.toSorted((a, b) => urgency(a) - urgency(b));
}

/**
 * Resolved entries with nothing unresolved below them, children before parents: safe to delete in this order.
 * Nothing is pruned before its asker has resolved it.
 */
export function prunable(entries: ReadonlyArray<Entry>): Array<number> {
	const order: Array<number> = [];
	const visit = (node: Node) => {
		node.children.forEach(visit);
		if (!node.active) order.push(node.entry.id);
	};
	build(entries).forEach(visit);
	return order;
}

/**
 * The work queue the page lists and walks with Next: every unresolved entry at any depth, flat. Entries waiting on
 * the user come first, then those waiting on the agents; oldest first within each.
 */
export function queue(entries: ReadonlyArray<Entry>): Array<number> {
	const rank = (entry: Entry) => (waitingOn(entry) === "user" ? 0 : 1);
	return entries
		.filter((entry) => entry.status !== "resolved")
		.toSorted((a, b) => rank(a) - rank(b) || a.id - b.id)
		.map((entry) => entry.id);
}

export function waitingCount(entries: ReadonlyArray<Entry>, party: Party): number {
	return entries.filter((entry) => waitingOn(entry) === party).length;
}

/** The entry and its ancestors, root first: the context an answerer needs. */
export function thread(entries: ReadonlyArray<Entry>, id: number): Array<Entry> {
	const byId = new Map(entries.map((entry) => [entry.id, entry]));
	const path: Array<Entry> = [];
	const seen = new Set<number>();
	for (let current = byId.get(id); current && !seen.has(current.id); current = byId.get(current.parent ?? NaN)) {
		seen.add(current.id);
		path.unshift(current);
	}
	return path;
}

function urgency(node: Node): number {
	if (node.waitingOnUser) return 0;
	return node.active ? 1 : 2;
}

function byIdAscending(a: Entry, b: Entry): number {
	return a.id - b.id;
}

function inCycle(entry: Entry, byId: ReadonlyMap<number, Entry>): boolean {
	const seen = new Set<number>();
	for (let parent = entry.parent; parent !== null; ) {
		if (parent === entry.id) return true;
		if (seen.has(parent)) return false;
		seen.add(parent);
		parent = byId.get(parent)?.parent ?? null;
	}
	return false;
}
