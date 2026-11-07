/**
 * The desk's layers: `Desk` (entries and their files) behind the page server, and, inside OpenCode, the tools and
 * the TUI's RPC. `Sessions` is the seam: OpenCode's sessions in the plugin, recorded ones in demos and tests.
 */
import { createHash } from "node:crypto";
import * as BunServices from "@effect/platform-bun/BunServices";
import type { Plugin } from "@opencode/plugin/effect";
import * as Layer from "effect/Layer";
import { Desk } from "./desk";
import * as OpenCode from "./opencode";
import * as Http from "./http";
import { Sessions } from "./sessions";

/** The desk for the checkout at `root`, its page on `port` (0: any free port). Needs `Sessions`. */
export const desk = (root: string, port: number) =>
	Http.layer(root, port).pipe(Layer.provideMerge(Desk.layer(root)), Layer.provideMerge(BunServices.layer));

/** A stable port per project, so a bookmarked page keeps working: 47000–47999. */
const port = (project: string) => 47000 + (createHash("sha256").update(project).digest().readUInt16BE(0) % 1000);

/** The desk inside OpenCode, for the checkout at `root`. */
export const layer = (ctx: Plugin.Context, root: string) =>
	OpenCode.layer(ctx).pipe(Layer.provideMerge(desk(root, port(ctx.location.project.id))), Layer.provide(Sessions.opencode(ctx)));
