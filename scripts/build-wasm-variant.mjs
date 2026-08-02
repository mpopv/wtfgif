import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const [target, outputDirectory, featureOrVariant, explicitVariant] =
	process.argv.slice(2);
const feature = featureOrVariant === "simd" ? undefined : featureOrVariant;
const variant = featureOrVariant === "simd" ? "simd" : explicitVariant;

if (!target || !outputDirectory || !["nodejs", "web"].includes(target)) {
	throw new Error(
		"Usage: build-wasm-variant.mjs <nodejs|web> <output-directory> [encode-only] [simd]",
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
if (variant === "simd") {
	const currentFlags = env.RUSTFLAGS?.trim();
	env.RUSTFLAGS = [currentFlags, "-C target-feature=+simd128"]
		.filter(Boolean)
		.join(" ");
}

execFileSync("wasm-pack", args, {
	cwd: root,
	env,
	stdio: "inherit",
});

console.log(
	`Built ${variant === "simd" ? "SIMD" : "scalar"} ${feature ?? "full"} ${target} Wasm -> ${resolve(root, "crates/wtfgif-core", outputDirectory)}`,
);
