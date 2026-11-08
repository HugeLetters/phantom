/** The page's API, served by the plugin over HTTP (`/rpc`, NDJSON) and called by the page with a typed client. */
import * as Rpc from "effect/rpc/Rpc";
import * as RpcGroup from "effect/rpc/RpcGroup";
import * as Schema from "effect/Schema";
import { Entry, Id } from "./entry";

/** A refusal the page (or the agent, for tools) shows as is. */
export class DeskError extends Schema.TaggedError<DeskError>()("DeskError", { message: Schema.String }) {}

/** The user's text for an entry: an answer, or (on their own question) a follow-up. */
export const Draft = Schema.Struct({ entry: Id, text: Schema.String });
export type Draft = typeof Draft.Type;

export const View = Schema.Struct({ root: Schema.String, entries: Schema.Array(Entry) });
export type View = typeof View.Type;

export const DeskApi = RpcGroup.make(
	/** The desk now, then after every change. */
	Rpc.make("view", { success: View, stream: true }),
	/** Every draft, all or nothing; one queued message per session. Returns how many went. */
	Rpc.make("submit", { payload: { drafts: Schema.Array(Draft) }, success: Schema.Int, error: DeskError }),
	/** One draft, steering its session's current turn. */
	Rpc.make("send", { payload: Draft, error: DeskError }),
	/** Stores a file the user attached to a draft (base64) as `coordination/attachments/<uuid>.<ext>`; returns that path. */
	Rpc.make("attach", { payload: { name: Schema.String, data: Schema.String }, success: Schema.String, error: DeskError }),
	/** The user marks one of their own questions done. */
	Rpc.make("resolve", { payload: { id: Id }, error: DeskError }),
);
