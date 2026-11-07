import { Rpc } from "@opencode/plugin/rpc";
import * as Schema from "effect/Schema";

/** What the footer badge shows: entries waiting on the user, and where the page is. */
export const Status = Schema.Struct({ url: Schema.String, waiting: Schema.Int });
export type Status = typeof Status.Type;

// Standard Schema, so the TUI's plain `client.rpc` can validate without Effect codecs.
export const DeskRpc = Rpc.define({
	id: "desk",
	methods: {
		status: { input: Schema.toStandardSchemaV1(Schema.Struct({})), output: Schema.toStandardSchemaV1(Status) },
	},
	events: {
		changed: { schema: Schema.toStandardSchemaV1(Status) },
	},
});
