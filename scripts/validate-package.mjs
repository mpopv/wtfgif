import {
	mkdtempSync,
	mkdirSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const temporaryRoot = mkdtempSync(join(tmpdir(), "wtfgif-package-"));
const consumerDir = join(temporaryRoot, "consumer");

const run = (command, args, options = {}) =>
	execFileSync(command, args, {
		cwd: consumerDir,
		encoding: "utf8",
		stdio: ["ignore", "pipe", "pipe"],
		...options,
	});

const parsePackResult = (output) => {
	let jsonStart = output.lastIndexOf("[");
	while (jsonStart !== -1) {
		try {
			const result = JSON.parse(output.slice(jsonStart));
			if (Array.isArray(result)) {
				return result[0];
			}
		} catch {
			// npm 10 can print lifecycle output before the JSON payload.
		}
		jsonStart = output.lastIndexOf("[", jsonStart - 1);
	}
	throw new Error("npm pack did not return a JSON payload.");
};

try {
	const packJson = execFileSync(
		"npm",
		[
			"pack",
			"--silent",
			"--ignore-scripts",
			"--pack-destination",
			temporaryRoot,
			"--json",
		],
		{ cwd: root, encoding: "utf8" },
	);
	const packResult = parsePackResult(packJson);
	if (!packResult?.filename) {
		throw new Error("npm pack did not return a package filename.");
	}
	const tarball = resolve(temporaryRoot, packResult.filename);

	mkdirSync(consumerDir);
	writeFileSync(
		join(consumerDir, "package.json"),
		JSON.stringify({ name: "wtfgif-package-test", private: true }, null, 2),
	);
	run("npm", ["install", "--ignore-scripts", tarball]);

	const commonJsResult = JSON.parse(
		run("node", [
			"-e",
			`
				const wtfgif = require("wtfgif");
				const status = wtfgif.installWasmCoreBackend();
				const gif = wtfgif.encodeIndexedGifFrames({
					width: 2,
					height: 2,
					frames: new Uint8Array([0, 1, 1, 0]),
					palette: [0, 0xffffff],
					backend: "native",
				});
				const reader = new wtfgif.GifReader(gif);
				const result = {
					available: status.available,
					frames: reader.numFrames(),
					hasCompatibilityApi:
						typeof wtfgif.initializeWasmGlobally === "function" &&
						typeof wtfgif.getWasmStatus === "function" &&
						typeof wtfgif.cleanupWasm === "function",
				};
				reader.dispose();
				process.stdout.write(JSON.stringify(result));
			`,
		]),
	);
	if (
		!commonJsResult.available ||
		commonJsResult.frames !== 1 ||
		!commonJsResult.hasCompatibilityApi
	) {
		throw new Error(
			`CommonJS package validation failed: ${JSON.stringify(commonJsResult)}`,
		);
	}

	const esmResult = JSON.parse(
		run("node", [
			"--input-type=module",
			"-e",
			`
				globalThis.window = {};
				const wtfgif = await import("wtfgif");
				const status = wtfgif.installWasmCoreBackend();
				const gif = wtfgif.encodeIndexedGifFrames({
					width: 2,
					height: 2,
					frames: new Uint8Array([0, 1, 1, 0]),
					palette: [0, 0xffffff],
					backend: "native",
				});
				const reader = new wtfgif.GifReader(gif);
				const result = {
					available: status.available,
					frames: reader.numFrames(),
					browserGlobal: globalThis.window.wtfgif?.GifReader === wtfgif.GifReader,
				};
				reader.dispose();
				process.stdout.write(JSON.stringify(result));
			`,
		]),
	);
	if (
		!esmResult.available ||
		esmResult.frames !== 1 ||
		!esmResult.browserGlobal
	) {
		throw new Error(`ESM package validation failed: ${JSON.stringify(esmResult)}`);
	}

	const browserWasmResult = JSON.parse(
		run("node", [
			"--conditions=browser",
			"--input-type=module",
			"-e",
			`
				import { readFileSync } from "node:fs";
				import { fileURLToPath } from "node:url";
				const wtfgif = await import("wtfgif");
				wtfgif.cleanupWasm();
				const wasmUrl = import.meta.resolve("wtfgif/wasm-web/wasm");
				const wasmBytes = readFileSync(fileURLToPath(wasmUrl));
				await wtfgif.initializeWasmGlobally(wasmBytes);
				const status = wtfgif.installWasmCoreBackend();
				process.stdout.write(JSON.stringify({
					available: status.available,
					initialized: wtfgif.getWasmStatus().initialized,
				}));
			`,
		]),
	);
	if (!browserWasmResult.available || !browserWasmResult.initialized) {
		throw new Error(
			`Browser Wasm package validation failed: ${JSON.stringify(browserWasmResult)}`,
		);
	}

	const webWrapper = readFileSync(
		join(root, "dist", "wasm-web", "wtfgif_core.js"),
		"utf8",
	);
	if (
		webWrapper.includes('from "node:') ||
		webWrapper.includes("require(") ||
		webWrapper.includes("node:fs")
	) {
		throw new Error("Browser Wasm wrapper contains a Node-only dependency.");
	}

	console.log(
		"Packaged CommonJS, Node ESM, browser global, and browser Wasm validation passed.",
	);
} finally {
	rmSync(temporaryRoot, { recursive: true, force: true });
}
