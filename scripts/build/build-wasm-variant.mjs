import { execFileSync } from "node:child_process";
import { renameSync } from "node:fs";
import { resolve } from "node:path";
import { root } from "../lib/paths.mjs";

export function buildWasmVariant(variant) {
	const args = [
		"build",
		"crates/wtfgif-core",
		"--target",
		variant.target,
		"--out-dir",
		variant.source,
		"--release",
	];
	if (variant.feature) {
		args.push("--", "--features", variant.feature);
	}

	const env = { ...process.env };
	if (variant.feature === "quality-only") {
		env.CARGO_PROFILE_RELEASE_OPT_LEVEL = "2";
	}
	if (variant.simd) {
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
		variant.source,
		"wtfgif_core_bg.wasm",
	);
	const optimizedWasmPath = `${wasmPath}.optimized`;
	const wasmOptArgs = [wasmPath];
	if (variant.feature === "quality-only") {
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
	execFileSync("wasm-opt", wasmOptArgs, {
		cwd: root,
		stdio: "inherit",
		shell: process.platform === "win32",
	});
	renameSync(optimizedWasmPath, wasmPath);

	console.log(
		`Built ${variant.simd ? "SIMD" : "scalar"} ${variant.feature ?? "full"} ${variant.target} Wasm -> ${resolve(root, "crates/wtfgif-core", variant.source)}`,
	);
}
