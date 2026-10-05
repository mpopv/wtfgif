import { execFile, spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { cpus, platform, release } from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { promisify } from "node:util";
import { build } from "esbuild";
import { contentType, startLocalHttpServer } from "../lib/http-server.mjs";
import { root } from "../lib/paths.mjs";
import { withTemporaryDirectory } from "../lib/temporary.mjs";
import { median, percentileNearest } from "./lib/metrics.mjs";
import { renderEncoderRaceChart } from "./render-encoder-race-chart.mjs";

const execFileAsync = promisify(execFile);
const biomeCli = createRequire(import.meta.url).resolve(
	"@biomejs/biome/bin/biome",
);
const iterations = Number(process.env.BENCH_RACE_ITERATIONS ?? 15);
const timeoutMs = Number(process.env.BENCH_RACE_TIMEOUT_MS ?? 60_000);
const saveOutputs = process.env.BENCH_RACE_SAVE_GIFS === "1";
const writeReceipt = process.env.BENCH_WRITE_RECEIPT !== "0";
const WIDTH = 128;
const HEIGHT = 128;
const FRAME_COUNT = 8;
const FRAME_BYTES = WIDTH * HEIGHT * 4;
const ALPHA_THRESHOLD = 179;

if (!Number.isInteger(iterations) || iterations < 1) {
	throw new Error("BENCH_RACE_ITERATIONS must be a positive integer");
}

const implementations = [
	{
		id: "wtfgif",
		label: "wtfgif",
		packageName: "wtfgif",
		palette: "adaptive global palette over all frames",
	},
	{
		id: "gifenc",
		label: "gifenc",
		packageName: "gifenc",
		palette: "rgb565 global palette over all opaque pixels",
	},
	{
		id: "modernGif",
		label: "modern-gif",
		packageName: "modern-gif",
		palette: "adaptive global palette over all frames",
	},
	{
		id: "omggif",
		label: "image-q + omggif",
		packageName: "omggif",
		palette: "image-q rgbquant global palette over all opaque pixels",
	},
	{
		id: "gif.js",
		label: "gif.js",
		packageName: "gif.js",
		palette: "NeuQuant local palette per frame, quality 1",
	},
	{
		id: "gif.js.optimized",
		label: "gif.js.optimized",
		packageName: "gif.js.optimized",
		palette: "NeuQuant local palette per frame, quality 1",
	},
];

function findChrome() {
	const candidates = [
		process.env.CHROME_PATH,
		"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
		"/Applications/Chromium.app/Contents/MacOS/Chromium",
		"/usr/bin/google-chrome",
		"/usr/bin/chromium",
	].filter(Boolean);
	const chrome = candidates.find((candidate) => existsSync(candidate));
	if (!chrome) {
		throw new Error("Chrome not found; set CHROME_PATH to its executable");
	}
	return chrome;
}

function packageVersion(packageName) {
	const file =
		packageName === "wtfgif"
			? path.join(root, "package.json")
			: path.join(root, "node_modules", packageName, "package.json");
	return readFile(file, "utf8").then((source) => JSON.parse(source).version);
}

const chrome = findChrome();
const [{ stdout: commit }, { stdout: status }, packageLock] = await Promise.all(
	[
		execFileAsync("git", ["rev-parse", "HEAD"], { cwd: root }),
		execFileAsync("git", ["status", "--porcelain"], { cwd: root }),
		readFile(path.join(root, "package-lock.json")),
	],
);
const provenance = {
	commit: commit.trim(),
	dirty: status.trim() !== "",
	packageLockSha256: createHash("sha256").update(packageLock).digest("hex"),
	packageVersion: await packageVersion("wtfgif"),
};
await withTemporaryDirectory("wtfgif-encoder-race-", async (temporary) => {
	const bundle = path.join(temporary, "race.mjs");

	await build({
		bundle: true,
		entryPoints: [
			path.join(root, "scripts", "bench", "encoder-race-browser.mjs"),
		],
		external: ["/dist/*"],
		format: "esm",
		mainFields: ["browser", "main", "module"],
		outfile: bundle,
		platform: "browser",
		target: ["chrome120"],
	});

	const html = `<!doctype html>
<meta charset="utf-8">
<title>wtfgif encoder race</title>
<script src="/vendor/gif.js"></script>
<script>globalThis.GifJsOriginal = globalThis.GIF;</script>
<script src="/vendor/gif.optimized.js"></script>
<script>globalThis.GifJsOptimized = globalThis.GIF;</script>
<script type="module" src="/race.mjs"></script>`;

	const routes = new Map([
		["/race.mjs", bundle],
		[
			"/fixture.rgba",
			path.join(root, "test", "rgba", "makeemoji-128x128x8.rgba"),
		],
		[
			"/vendor/gif.js",
			path.join(root, "node_modules", "gif.js", "dist", "gif.js"),
		],
		[
			"/vendor/gif.worker.js",
			path.join(root, "node_modules", "gif.js", "dist", "gif.worker.js"),
		],
		[
			"/vendor/gif.optimized.js",
			path.join(root, "node_modules", "gif.js.optimized", "dist", "gif.js"),
		],
		[
			"/vendor/gif.optimized.worker.js",
			path.join(
				root,
				"node_modules",
				"gif.js.optimized",
				"dist",
				"gif.worker.js",
			),
		],
	]);

	const pending = new Map();
	const server = await startLocalHttpServer(async (request, response) => {
		response.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
		response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
		response.setHeader("Cross-Origin-Resource-Policy", "same-origin");
		if (request.method === "POST" && request.url === "/result") {
			const chunks = [];
			for await (const chunk of request) chunks.push(chunk);
			const result = JSON.parse(Buffer.concat(chunks).toString("utf8"));
			const waiter = pending.get(result.token);
			if (waiter) {
				pending.delete(result.token);
				waiter.resolve(result);
			}
			response.writeHead(204).end();
			return;
		}

		const pathname = new URL(request.url ?? "/", "http://localhost").pathname;
		if (pathname === "/") {
			response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
			response.end(html);
			return;
		}
		const file = pathname.startsWith("/dist/")
			? path.join(root, pathname.slice(1))
			: routes.get(pathname);
		if (!file || (!file.startsWith(root) && file !== bundle)) {
			response.writeHead(404).end("Not found");
			return;
		}
		response.writeHead(200, { "content-type": contentType(file) });
		response.end(await readFile(file));
	});

	async function runInFreshChrome(implementation, run) {
		return withTemporaryDirectory("wtfgif-race-chrome-", async (profile) => {
			const token = randomUUID();
			const url = new URL(`${server.origin}/`);
			url.searchParams.set("implementation", implementation);
			url.searchParams.set("run", String(run));
			url.searchParams.set("token", token);
			if (saveOutputs && run === 0) url.searchParams.set("includeOutput", "1");

			let stderr = "";
			let child;
			const result = new Promise((resolve, reject) => {
				pending.set(token, { reject, resolve });
				child = spawn(
					chrome,
					[
						"--headless=new",
						"--disable-background-networking",
						"--disable-component-update",
						"--disable-default-apps",
						"--disable-gpu",
						"--disable-sync",
						"--metrics-recording-only",
						"--mute-audio",
						"--no-default-browser-check",
						"--no-first-run",
						"--remote-debugging-port=0",
						`--user-data-dir=${profile}`,
						url.href,
					],
					{ stdio: ["ignore", "ignore", "pipe"] },
				);
				child.stderr.on("data", (chunk) => {
					stderr += chunk.toString();
					if (stderr.length > 12_000) stderr = stderr.slice(-12_000);
				});
				child.once("error", reject);
				child.once("exit", (code, signal) => {
					if (pending.delete(token)) {
						reject(
							new Error(
								`Chrome exited before ${implementation} returned (${code ?? signal})\n${stderr}`,
							),
						);
					}
				});
			});
			const timeout = setTimeout(() => {
				const waiter = pending.get(token);
				if (waiter) {
					pending.delete(token);
					waiter.reject(
						new Error(`${implementation} exceeded ${timeoutMs} ms\n${stderr}`),
					);
				}
			}, timeoutMs);

			try {
				const value = await result;
				if (value.error) {
					throw new Error(
						`${implementation}: ${value.error}\n${value.stack ?? ""}`,
					);
				}
				return value;
			} finally {
				clearTimeout(timeout);
				child?.kill("SIGTERM");
			}
		});
	}

	const samples = new Map(
		implementations.map((implementation) => [implementation.id, []]),
	);
	console.log(
		`Browser arbitrary-RGBA encoder race: ${iterations} fresh Chrome processes per implementation, zero fixture-derived warmups`,
	);

	try {
		for (let run = 0; run < iterations; run += 1) {
			for (let offset = 0; offset < implementations.length; offset += 1) {
				const candidate =
					implementations[(run + offset) % implementations.length];
				const started = performance.now();
				const result = await runInFreshChrome(candidate.id, run);
				if (result.outputBase64) {
					const outputDirectory = path.join(root, "output", "encoder-race");
					await mkdir(outputDirectory, { recursive: true });
					await writeFile(
						path.join(outputDirectory, `${candidate.id}.gif`),
						Buffer.from(result.outputBase64, "base64"),
					);
					delete result.outputBase64;
				}
				samples.get(candidate.id).push(result);
				const current = samples
					.get(candidate.id)
					.map((sample) => sample.elapsedMs);
				console.log(
					`${candidate.label.padEnd(20)} run ${(run + 1).toString().padStart(2)}/${iterations}: ${result.elapsedMs.toFixed(3).padStart(9)} ms · median ${median(current).toFixed(3).padStart(9)} ms · ${(performance.now() - started).toFixed(0)} ms wall`,
				);
			}
		}

		const versions = new Map(
			await Promise.all(
				implementations.map(async (implementation) => [
					implementation.id,
					await packageVersion(implementation.packageName),
				]),
			),
		);
		const { stdout: browserVersion } = await execFileAsync(chrome, [
			"--version",
		]);
		const results = implementations.map((implementation) => {
			const implementationSamples = samples.get(implementation.id);
			const timings = implementationSamples.map((sample) => sample.elapsedMs);
			const bytes = implementationSamples.map((sample) => sample.bytes);
			const psnr = implementationSamples.map((sample) => sample.psnrDb);
			const alpha = implementationSamples.map(
				(sample) => sample.alphaAccuracyPercent,
			);
			const delays = implementationSamples.map((sample) => sample.delays);
			if (
				!bytes.every((value) => value === bytes[0]) ||
				!psnr.every((value) => value === psnr[0]) ||
				!alpha.every((value) => value === alpha[0]) ||
				!delays.every(
					(value) => JSON.stringify(value) === JSON.stringify(delays[0]),
				)
			) {
				throw new Error(
					`${implementation.label} output changed across fresh runs`,
				);
			}
			return {
				alphaAccuracyPercent: alpha[0],
				bytes: bytes[0],
				delays: delays[0],
				id: implementation.id,
				label: implementation.label,
				medianMs: median(timings),
				p25Ms: percentileNearest(timings, 0.25),
				p75Ms: percentileNearest(timings, 0.75),
				palette: implementation.palette,
				psnrDb: psnr[0],
				samplesMs: timings,
				version: versions.get(implementation.id),
			};
		});
		const wtfgifMs = results.find((result) => result.id === "wtfgif").medianMs;
		for (const result of results) {
			result.slowerThanWtfgif = result.medianMs / wtfgifMs;
		}

		const receipt = {
			boundary:
				"First user-input encode from arbitrary RGBA after package loading, source-independent wtfgif Wasm runtime preparation, and 64 MiB unrelated-memory cache eviction; zero fixture-derived warmups; includes palette creation, pixel mapping, compression, and output assembly",
			environment: {
				browser: browserVersion.trim(),
				...provenance,
				cpu: cpus()[0]?.model ?? "unknown CPU",
				iterations,
				node: process.version,
				os: `${platform()} ${release()}`,
			},
			fixture: {
				alphaThreshold: ALPHA_THRESHOLD,
				bytes: FRAME_COUNT * FRAME_BYTES,
				cacheEvictionBytes: 64 * 1024 * 1024,
				cacheQuiescence:
					"One requestAnimationFrame after eviction; no encoder operation or fixture access occurs in the callback.",
				frames: FRAME_COUNT,
				height: HEIGHT,
				inputLayout:
					"wtfgif receives eight independently allocated RGBA typed arrays created before cache eviction and timing",
				name: "real MakeEmoji images",
				width: WIDTH,
			},
			generatedAt: new Date().toISOString(),
			results,
			schemaVersion: 2,
		};

		if (writeReceipt) {
			const benchmarksDirectory = path.join(root, "benchmarks");
			const docsDirectory = path.join(root, "docs");
			await Promise.all([
				mkdir(benchmarksDirectory, { recursive: true }),
				mkdir(docsDirectory, { recursive: true }),
			]);
			const receiptPath = path.join(benchmarksDirectory, "encoder-race.json");
			await Promise.all([
				writeFile(receiptPath, `${JSON.stringify(receipt, null, "\t")}\n`),
				writeFile(
					path.join(docsDirectory, "encoder-race.svg"),
					renderEncoderRaceChart(receipt),
				),
			]);
			await execFileAsync(process.execPath, [
				biomeCli,
				"format",
				"--write",
				receiptPath,
			]);
		}

		console.log(
			"\nimplementation\tmedian ms\tslower than wtfgif\tbytes\tPSNR dB\talpha match",
		);
		for (const result of results.toSorted(
			(left, right) => left.medianMs - right.medianMs,
		)) {
			console.log(
				[
					result.label,
					result.medianMs.toFixed(3),
					`${result.slowerThanWtfgif.toFixed(2)}x`,
					result.bytes,
					result.psnrDb.toFixed(2),
					`${result.alphaAccuracyPercent.toFixed(3)}%`,
				].join("\t"),
			);
		}
		console.log(
			writeReceipt
				? "\nWrote benchmarks/encoder-race.json and docs/encoder-race.svg"
				: "\nReceipt writing disabled",
		);
	} finally {
		await server.close();
	}
});
