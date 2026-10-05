import { defineConfig } from "tsup";

export default defineConfig({
	entry: ["src/index.ts", "src/encode.ts", "src/global.ts"],
	format: ["esm", "cjs"],
	dts: false,
	sourcemap: true,
	minify: true,
	treeshake: true,
	external: ["wtfgif"],
	// Keep the encode subpath self-contained. With code splitting enabled,
	// tsup puts the full package's decoder runtime in a shared chunk, so merely
	// importing `wtfgif/encode` still evaluates and preloads the larger Wasm.
	splitting: false,
	clean: true,
	outDir: "dist",
	outExtension({ format }) {
		return { js: format === "esm" ? ".mjs" : ".cjs" };
	},
});
