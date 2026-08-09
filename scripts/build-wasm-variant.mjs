import { execFileSync } from "node:child_process";
import { renameSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const [target, outputDirectory, featureOrVariant, explicitVariant] =
	process.argv.slice(2);
const feature = featureOrVariant === "simd" ? undefined : featureOrVariant;
const variant = featureOrVariant === "simd" ? "simd" : explicitVariant;

if (!target || !outputDirectory || !["nodejs", "web"].includes(target)) {
	throw new Error(
		"Usage: build-wasm-variant.mjs <nodejs|web> <output-directory> [encode-only|quality-only] [simd]",
	);
}

const args = [
	"build",
	"crates/wtfgif-core",
	"--target",
	target,
	"--out-dir",
	outputDirectory,
	"--release",
];
if (feature) {
	args.push("--", "--features", feature);
}

const env = { ...process.env };
if (feature === "quality-only") {
	env.CARGO_PROFILE_RELEASE_OPT_LEVEL = "2";
}
if (variant === "simd") {
	const currentFlags = env.RUSTFLAGS?.trim();
	env.RUSTFLAGS = [
		currentFlags,
		"-C target-feature=+simd128,+bulk-memory,+nontrapping-fptoint",
	]
		.filter(Boolean)
		.join(" ");
}

execFileSync("wasm-pack", args, {
	cwd: root,
	env,
	stdio: "inherit",
});

const wasmPath = resolve(
	root,
	"crates/wtfgif-core",
	outputDirectory,
	"wtfgif_core_bg.wasm",
);
const optimizedWasmPath = `${wasmPath}.optimized`;
const wasmOptArgs = [wasmPath];
if (feature === "quality-only") {
	wasmOptArgs.push(
		"--no-inline=*index_rgba_frames_quality_high_res*",
		"--no-inline=*index_rgba_frames_quality_u32*",
		"--no-inline=*index_rgba_frames_quality_u64*",
		"--no-inline=*quality_prefers_high_precision_histogram*",
		"--no-inline=*build_quality_index_plan_from_colors*",
		"--gufa",
		"--dae-optimizing",
		"--inlining-optimizing",
		"--converge",
	);
} else {
	wasmOptArgs.push("--strip-debug");
}
wasmOptArgs.push("-o", optimizedWasmPath);
execFileSync("wasm-opt", wasmOptArgs, { cwd: root, stdio: "inherit" });
renameSync(optimizedWasmPath, wasmPath);

console.log(
	`Built ${variant === "simd" ? "SIMD" : "scalar"} ${feature ?? "full"} ${target} Wasm -> ${resolve(root, "crates/wtfgif-core", outputDirectory)}`,
);
