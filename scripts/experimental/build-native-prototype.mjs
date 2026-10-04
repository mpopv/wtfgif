import { execFileSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { root } from "../lib/paths.mjs";

const nodeRoot = dirname(dirname(process.execPath));
const outputDir = join(root, "native", "build");
mkdirSync(outputDir, { recursive: true });
execFileSync(
	"cargo",
	[
		"build",
		"--manifest-path",
		join(root, "crates", "wtfgif-core", "Cargo.toml"),
		"--release",
	],
	{ stdio: "inherit" },
);

execFileSync(
	"clang",
	[
		"-O3",
		"-flto",
		"-bundle",
		"-undefined",
		"dynamic_lookup",
		"-I",
		join(nodeRoot, "include", "node"),
		join(root, "native", "wtfgif_native.c"),
		join(
			root,
			"crates",
			"wtfgif-core",
			"target",
			"release",
			"libwtfgif_core.a",
		),
		"-o",
		join(outputDir, "wtfgif_native.node"),
	],
	{ stdio: "inherit" },
);
