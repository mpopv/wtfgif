import { existsSync } from "node:fs";
import path from "node:path";

export const wasmPackageFiles = [
	"wtfgif_core.d.ts",
	"wtfgif_core.js",
	"wtfgif_core_bg.wasm",
	"wtfgif_core_bg.wasm.d.ts",
];

export const wasmVariants = [
	{
		id: "full-node-scalar",
		target: "nodejs",
		source: "pkg",
		dist: "wasm-core",
		label: "Node",
		moduleType: "commonjs",
		feature: null,
		simd: false,
	},
	{
		id: "full-web-scalar",
		target: "web",
		source: "pkg-web",
		dist: "wasm-web",
		label: "browser",
		moduleType: "module",
		feature: null,
		simd: false,
	},
	{
		id: "full-node-simd",
		target: "nodejs",
		source: "pkg-simd",
		dist: "wasm-core-simd",
		label: "Node SIMD",
		moduleType: "commonjs",
		feature: null,
		simd: true,
	},
	{
		id: "full-web-simd",
		target: "web",
		source: "pkg-web-simd",
		dist: "wasm-web-simd",
		label: "browser SIMD",
		moduleType: "module",
		feature: null,
		simd: true,
	},
	{
		id: "quality-node-scalar",
		target: "nodejs",
		source: "pkg-quality",
		dist: "wasm-quality",
		label: "quality encode Node",
		moduleType: "commonjs",
		feature: "quality-only",
		simd: false,
	},
	{
		id: "quality-web-scalar",
		target: "web",
		source: "pkg-quality-web",
		dist: "wasm-quality-web",
		label: "quality encode browser",
		moduleType: "module",
		feature: "quality-only",
		simd: false,
	},
	{
		id: "quality-node-simd",
		target: "nodejs",
		source: "pkg-quality-simd",
		dist: "wasm-quality-simd",
		label: "quality encode Node SIMD",
		moduleType: "commonjs",
		feature: "quality-only",
		simd: true,
	},
	{
		id: "quality-web-simd",
		target: "web",
		source: "pkg-quality-web-simd",
		dist: "wasm-quality-web-simd",
		label: "quality encode browser SIMD",
		moduleType: "module",
		feature: "quality-only",
		simd: true,
	},
];

export function selectWasmVariants(ids) {
	if (ids.length === 0 || ids.includes("--all")) return wasmVariants;
	const selected = ids.map((id) => {
		const variant = wasmVariants.find((candidate) => candidate.id === id);
		if (!variant) {
			throw new Error(
				`Unknown Wasm variant ${id}. Expected one of: ${wasmVariants.map((value) => value.id).join(", ")}`,
			);
		}
		return variant;
	});
	return [...new Set(selected)];
}

export function assertWasmVariantArtifacts(
	root,
	location,
	variants = wasmVariants,
) {
	for (const variant of variants) {
		const directory =
			location === "source"
				? path.join(root, "crates", "wtfgif-core", variant.source)
				: path.join(root, "dist", variant.dist);
		const missing = wasmPackageFiles.filter(
			(file) => !existsSync(path.join(directory, file)),
		);
		if (missing.length > 0) {
			throw new Error(
				`${variant.label} ${location} Wasm artifact is incomplete: ${missing.join(", ")}`,
			);
		}
	}
}
