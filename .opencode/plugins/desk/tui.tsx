/** The footer badge: how many desk entries wait on the user; clicking it opens the desk page. */
import { Plugin } from "@opencode/plugin/tui";
import type { SlotMap } from "@opencode/plugin/tui/context";
import { createEffect, createSignal, onCleanup } from "solid-js";
import { DeskRpc, type Status } from "./tui-rpc";

/** Opens a URL in the browser; DESK_OPENER replaces `open` for tests. */
function openUrl(url: string) {
	Bun.spawn([process.env.DESK_OPENER ?? "open", url], { stdout: "ignore", stderr: "ignore" });
}

function Badge(props: { context: Plugin.Context; slot: SlotMap["prompt.footer.status"] }) {
	const context = props.context;
	const desk = context.client.rpc(DeskRpc);
	const [status, setStatus] = createSignal<Status>();
	const [hovered, setHovered] = createSignal(false);
	const location = () => {
		const sessionID = props.slot.sessionID;
		return (sessionID ? context.data.session.get(sessionID)?.location : undefined) ?? context.location ?? context.data.location.default();
	};

	createEffect(() => {
		const where = location();
		const controller = new AbortController();
		// Locations without the desk plugin reject: the badge stays hidden there.
		desk.status({}, { location: where, signal: controller.signal }).then(setStatus, () => setStatus(undefined));
		const stop = desk.events.on("changed", (event) => {
			if (event.location.directory === where.directory) setStatus(event.data as Status);
		});
		onCleanup(() => {
			controller.abort();
			stop();
		});
	});

	const label = () => {
		const waiting = status()?.waiting ?? 0;
		const critical = status()?.critical ?? 0;
		return waiting > 0 ? `desk ${waiting}${critical > 0 ? ` !${critical}` : ""}` : "desk";
	};
	const color = () => {
		if (hovered()) return context.theme.text.action.secondary.hovered;
		if ((status()?.critical ?? 0) > 0) return context.theme.text.error;
		return (status()?.waiting ?? 0) > 0 ? context.theme.text.warning : context.theme.text.muted;
	};

	return (
		<text
			visible={status() !== undefined}
			marginLeft={1}
			selectable={false}
			fg={color()}
			bg={hovered() ? context.theme.background.action.secondary.hovered : undefined}
			onMouseOver={() => setHovered(true)}
			onMouseOut={() => setHovered(false)}
			onMouseDown={(event) => {
				const url = status()?.url;
				if (event.button !== 0 || !url) return;
				event.preventDefault();
				event.stopPropagation();
				openUrl(url);
			}}
		>
			{label()}
		</text>
	);
}

export default Plugin.define({
	id: "desk",
	setup(context) {
		return context.ui.slot({
			append: "prompt.footer.status",
			render: (slot) => <Badge context={context} slot={slot} />,
		});
	},
});
