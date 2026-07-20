import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { execFileSync } from "node:child_process";

const nodeRoot = dirname(dirname(process.execPath));
const outputDir = join(import.meta.dirname, "..", "native", "build");
mkdirSync(outputDir, { recursive: true });
execFileSync(
	"cargo",
	[
		"build",
		"--manifest-path",
		join(import.meta.dirname, "..", "crates", "wtfgif-core", "Cargo.toml"),
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
		join(import.meta.dirname, "..", "native", "wtfgif_native.c"),
		join(
			import.meta.dirname,
			"..",
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
