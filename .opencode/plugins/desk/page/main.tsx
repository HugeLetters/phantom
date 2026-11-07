/** @jsxImportSource solid-js */
/**
 * The desk page: a flat list of open entries (any depth), and an entry view with its parent, follow-ups, the
 * answer draft and Next. Live: the server pushes a new view on every change; `reconcile` keeps the DOM (and
 * whatever the user is typing) in place.
 */
import { createEffect, createMemo, createRoot, createSignal, For, type JSX, on, Show } from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import { render } from "solid-js/web";
import { api, type Draft, type Entry, type SessionInfo, type View } from "./api";
import { render as markdown, renderDiagrams } from "./markdown";

// ─── State ───────────────────────────────────────────────────────────────────

type Typed = { title: string; text: string };

const [state, setState] = createStore<{ view: View | null; online: boolean }>({ view: null, online: true });
/** Text typed but not yet echoed back by the server, so live updates never clobber typing. */
const [typed, setTyped] = createStore<Record<string, Typed>>({});
const saving = new Map<string, ReturnType<typeof setTimeout>>();
const [toast, setToast] = createSignal<{ message: string; error: boolean }>();
const [composing, setComposing] = createSignal(false);
const [following, setFollowing] = createSignal<ReadonlySet<number>>(new Set());
const [hash, setHash] = createSignal(location.hash);
window.addEventListener("hashchange", () => {
	void flushAll();
	setHash(location.hash);
	window.scrollTo(0, 0);
});
// The page's state lives for the page's lifetime: one root, never disposed.
const route = createRoot(() =>
	createMemo(() => {
		const match = /^#\/(\d+)$/.exec(hash());
		return match ? Number(match[1]) : null;
	}),
);

/**
 * How Next works through the list: it goes to the first entry, in the live list order, that the user hasn't opened
 * since starting from the list, and that wasn't above the current entry when the user opened it. So answering an
 * entry (which moves it down the list) doesn't send Next back to it, and an entry that arrives meanwhile is still
 * reached in its place.
 */
const [visited, setVisited] = createSignal<ReadonlySet<number>>(new Set());
/** The list as it was when the current entry was opened. */
let openedWith: ReadonlyArray<number> = [];
createRoot(() =>
	createEffect(
		on(route, (id) => {
			if (id === null) return;
			openedWith = state.view ? [...state.view.queue] : [];
			setVisited((ids) => (ids.has(id) ? ids : new Set([...ids, id])));
		}),
	),
);

const view = () => state.view!;
const byId = (id: number) => view().entries.find((entry) => entry.id === id);
const yourTurn = (entry: Entry) =>
	(entry.status === "open" && entry.answerer === "user") || (entry.status === "answered" && entry.asker === "user");
const answerKey = (id: number) => `answer-${id}`;
const askKey = (parent: number | null) => `ask-${parent ?? "top"}`;

function draftValue(key: string, field: keyof Typed): string {
	const local = typed[key];
	if (local) return local[field];
	const saved = view().drafts[key] as Draft | undefined;
	if (!saved) return "";
	return field === "title" ? (saved.kind === "ask" ? saved.title : "") : saved.text;
}

const draftCount = createRoot(() =>
	createMemo(() => {
		if (!state.view) return 0;
		const keys = new Set(Object.keys(view().drafts));
		for (const [key, local] of Object.entries(typed)) {
			if ((local.text || local.title).trim()) keys.add(key);
			else keys.delete(key);
		}
		return keys.size;
	}),
);

function notify(message: string, error = false) {
	setToast({ message, error });
	const shown = toast();
	setTimeout(() => toast() === shown && setToast(undefined), error ? 6000 : 2500);
}

async function attempt(action: () => Promise<unknown>) {
	try {
		await action();
	} catch (error) {
		notify((error as Error).message, true);
	}
}

// ─── Drafts ──────────────────────────────────────────────────────────────────

