import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = path.join(root, "site");
const output = path.join(root, "site-dist");

const fixtureCopies = [
	["Disappear Homer Simpson GIF.gif", "homer.gif"],
	["GIGACHAD-4x.gif", "gigachad.gif"],
	["partyparrot.gif", "partyparrot.gif"],
	["tenor.gif", "tenor.gif"],
];

await rm(output, { force: true, recursive: true });
await mkdir(path.join(output, "gifs"), { recursive: true });
await mkdir(path.join(output, "vendor"), { recursive: true });

await Promise.all([
	cp(path.join(source, "index.html"), path.join(output, "index.html")),
	cp(path.join(source, "styles.css"), path.join(output, "styles.css")),
	cp(path.join(source, "app.js"), path.join(output, "app.js")),
	cp(path.join(source, "assets"), path.join(output, "assets"), {
		recursive: true,
	}),
	cp(
		path.join(root, "dist", "index.mjs"),
		path.join(output, "vendor", "wtfgif.mjs"),
	),
	cp(
		path.join(root, "dist", "wasm-web"),
		path.join(output, "vendor", "wasm-web"),
		{ recursive: true },
	),
	...fixtureCopies.map(([from, to]) =>
		cp(path.join(root, "test", "gifs", from), path.join(output, "gifs", to)),
	),
]);

await build({
	entryPoints: [path.join(source, "omggif-entry.js")],
	bundle: true,
	format: "esm",
	mainFields: ["main", "module"],
	minify: true,
	outfile: path.join(output, "vendor", "omggif.mjs"),
	platform: "browser",
	target: ["es2022"],
});

const html = await readFile(path.join(output, "index.html"), "utf8");
if (!html.includes("./vendor/wtfgif.mjs")) {
	const app = await readFile(path.join(output, "app.js"), "utf8");
	if (!app.includes("./vendor/wtfgif.mjs")) {
		throw new Error("Built site does not reference the wtfgif browser module");
	}
}

await writeFile(path.join(output, ".nojekyll"), "");
console.log(
	`Built GitHub Pages artifact with ${fixtureCopies.length} GIFs and the real-image encoder workload.`,
);
