/** Entry Markdown → sanitized HTML: GFM, highlighted code, images/GIFs, video via image syntax, and Mermaid. */
import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/common";
import { marked } from "marked";
import type { Mermaid } from "mermaid";

const video = /\.(mp4|webm|mov|m4v)$/i;
/** Repo-relative paths load through the desk's `/files/`. */
const asset = (url: string) => (/^([a-z]+:|#|\/)/i.test(url) ? url : `/files/${url.replace(/^\.\//, "")}`);

export function render(text: string): string {
	const template = document.createElement("template");
	template.innerHTML = DOMPurify.sanitize(marked.parse(text, { async: false }));
	for (const img of template.content.querySelectorAll("img")) {
		const src = asset(img.getAttribute("src") ?? "");
		if (!video.test(src)) {
			img.src = src;
			continue;
		}
		const player = Object.assign(document.createElement("video"), { src, controls: true, loop: true, muted: true, title: img.alt });
		img.replaceWith(player);
	}
	for (const link of template.content.querySelectorAll("a")) {
		link.href = asset(link.getAttribute("href") ?? "");
		link.target = "_blank";
	}
	for (const code of template.content.querySelectorAll("pre > code")) {
		if (!code.classList.contains("language-mermaid")) {
			hljs.highlightElement(code as HTMLElement);
			continue;
		}
		const diagram = Object.assign(document.createElement("div"), { className: "mermaid", textContent: code.textContent });
		code.parentElement!.replaceWith(diagram);
	}
	return template.innerHTML;
}

/** The plugin serves Mermaid's prebuilt bundle; it loads the first time a diagram appears. */
let mermaid: Promise<Mermaid> | undefined;
const mermaidUrl = "/mermaid/mermaid.esm.min.mjs";

export async function renderDiagrams(container: HTMLElement): Promise<void> {
	const diagrams = [...container.querySelectorAll<HTMLElement>(".mermaid:not([data-processed])")];
	if (diagrams.length === 0) return;
	mermaid ??= import(mermaidUrl).then(({ default: engine }: { default: Mermaid }) => {
		engine.initialize({ startOnLoad: false, securityLevel: "strict" });
		return engine;
	});
	await (await mermaid).run({ nodes: diagrams });
}
