/**
 * Is `.opencode/`'s `bun install` complete? Walks the dependency tree from `package.json` through each package's own
 * `dependencies`, resolving like Node (the nearest `node_modules` up the tree), on the file system only.
 *
 * The plugin asks before importing anything that needs the install, so a missing or partial install is one clear
 * log line rather than a resolve error. Imports only Node built-ins, so it runs without an install.
 */
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";

interface Manifest {
	readonly dependencies?: Record<string, string>;
}

function manifest(directory: string): Manifest | undefined {
	try {
		return JSON.parse(readFileSync(join(directory, "package.json"), "utf8")) as Manifest;
	} catch {
		return undefined;
	}
}

/** The installed directory of `name` as seen from `from`, or undefined. */
function locate(name: string, from: string, root: string): string | undefined {
	for (let directory = from; ; directory = dirname(directory)) {
		const candidate = join(directory, "node_modules", name);
		if (existsSync(join(candidate, "package.json"))) return candidate;
		if (directory === root || directory === dirname(directory)) return undefined;
	}
}

/** Packages `package.json` (transitively) needs that aren't installed; empty when the install is complete. */
export function missingPackages(packageRoot: string): Array<string> {
	const missing = new Set<string>();
	const seen = new Set<string>();
	const queue: Array<string> = [packageRoot];
	while (queue.length > 0) {
		const directory = queue.pop()!;
		for (const name of Object.keys(manifest(directory)?.dependencies ?? {})) {
			const found = locate(name, directory, packageRoot);
			if (!found) missing.add(name);
			else if (!seen.has(found)) {
				seen.add(found);
				queue.push(found);
			}
		}
	}
	return [...missing].sort();
}
