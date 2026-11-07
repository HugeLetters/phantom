/** @jsxImportSource solid-js */
/**
 * The desk page: open entries as a flat list, and an entry view with its parent, follow-ups, a draft box and Next.
 * Drafts live in the browser until Submit (all of them, one message per session) or Send now (one).
 */
import * as Effect from "effect/Effect";
import * as Fiber from "effect/Fiber";
import * as FetchHttpClient from "effect/http/FetchHttpClient";
import * as Layer from "effect/Layer";
import * as ManagedRuntime from "effect/ManagedRuntime";
import * as Schedule from "effect/Schedule";
import * as Stream from "effect/Stream";
import * as RpcClient from "effect/rpc/RpcClient";
import * as RpcSerialization from "effect/rpc/RpcSerialization";
import { createEffect, createMemo, createSignal, For, onCleanup, Show } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { render } from "solid-js/web";
import { DeskApi, type Draft, type View } from "../api";
import { type Entry, shortId, waitingOn } from "../entry";
import { render as markdown, renderDiagrams } from "./markdown";

const runtime = ManagedRuntime.make(
	RpcClient.layerProtocolHttp({ url: "/rpc" }).pipe(Layer.provide([FetchHttpClient.layer, RpcSerialization.layerNdjson])),
);
const client = RpcClient.make(DeskApi);
const withClient = <A, E>(f: (desk: Effect.Success<typeof client>) => Effect.Effect<A, E>) => Effect.scoped(Effect.flatMap(client, f));
/** Calls the desk; rejects with the desk's message. */
const call = <A, E>(f: (desk: Effect.Success<typeof client>) => Effect.Effect<A, E>) => runtime.runPromise(withClient(f));

const yours = (entry: Entry) => waitingOn(entry) === "user";

function Markdown(props: { text: string }) {
	let container!: HTMLDivElement;
	createEffect(() => {
		container.innerHTML = markdown(props.text);
		void renderDiagrams(container);
	});
	return <div class="md" ref={container} />;
}

function Pills(props: { entry: Entry }) {
	return (
		<>
			<span class={`pill ${props.entry.status}`}>{props.entry.status}</span>
			<Show when={yours(props.entry)}>
				<span class="pill yours">your turn</span>
			</Show>
		</>
	);
}

