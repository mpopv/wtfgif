import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const files = [
	"wtfgif_core.d.ts",
	"wtfgif_core.js",
	"wtfgif_core_bg.wasm",
	"wtfgif_core_bg.wasm.d.ts",
];

const packages = [
	{ source: "pkg", target: "wasm-core", label: "Node" },
	{ source: "pkg-web", target: "wasm-web", label: "browser" },
];

for (const wasmPackage of packages) {
	const sourceDir = join(
		root,
		"crates",
		"wtfgif-core",
		wasmPackage.source,
	);
	const targetDir = join(root, "dist", wasmPackage.target);
	const missing = files.filter((file) => !existsSync(join(sourceDir, file)));
	if (missing.length > 0) {
		console.warn(
			`Skipping ${wasmPackage.label} Wasm core copy; run npm run build:wasm first. Missing: ${missing.join(", ")}`,
		);
		continue;
	}

	mkdirSync(targetDir, { recursive: true });
	for (const file of files) {
		copyFileSync(join(sourceDir, file), join(targetDir, file));
	}
}
