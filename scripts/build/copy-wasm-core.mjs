import { copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { root } from "../lib/paths.mjs";
import { wasmPackageFiles, wasmVariants } from "./wasm-variants.mjs";

for (const wasmPackage of wasmVariants) {
	const sourceDir = join(root, "crates", "wtfgif-core", wasmPackage.source);
	const targetDir = join(root, "dist", wasmPackage.dist);
	const missing = wasmPackageFiles.filter(
		(file) => !existsSync(join(sourceDir, file)),
	);
	if (missing.length > 0) {
		console.warn(
			`Skipping ${wasmPackage.label} Wasm core copy; run npm run build:wasm first. Missing: ${missing.join(", ")}`,
		);
		continue;
	}

	mkdirSync(targetDir, { recursive: true });
	for (const file of wasmPackageFiles) {
		copyFileSync(join(sourceDir, file), join(targetDir, file));
	}
	writeFileSync(
		join(targetDir, "package.json"),
		`${JSON.stringify({ type: wasmPackage.moduleType }, null, 2)}\n`,
	);
}
