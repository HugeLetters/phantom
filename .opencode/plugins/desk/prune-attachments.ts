/**
 * One-off cleanup: deletes the files in `coordination/attachments/` that no entry references. Before attachments were
 * stored on send, attaching uploaded at once, so a file whose link the user deleted, or one in a draft never sent,
 * stayed there for good.
 *
 *   cd .opencode && bun run prune-attachments [root]   # default: the checkout this plugin is in
 *
 * Entries are written to disk on every change, so the files are the whole desk; run it while nobody is sending.
 */
import { readdir, readFile, rm } from "node:fs/promises";
import { join } from "node:path";
import * as Option from "effect/Option";
import { attachmentsIn, idOf, parse } from "./entry";

/** Deletes the unreferenced attachments of the desk at `root`; returns their repo-relative paths. */
export async function pruneAttachments(root: string): Promise<Array<string>> {
	const directory = join(root, "coordination");
	const referenced = new Set<string>();
	for (const name of await readdir(directory)) {
		const id = idOf(name);
		const entry = id === undefined ? Option.none() : parse(id, await readFile(join(directory, name), "utf8"));
		if (Option.isSome(entry)) for (const { file } of attachmentsIn(`${entry.value.question}\n${entry.value.answer ?? ""}`)) referenced.add(file);
	}
	const files = await readdir(join(directory, "attachments")).catch(() => []);
	const orphans = files.map((name) => `coordination/attachments/${name}`).filter((file) => !referenced.has(file));
	for (const file of orphans) await rm(join(root, file), { force: true });
	return orphans;
}

if (import.meta.main) {
	const root = process.argv[2] ?? join(import.meta.dir, "../../..");
	const pruned = await pruneAttachments(root);
	for (const file of pruned) console.log(`deleted ${file}`);
	console.log(`${pruned.length} unreferenced attachment${pruned.length === 1 ? "" : "s"} deleted from ${join(root, "coordination", "attachments")}`);
}
