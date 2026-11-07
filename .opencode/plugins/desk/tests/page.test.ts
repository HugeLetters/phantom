import { expect, test } from "bun:test";
import { bundle } from "../page/build";

test("the committed page bundle is built from the current source (run `bun plugins/desk/page/build.ts`)", async () => {
	const committed = await Bun.file(new URL("../page/dist/page.js", import.meta.url)).text();
	expect(committed === (await bundle())).toBe(true);
}, 30_000);
