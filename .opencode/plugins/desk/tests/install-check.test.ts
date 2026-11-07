import { afterEach, beforeEach, expect, test } from "bun:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { missingPackages } from "../install-check";

let root: string;
beforeEach(async () => {
	root = await mkdtemp(join(tmpdir(), "desk-install-"));
});
afterEach(() => rm(root, { recursive: true, force: true }));

const manifest = (directory: string, dependencies: Record<string, string>) => Bun.write(join(directory, "package.json"), JSON.stringify({ dependencies }));

test("lists direct and transitive dependencies that aren't installed", async () => {
	await manifest(root, { a: "1", "@scope/b": "1" });
	expect(missingPackages(root)).toEqual(["@scope/b", "a"]);
	await manifest(join(root, "node_modules", "a"), { c: "1" });
	expect(missingPackages(root)).toEqual(["@scope/b", "c"]);
	await manifest(join(root, "node_modules", "@scope", "b"), {});
	await manifest(join(root, "node_modules", "a", "node_modules", "c"), { a: "1" }); // nested, and a cycle back to a
	expect(missingPackages(root)).toEqual([]);
});
