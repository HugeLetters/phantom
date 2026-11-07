/**
 * How the desk's layers compose. The plugin entry adds OpenCode (`PluginContext`, `Sessions`); tests swap in fakes.
 *
 *   Platform        DeskConfig · BunServices (FileSystem, Path, …)
 *   Core            Store ← MainSession ← Desk                (needs Sessions)
 *   Background      Watcher · Pruning                         (streams over Store, forked in the layer's scope)
 *   Page            HttpRouter routes on BunHttpServer → PageUrl; the Solid page compiled on load (PageBundle)
 *   OpenCode        Tools · Status (RPC)                      (needs PluginContext)
 */
import * as Context from "effect/Context";
import * as Layer from "effect/Layer";
import * as BunServices from "@effect/platform-bun/BunServices";
import { DeskConfig } from "./config";
import { Desk } from "./desk";
import * as Routes from "./http/routes";
import * as HttpServer from "./http/server";
import { PageBundle } from "./page/bundle";
import { MainSession } from "./main-session";
import * as Pruning from "./pruning";
import { Sessions } from "./sessions";
import * as Status from "./status";
import { Store } from "./store";
import * as Tools from "./tools";
import * as Watcher from "./watcher";

export const Platform = (root: string, options: { readonly mainSession?: string } = {}) =>
	Layer.mergeAll(BunServices.layer, DeskConfig.layer(root, options));

export const Core = Desk.layer.pipe(Layer.provideMerge(MainSession.layer), Layer.provideMerge(Store.layer));

export const Background = Layer.mergeAll(Watcher.layer, Pruning.layer);

export const Page = (port?: number) => HttpServer.layer(Routes.layer, port).pipe(Layer.provide(PageBundle.layer));

/** The whole desk inside OpenCode. */
export const Plugin = Layer.mergeAll(Tools.layer, Status.layer).pipe(
	Layer.provideMerge(Page()),
	Layer.provideMerge(Background),
	Layer.provideMerge(Core),
	Layer.provideMerge(Sessions.layer),
);

/** The page's address in a built `Plugin`. */
export const pageUrl = (services: Context.Context<HttpServer.PageUrl>) => Context.get(services, HttpServer.PageUrl);
