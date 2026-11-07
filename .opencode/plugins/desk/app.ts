/**
 * How the desk's layers compose. The plugin entry adds OpenCode (`PluginContext`, `Sessions`); tests swap in fakes.
 *
 *   Platform        DeskConfig · FileSystem (node:fs) · Path
 *   Core            Store ← MainSession ← Desk                (needs Sessions)
 *   Background      Watcher · Pruning                         (streams over Store, forked in the layer's scope)
 *   Page            HttpRouter routes on HttpServer(Bun.serve) → PageUrl
 *   OpenCode        Tools · Status (RPC)                      (needs PluginContext)
 */
import * as Layer from "effect/Layer";
import * as Path from "effect/Path";
import { DeskConfig } from "./config";
import { Desk } from "./desk";
import * as Routes from "./http/routes";
import * as HttpServer from "./http/server";
import * as LocalFileSystem from "./local-fs";
import { MainSession } from "./main-session";
import * as Pruning from "./pruning";
import { Sessions } from "./sessions";
import * as Status from "./status";
import { Store } from "./store";
import * as Tools from "./tools";
import * as Watcher from "./watcher";

export const Platform = (root: string, options: { readonly mainSession?: string } = {}) =>
	Layer.mergeAll(LocalFileSystem.layer, Path.layer, DeskConfig.layer(root, options));

export const Core = Desk.layer.pipe(Layer.provideMerge(MainSession.layer), Layer.provideMerge(Store.layer));

export const Background = Layer.mergeAll(Watcher.layer, Pruning.layer);

export const Page = (port?: number) => HttpServer.layer(Routes.layer, port);

/** The whole desk inside OpenCode. */
export const Plugin = Layer.mergeAll(Tools.layer, Status.layer).pipe(
	Layer.provideMerge(Page()),
	Layer.provideMerge(Background),
	Layer.provideMerge(Core),
	Layer.provideMerge(Sessions.layer),
);
