/** The page's side of the desk API (see `http/routes.ts`). */
import type { View } from "../http/routes";
import type { Draft } from "../store";

export type { View };
export type Entry = View["entries"][number];
export type { Draft };

export interface SessionInfo {
	readonly id: string;
	readonly title: string;
}

/** POSTs carry `x-desk` so the server can tell them from cross-site requests. Throws the server's message. */
async function post<A>(path: string, body: unknown = {}): Promise<A> {
	const response = await fetch(path, { method: "POST", headers: { "content-type": "application/json", "x-desk": "1" }, body: JSON.stringify(body) });
	const result = (await response.json().catch(() => ({ ok: false, error: response.statusText }))) as { ok: boolean; value?: A; error?: string };
	if (!result.ok) throw new Error(result.error);
	return result.value as A;
}

export const api = {
	saveDraft: (draft: Draft) => post<null>("/api/draft", draft),
	submit: () => post<number>("/api/submit"),
	send: (key: string) => post<number>("/api/send", { key }),
	resolve: (id: number) => post<unknown>("/api/resolve", { id }),
	selectMain: (sessionID: string) => post<null>("/api/main", { sessionID }),
	async sessions(): Promise<ReadonlyArray<SessionInfo>> {
		const result = (await fetch("/api/sessions").then((response) => response.json())) as { ok: boolean; value?: ReadonlyArray<SessionInfo> };
		return result.ok ? (result.value ?? []) : [];
	},
	/** Live view: called with every view the server sends; `online` tracks the connection. */
	subscribe(onView: (view: View) => void, online: (connected: boolean) => void): () => void {
		const source = new EventSource("/api/events");
		source.addEventListener("view", (event) => {
			online(true);
			onView(JSON.parse((event as MessageEvent<string>).data) as View);
		});
		source.onerror = () => online(false);
		return () => source.close();
	},
};