function type(key: string, field: keyof Typed, value: string) {
	setTyped(key, (local) => ({ ...(local ?? { title: draftValue(key, "title"), text: draftValue(key, "text") }), [field]: value }));
	clearTimeout(saving.get(key));
	saving.set(key, setTimeout(() => void flush(key), 400));
}

async function flush(key: string) {
	clearTimeout(saving.get(key));
	saving.delete(key);
	const local = typed[key];
	if (!local) return;
	const draft: Draft = key.startsWith("answer-")
		? { kind: "answer", entry: Number(key.slice("answer-".length)), text: local.text }
		: { kind: "ask", parent: key === "ask-top" ? null : Number(key.slice("ask-".length)), title: local.title, text: local.text };
	await api.saveDraft(draft).catch((error: Error) => notify(`Draft not saved: ${error.message}`, true));
}

const flushAll = () => Promise.all([...saving.keys()].map(flush));

/** The server caught up with the typing: stop overriding it. */
function settle(next: View) {
	setTyped(
		produce((locals) => {
			for (const [key, local] of Object.entries(locals)) {
				if (saving.has(key)) continue;
				const saved = next.drafts[key] as Draft | undefined;
				const empty = !local.text.trim() && !local.title.trim();
				const same = saved && saved.text === local.text && (saved.kind === "ask" ? saved.title : "") === local.title;
				if (same || (!saved && empty)) delete locals[key];
			}
		}),
	);
}

api.subscribe(
	(next) => {
		settle(next);
		setState("view", (current) => (current ? reconcile(next, { key: "id", merge: true })(current) : next));
	},
	(online) => setState("online", online),
);

// ─── Components ──────────────────────────────────────────────────────────────

function Markdown(props: { text: string }) {
	let container!: HTMLDivElement;
	const html = createMemo(() => markdown(view().root, props.text));
	createEffect(() => {
		container.innerHTML = html();
		void renderDiagrams(container);
	});
	return <div class="md" ref={container} />;
}

function Pills(props: { entry: Entry }) {
	return (
		<>
			<span class={`pill ${props.entry.status}`}>{props.entry.status}</span>
			<Show when={yourTurn(props.entry)}>
				<span class="pill yours">your turn</span>
			</Show>
		</>
	);
}

function DraftBox(props: { draftKey: string; placeholder: string; ask?: boolean }) {
	return (
		<div class="actions">
			<Show when={props.ask}>
				<input type="text" placeholder={props.placeholder} value={draftValue(props.draftKey, "title")} onInput={(event) => type(props.draftKey, "title", event.currentTarget.value)} />
			</Show>
			<textarea
				placeholder={props.ask ? "Details (Markdown)" : props.placeholder}
				value={draftValue(props.draftKey, "text")}
				onInput={(event) => type(props.draftKey, "text", event.currentTarget.value)}
			/>
			<div class="row">
				<button
					class="secondary"
					title="Deliver now, steering the main session's current turn"
					onClick={() =>
						attempt(async () => {
							await flushAll();
							await api.send(props.draftKey);
							setTyped(produce((locals) => void delete locals[props.draftKey]));
							notify("Sent to the main session");
						})
					}
				>
					Send now
				</button>
				<span class="note">Saved as a draft; Submit sends all drafts together.</span>
			</div>
		</div>
	);
}