function App() {
	const [state, setState] = createStore<{ view: View | null; online: boolean }>({ view: null, online: true });
	const [drafts, setDrafts] = createStore<Record<string, string>>(JSON.parse(localStorage.getItem("desk.drafts") ?? "{}"));
	const [toast, setToast] = createSignal<{ message: string; error: boolean }>();
	const [hash, setHash] = createSignal(location.hash);

	const onHash = () => (setHash(location.hash), window.scrollTo(0, 0));
	window.addEventListener("hashchange", onHash);
	onCleanup(() => window.removeEventListener("hashchange", onHash));
	createEffect(() => localStorage.setItem("desk.drafts", JSON.stringify(drafts)));

	const subscription = runtime.runFork(
		withClient((desk) =>
			desk.view().pipe(
				Stream.runForEach((view) =>
					Effect.sync(() => {
						setState("online", true);
						setState("view", reconcile(view, { key: "id", merge: true }));
						// A draft for an entry that left the desk can't be sent anymore.
						for (const id of Object.keys(drafts)) if (!view.entries.some((entry) => entry.id === id)) setDrafts(id, undefined!);
					}),
				),
			),
		).pipe(Effect.tapError(() => Effect.sync(() => setState("online", false))), Effect.retry(Schedule.spaced("1 second"))),
	);
	onCleanup(() => runtime.runFork(Fiber.interrupt(subscription)));

	const entries = () => state.view?.entries ?? [];
	const open = createMemo(() => entries().filter((entry) => entry.status !== "resolved"));
	const mine = createMemo(() => open().filter(yours));
	const route = createMemo(() => /^#\/([0-9a-f-]+)$/.exec(hash())?.[1]);
	const pending = (): Array<Draft> => Object.entries(drafts).flatMap(([entry, text]) => (text?.trim() ? [{ entry, text }] : []));
	const byId = (id: string) => entries().find((entry) => entry.id === id);

	createEffect(() => (document.title = mine().length ? `(${mine().length}) Desk` : "Desk"));

	function notify(message: string, error = false) {
		const shown = { message, error };
		setToast(shown);
		setTimeout(() => toast() === shown && setToast(undefined), error ? 6000 : 2500);
	}
	const attempt = (action: () => Promise<string>) =>
		action().then(notify, (error: { message?: string }) => notify(error.message ?? String(error), true));

	const submit = () =>
		attempt(async () => {
			const count = await call((client) => client.submit({ drafts: pending() }));
			setDrafts(reconcile({}));
			return `Submitted ${count} draft${count === 1 ? "" : "s"}; queued for the agents`;
		});

	function List() {
		const Section = (props: { title: string; entries: ReadonlyArray<Entry> }) => (
			<Show when={props.entries.length > 0}>
				<div class="section">{props.title}</div>
				<div class="list">
					<For each={props.entries}>
						{(entry) => (
							<a class={`item${yours(entry) ? " turn" : ""}`} href={`#/${entry.id}`}>
								<span class="id">#{shortId(entry.id)}</span>
								<span class="title">{entry.title}</span>
								<Pills entry={entry} />
							</a>
						)}
					</For>
				</div>
			</Show>
		);
		return (
			<Show when={open().length > 0} fallback={<div class="empty">Nothing is open.</div>}>
				<Section title="Waiting on you" entries={mine()} />
				<Section title="Waiting on the agents" entries={open().filter((entry) => !yours(entry))} />
			</Show>
		);
	}

	function EntryView(props: { id: string }) {
		const entry = () => byId(props.id);
		/** The next entry waiting on you, by id, wrapping round. */
		const next = () => mine().find((other) => other.id > props.id) ?? mine().find((other) => other.id !== props.id);
		return (
			<>
				<div class="nav">
					<a href="#/">← All entries</a>
					<span class="spacer" />
					<button class="secondary" title={props.id} onClick={() => void attempt(() => navigator.clipboard.writeText(props.id).then(() => "Id copied"))}>
						Copy id
					</button>
					<button class="primary" disabled={!next()} onClick={() => (location.hash = `#/${next()!.id}`)}>
						Next →
					</button>
				</div>
				<Show when={entry()} fallback={<div class="empty">#{shortId(props.id)} is no longer on the desk.</div>}>
					{(entry) => {
						const parent = () => (entry().parent === null ? undefined : byId(entry().parent!));
						const children = () => entries().filter((other) => other.parent === entry().id);
						return (
							<div class={`card${yours(entry()) ? " turn" : ""}`}>
								<div class="head">
									<span class="id">#{shortId(entry().id)}</span>
									<span class="title">{entry().title}</span>
									<Pills entry={entry()} />
									<For each={entry().labels}>{(label) => <span class="label">{label}</span>}</For>
								</div>
								<div class="muted">{entry().asker === "agent" ? "agents → you" : "you → agents"}</div>
								<Show when={parent()}>
									{(parent) => (
										<div class="relation">
											Follow-up to <a href={`#/${parent().id}`}>#{shortId(parent().id)} {parent().title}</a> <Pills entry={parent()} />
										</div>
									)}
								</Show>
								<div class="body">
									<Markdown text={entry().question} />
								</div>
								<Show when={entry().answer}>
									{(answer) => (
										<div class="answer">
											<div class="caption">Answer from {entry().asker === "agent" ? "you" : "the agents"}</div>
											<Markdown text={answer()} />
										</div>
									)}
								</Show>
								<Show when={entry().status !== "resolved"}>
									<div class="actions">
										<textarea
											placeholder={yours(entry()) && entry().status === "open" ? "Your answer (Markdown)" : "A follow-up question (Markdown)"}
											value={drafts[entry().id] ?? ""}
											onInput={(event) => setDrafts(entry().id, event.currentTarget.value)}
										/>
										<div class="row">
											<button
												class="secondary"
												title="Deliver now, steering the session's current turn"
												onClick={() => {
													const draft = { entry: entry().id, text: drafts[entry().id] ?? "" };
													void attempt(async () => {
														await call((client) => client.send(draft));
														setDrafts(draft.entry, undefined!);
														return "Sent to the agents";
													});
												}}
											>
												Send now
											</button>
											<Show when={entry().asker === "user"}>
												<button
													class="secondary"
													onClick={() => {
														const id = entry().id;
														void attempt(() => call((client) => client.resolve({ id })).then(() => `#${shortId(id)} marked done`));
													}}
												>
													Mark done
												</button>
											</Show>
											<span class="note">Kept as a draft until Submit.</span>
										</div>
									</div>
								</Show>
								<Show when={children().length > 0}>
									<div class="children">
										<div class="caption">Follow-ups</div>
										<ul>
											<For each={children()}>
												{(child) => (
													<li>
														<a href={`#/${child.id}`}>
															#{shortId(child.id)} {child.title}
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
					}}
				</Show>
			</>
		);
	}

	return (
		<Show when={state.view} fallback={<div class="empty">Connecting…</div>}>
			<header>
				<div class="bar">
					<h1>
						<a href="#/">Desk</a>
					</h1>
					<span class="muted">{state.view!.root.split("/").pop()}</span>
					<span class="spacer" />
					<Show when={!state.online}>
						<span class="offline">reconnecting…</span>
					</Show>
					<button class="primary" disabled={pending().length === 0} title="Send every draft, one queued message per session" onClick={submit}>
						{pending().length ? `Submit ${pending().length} draft${pending().length === 1 ? "" : "s"}` : "Submit"}
					</button>
				</div>
			</header>
			<main>
				<Show when={route()} keyed fallback={<List />}>
					{(id) => <EntryView id={id} />}
				</Show>
			</main>
			<Show when={toast()} keyed>
				{(toast) => <div class={`toast${toast.error ? " error" : ""}`}>{toast.message}</div>}
			</Show>
		</Show>
	);
}

render(() => <App />, document.body);
