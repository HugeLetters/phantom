/**
 * `FileSystem.FileSystem` over `node:fs`, for the operations the desk uses.
 *
 * Effect's own implementations live in `@effect/platform-bun`/`-node`, which OpenCode does not provide to plugins
 * that have no `bun install`; `effect` itself is provided, so the desk codes against its `FileSystem` interface and
 * supplies this small implementation. Unused operations fail (see `FileSystem.makeNoop`).
 */
import * as NFS from "node:fs";
import * as fs from "node:fs/promises";
import { resolve } from "node:path";
import * as Cause from "effect/Cause";
import * as Effect from "effect/Effect";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as PlatformError from "effect/PlatformError";
import * as Queue from "effect/Queue";
import * as Size from "effect/ByteSize";
import * as Stream from "effect/Stream";

const tags: Record<string, PlatformError.SystemErrorTag> = {
	ENOENT: "NotFound",
	EEXIST: "AlreadyExists",
	EACCES: "PermissionDenied",
	EPERM: "PermissionDenied",
	EBUSY: "Busy",
};

function attempt<A>(method: string, path: string, run: () => Promise<A>): Effect.Effect<A, PlatformError.PlatformError> {
	return Effect.tryPromise({
		try: run,
		catch: (cause) =>
			PlatformError.systemError({
				_tag: tags[(cause as NodeJS.ErrnoException)?.code ?? ""] ?? "Unknown",
				module: "FileSystem",
				method,
				pathOrDescriptor: path,
				cause,
			}),
	});
}

function info(stat: NFS.BigIntStats): FileSystem.File.Info {
	return {
		type: stat.isDirectory() ? "Directory" : stat.isFile() ? "File" : stat.isSymbolicLink() ? "SymbolicLink" : "Unknown",
		mtime: Option.some(stat.mtime),
		atime: Option.some(stat.atime),
		birthtime: Option.some(stat.birthtime),
		dev: stat.dev,
		ino: Option.some(stat.ino),
		mode: Number(stat.mode),
		nlink: Option.some(stat.nlink),
		uid: Option.some(Number(stat.uid)),
		gid: Option.some(Number(stat.gid)),
		rdev: Option.some(stat.rdev),
		size: Size.bytes(stat.size),
		blksize: Option.some(Size.bytes(stat.blksize)),
		blocks: Option.some(stat.blocks),
	};
}

/** The same events Effect's Node watcher emits: a rename is a Create if the path still exists, else a Remove. */
const watch = (path: string, options?: FileSystem.WatchOptions) =>
	Stream.callback<FileSystem.WatchEvent, PlatformError.PlatformError>((queue) =>
		Effect.acquireRelease(
			Effect.sync(() => {
				const watcher = NFS.watch(path, { recursive: options?.recursive ?? false }, (event, name) => {
					if (!name) return;
					if (event === "change") return void Queue.offerUnsafe(queue, { _tag: "Update", path: name });
					const tag = NFS.existsSync(resolve(path, name)) ? "Create" : "Remove";
					Queue.offerUnsafe(queue, { _tag: tag, path: name });
				});
				watcher.on("error", (cause) =>
					Queue.failCauseUnsafe(queue, Cause.fail(PlatformError.systemError({ _tag: "Unknown", module: "FileSystem", method: "watch", pathOrDescriptor: path, cause }))),
				);
				watcher.on("close", () => Queue.endUnsafe(queue));
				return watcher;
			}),
			(watcher) => Effect.sync(() => watcher.close()),
		),
	);


export const layer: Layer.Layer<FileSystem.FileSystem> = FileSystem.layerNoop({
	exists: (path) => Effect.sync(() => NFS.existsSync(path)),
	makeDirectory: (path, options) => attempt("makeDirectory", path, () => fs.mkdir(path, options).then(() => undefined)),
	readDirectory: (path) => attempt("readDirectory", path, () => fs.readdir(path)),
	readFile: (path) => attempt("readFile", path, () => fs.readFile(path)),
	readFileString: (path) => attempt("readFileString", path, () => fs.readFile(path, "utf8")),
	realPath: (path) => attempt("realPath", path, () => fs.realpath(path)),
	remove: (path, options) => attempt("remove", path, () => fs.rm(path, options)),
	rename: (from, to) => attempt("rename", from, () => fs.rename(from, to)),
	stat: (path) => attempt("stat", path, () => fs.stat(path, { bigint: true })).pipe(Effect.map(info)),
	watch,
	writeFile: (path, data) => attempt("writeFile", path, () => fs.writeFile(path, data)),
	writeFileString: (path, data) => attempt("writeFileString", path, () => fs.writeFile(path, data)),
});