function Header() {
	const [sessions, setSessions] = createSignal<ReadonlyArray<SessionInfo>>([]);
	const refresh = () => void api.sessions().then(setSessions);
	createEffect(on(() => view().main?.id, refresh));
	const options = createMemo(() => {
		const current = view().main;
		return current && !sessions().some((session) => session.id === current.id) ? [current, ...sessions()] : sessions();
	});
	return (
		<header>
			<div class="bar">
				<h1>
					<a href="#/">Desk</a>
				</h1>
				<span class="muted" title={view().root}>
					{view().root.split("/").pop()}
				</span>
				<span class="spacer" />
				<Show when={!state.online}>
					<span class="offline">reconnecting…</span>
				</Show>
				<select
					title="Main session: where answers are delivered"
					onFocus={refresh}
					onChange={(event) => {
						const sessionID = event.currentTarget.value;
						if (sessionID) void attempt(() => api.selectMain(sessionID).then(() => notify("Main session changed")));
					}}
				>
					<Show when={!view().main}>
						<option value="">Main: first session to use a desk tool</option>
					</Show>
					<For each={options()}>{(session) => <option value={session.id} selected={session.id === view().main?.id}>Main: {session.title}</option>}</For>
				</select>
				<button class="secondary" onClick={() => setComposing(true)}>
					New question
				</button>
				<button
					class="primary"
					disabled={draftCount() === 0}
					title="Send every draft to the main session in one message, after its current turn"
					onClick={() =>
						attempt(async () => {
							await flushAll();
							const count = await api.submit();
							setTyped(reconcile({}));
							setFollowing(new Set<number>());
							setComposing(false);
							notify(`Submitted ${count} draft${count === 1 ? "" : "s"}; queued for the main session`);
						})
					}
				>
					{draftCount() ? `Submit ${draftCount()} draft${draftCount() === 1 ? "" : "s"}` : "Submit"}
				</button>
			</div>
		</header>
	);
}

function List() {
	const entries = createMemo(() => view().queue.map((id) => byId(id)!).filter(Boolean));
	const Section = (props: { title: string; entries: ReadonlyArray<Entry> }) => (
		<Show when={props.entries.length > 0}>
			<div class="section">{props.title}</div>
			<div class="list">
				<For each={props.entries}>
					{(entry) => (
						<a class={`item${yourTurn(entry) ? " turn" : ""}`} href={`#/${entry.id}`} onClick={() => setVisited(new Set<number>())}>
							<span class="id">#{entry.id}</span>
							<span class="title">{entry.title}</span>
							<Pills entry={entry} />
						</a>
					)}
				</For>
			</div>
		</Show>
	);
	return (
		<Show when={entries().length > 0} fallback={<div class="empty">Nothing is open.</div>}>
			<Section title="Waiting on you" entries={entries().filter(yourTurn)} />
			<Section title="Waiting on the agents" entries={entries().filter((entry) => !yourTurn(entry))} />
		</Show>
	);
}

function nextAfter(id: number): number | null {
	const position = openedWith.indexOf(id);
	const above = new Set(position >= 0 ? openedWith.slice(0, position) : []);
	return view().queue.find((other) => other !== id && !visited().has(other) && !above.has(other)) ?? null;
}

function Nav(props: { id: number }) {
	const next = createMemo(() => nextAfter(props.id));
	const position = createMemo(() => view().queue.indexOf(props.id));
	return (
		<div class="nav">
			<a href="#/">← All entries</a>
			<span class="spacer" />
			<Show when={position() >= 0}>
				<span class="muted">
					{position() + 1} of {view().queue.length}
				</span>
			</Show>
			<button
				class="primary"
				disabled={next() === null}
				title={next() === null ? "No more open entries" : undefined}
				onClick={() => {
					const target = next();
					if (target !== null) location.hash = `#/${target}`;
				}}
			>
				Next →
			</button>
		</div>
	);
}

