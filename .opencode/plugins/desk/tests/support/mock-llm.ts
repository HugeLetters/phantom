/**
 * A scripted OpenAI-compatible model for integration runs, so a throwaway OpenCode server needs no credentials.
 *
 * A user message containing `@call <tool> <json>` gets that tool call back; anything else gets `ack: <text>`.
 * `@slow` delays the reply 4 s, to queue or steer into a running turn. Every request is kept in `requests`;
 * `MOCK_LLM_LOG=<file>` also writes the latest one there. Run directly to keep one up: `bun mock-llm.ts <port>`.
 */
export interface MockLlm {
	readonly url: string;
	readonly requests: ReadonlyArray<{ readonly messages: ReadonlyArray<{ role: string; content: unknown }> }>;
	readonly stop: () => void;
}

export function startMockLlm(port = 0): MockLlm {
	const requests: Array<{ messages: Array<{ role: string; content: unknown }> }> = [];
	const server = Bun.serve({
		port,
		hostname: "127.0.0.1",
		async fetch(request) {
			const url = new URL(request.url);
			if (url.pathname.endsWith("/models")) return Response.json({ data: [{ id: "mock", object: "model" }] });
			if (!url.pathname.endsWith("/chat/completions")) return new Response("not found", { status: 404 });
			const body = (await request.json()) as { messages: Array<{ role: string; content: unknown }>; stream?: boolean };
			requests.push(body);
			if (process.env.MOCK_LLM_LOG) await Bun.write(process.env.MOCK_LLM_LOG, JSON.stringify(body, null, 1));
			const last = body.messages.at(-1);
			const text = last ? textOf(last.content) : "";
			if (last?.role === "user" && text.includes("@slow")) await Bun.sleep(4000);
			const call = last?.role === "user" ? /@call (\S+) (\{.*\})/s.exec(text) : null;
			const chunks = call
				? [
						{ delta: { role: "assistant", tool_calls: [{ index: 0, id: `call_${requests.length}`, type: "function", function: { name: call[1], arguments: call[2] } }] } },
						{ delta: {}, finish_reason: "tool_calls" },
					]
				: [
						{ delta: { role: "assistant", content: last?.role === "tool" ? "done" : `ack: ${text.slice(0, 200)}` } },
						{ delta: {}, finish_reason: "stop" },
					];
			const lines = chunks.map((choice) => `data: ${JSON.stringify({ id: "mock", object: "chat.completion.chunk", created: 0, model: "mock", choices: [{ index: 0, ...choice }] })}\n\n`);
			lines.push(`data: ${JSON.stringify({ id: "mock", object: "chat.completion.chunk", created: 0, model: "mock", choices: [], usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 } })}\n\n`, "data: [DONE]\n\n");
			return new Response(lines.join(""), { headers: { "content-type": "text/event-stream" } });
		},
	});
	return { url: `http://127.0.0.1:${server.port}/v1`, requests, stop: () => server.stop(true) };
}

function textOf(content: unknown): string {
	if (typeof content === "string") return content;
	if (Array.isArray(content)) return content.map((part) => (typeof part?.text === "string" ? part.text : "")).join("\n");
	return "";
}

if (import.meta.main) {
	const llm = startMockLlm(Number(process.argv[2] ?? 0));
	console.log(llm.url);
}
