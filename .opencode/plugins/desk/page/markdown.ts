/** Entry Markdown → sanitized HTML: GFM, highlighted code, Mermaid, images/GIFs, and video via image syntax. */
import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/common";
import { marked } from "marked";

const videoExtension = /\.(mp4|webm|mov|m4v)(\?|#|$)/i;

/** Repo-relative paths (and file:// URLs inside the checkout) load through the desk's `/files/` route. */
export function asset(root: string, url: string | null): string {
	if (!url || /^(https?:|data:|blob:|#|mailto:)/i.test(url)) return url ?? "";
	if (url.startsWith("file://")) {
		const path = decodeURIComponent(url.slice(7));
		return path.startsWith(`${root}/`) ? `/files/${path.slice(root.length + 1)}` : url;
	}
	if (url.startsWith(`${root}/`)) return `/files/${url.slice(root.length + 1)}`;
	return `/files/${url.replace(/^\.?\//, "")}`;
}

export function render(root: string, text: string): string {
	const html = DOMPurify.sanitize(marked.parse(text, { gfm: true, async: false }), {
		ADD_TAGS: ["video"],
		ADD_ATTR: ["controls", "loop", "muted", "playsinline"],
	});
	const template = document.createElement("template");
	template.innerHTML = html;
	for (const img of template.content.querySelectorAll("img")) {
		const src = asset(root, img.getAttribute("src"));
		if (videoExtension.test(src)) {
			const video = document.createElement("video");
			Object.assign(video, { src, controls: true, loop: true, muted: true, playsInline: true, title: img.alt });
			img.replaceWith(video);
		} else img.setAttribute("src", src);
	}
	for (const link of template.content.querySelectorAll("a[href]")) {
		const href = link.getAttribute("href") ?? "";
		if (href.startsWith("#")) continue;
		if (!/^(https?:|mailto:)/i.test(href)) link.setAttribute("href", asset(root, href));
		link.setAttribute("target", "_blank");
		link.setAttribute("rel", "noreferrer");
	}
	for (const code of template.content.querySelectorAll("pre > code")) {
		if (code.classList.contains("language-mermaid")) {
			const div = document.createElement("div");
			div.className = "mermaid";
			div.dataset.source = code.textContent ?? "";
			code.parentElement!.replaceWith(div);
		} else hljs.highlightElement(code as HTMLElement);
	}
	return template.innerHTML;
}

interface Mermaid {
	initialize(config: object): void;
	render(id: string, source: string): Promise<{ svg: string }>;
}

/** Mermaid is large, so it loads from jsDelivr the first time a diagram appears. */
const mermaidUrl = "https://cdn.jsdelivr.net/npm/mermaid@11.6.0/dist/mermaid.esm.min.mjs";
let mermaid: Promise<Mermaid> | undefined;
const diagrams = new Map<string, string>();

export async function renderDiagrams(container: HTMLElement): Promise<void> {
	const nodes = [...container.querySelectorAll<HTMLElement>(".mermaid[data-source]")];
	if (nodes.length === 0) return;
	mermaid ??= import(/* @vite-ignore */ mermaidUrl).then((module: { default: Mermaid }) => {
		module.default.initialize({ startOnLoad: false, theme: "default", securityLevel: "strict" });
		return module.default;
	});
	const engine = await mermaid;
	for (const node of nodes) {
		const source = node.dataset.source ?? "";
		delete node.dataset.source;
		try {
			if (!diagrams.has(source)) diagrams.set(source, (await engine.render(`m${diagrams.size}`, source)).svg);
			node.innerHTML = diagrams.get(source)!;
		} catch (error) {
			node.textContent = `${source}\n\nMermaid: ${(error as Error).message}`;
		}
	}
}