function EntryPage(props: { id: number }) {
	const entry = createMemo(() => byId(props.id));
	return (
		<>
			<Nav id={props.id} />
			<Show when={entry()} fallback={<div class="empty">#{props.id} is no longer on the desk: it was resolved and removed.</div>}>
				{(entry) => <EntryCard entry={entry()} />}
			</Show>
		</>
	);
}

function EntryCard(props: { entry: Entry }) {
	const entry = () => props.entry;
	const parent = createMemo(() => (entry().parent === null ? undefined : byId(entry().parent!)));
	const children = createMemo(() => view().entries.filter((other) => other.parent === entry().id));
	const followUp = createMemo(() => following().has(entry().id) || draftValue(askKey(entry().id), "text") !== "" || draftValue(askKey(entry().id), "title") !== "");
	return (
		<div class={`card${yourTurn(entry()) ? " turn" : ""}`}>
			<div class="head">
				<span class="id">#{entry().id}</span>
				<span class="title">{entry().title}</span>
				<Pills entry={entry()} />
				<For each={entry().labels}>{(label) => <span class="label">{label}</span>}</For>
			</div>
			<div class="muted">{entry().asker === "main" ? "agents → you" : "you → agents"} · {relative(entry().created)}</div>
			<Show when={entry().parent !== null}>
				<div class="relation">
					Follow-up to{" "}
					<Show when={parent()} fallback={<>#{entry().parent} (removed)</>}>
						{(parent) => (
							<>
								<a href={`#/${parent().id}`}>
									#{parent().id} {parent().title}
								</a>{" "}
								<Pills entry={parent()} />
							</>
						)}
					</Show>
				</div>
			</Show>
			<div class="body">
				<Show when={entry().question} fallback={<p class="note">(no details)</p>}>
					<Markdown text={entry().question} />
				</Show>
			</div>
			<Show when={entry().answer !== null}>
				<div class="answer">
					<div class="caption">Answer from {entry().answerer === "user" ? "you" : "the agents"}</div>
					<Markdown text={entry().answer!} />
				</div>
			</Show>
			<Show when={entry().status !== "resolved"}>
				<Show when={entry().answerer === "user"}>
					<DraftBox draftKey={answerKey(entry().id)} placeholder={entry().status === "answered" ? "Change your answer (Markdown)" : "Your answer (Markdown)"} />
				</Show>
				<div class="row actions">
					<Show when={entry().asker === "user"}>
						<button
							class="secondary"
							onClick={() => {
								// The entry leaves the store as soon as it's pruned; keep its id.
								const id = entry().id;
								void attempt(() => api.resolve(id).then(() => notify(`#${id} marked done`)));
							}}
						>
							Mark done
						</button>
					</Show>
					<Show when={!followUp()}>
						<button class="link" onClick={() => setFollowing((ids) => new Set([...ids, entry().id]))}>
							Follow up
						</button>
					</Show>
				</div>
				<Show when={followUp()}>
					<DraftBox draftKey={askKey(entry().id)} placeholder="Follow-up question" ask />
				</Show>
			</Show>
			<Show when={children().length > 0}>
				<div class="children">
					<div class="caption">Follow-ups</div>
					<ul>
						<For each={children()}>
							{(child) => (
								<li>
									<a href={`#/${child.id}`}>
										#{child.id} {child.title}
									</a>{" "}
									<Pills entry={child} />
								</li>
							)}
						</For>
					</ul>
				</div>
			</Show>
		</div>
	);
}

function relative(iso: string): string {
	const seconds = (Date.now() - Date.parse(iso)) / 1000;
	if (!Number.isFinite(seconds)) return "";
	if (seconds < 90) return "just now";
	if (seconds < 5400) return `${Math.round(seconds / 60)} min ago`;
	if (seconds < 129600) return `${Math.round(seconds / 3600)} h ago`;
	return new Date(iso).toLocaleDateString();
}

function App(): JSX.Element {
	createEffect(() => {
		const waiting = state.view?.waiting ?? 0;
		document.title = waiting ? `(${waiting}) Desk` : "Desk";
	});
	return (
		<Show when={state.view} fallback={<div class="empty">Connecting…</div>}>
			<Header />
			<main>
				<Show when={composing() || draftValue(askKey(null), "title") !== "" || draftValue(askKey(null), "text") !== ""}>
					<div class="card composer">
						<div class="caption">New question for the agents</div>
						<DraftBox draftKey={askKey(null)} placeholder="Your question for the agents" ask />
					</div>
				</Show>
				<Show when={route()} keyed fallback={<List />}>
					{(id) => <EntryPage id={id} />}
				</Show>
			</main>
			<Show when={toast()} keyed>
				{(toast) => <div class={`toast${toast.error ? " error" : ""}`}>{toast.message}</div>}
			</Show>
		</Show>
	);
}

render(() => <App />, document.getElementById("app")!);
