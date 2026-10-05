import { createHash } from "node:crypto";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { chromium, firefox, webkit } from "playwright";
import { encodeWtfgif, initializeAdapter } from "../bench/lib/adapters.mjs";
import { ALPHA_THRESHOLD, loadBenchmarkCorpus } from "../bench/lib/corpus.mjs";
import { decodeCompositedGif } from "../bench/lib/metrics.mjs";
import { startLocalHttpServer } from "../lib/http-server.mjs";
import { root } from "../lib/paths.mjs";

const outputDir = join(root, "output", "playwright");
mkdirSync(outputDir, { recursive: true });
await initializeAdapter("wtfgif");

function sha256(bytes) {
	return createHash("sha256").update(bytes).digest("hex");
}

const encoded = new Map();
for (const value of loadBenchmarkCorpus()) {
	const conformanceValue = { ...value, delay: 60_000 };
	const bytes = encodeWtfgif(conformanceValue, ALPHA_THRESHOLD);
	const decoded = decodeCompositedGif(bytes);
	const frameBytes = value.width * value.height * 4;
	const frameSha256 = Array.from({ length: decoded.frameCount }, (_, frame) =>
		sha256(
			decoded.pixels.subarray(frame * frameBytes, (frame + 1) * frameBytes),
		),
	);
	encoded.set(value.id, {
		bytes,
		expectedSha256: frameSha256[0],
		frameSha256,
		width: value.width,
		height: value.height,
	});
}

const server = await startLocalHttpServer((request, response) => {
	const pathname = new URL(request.url ?? "/", "http://localhost").pathname;
	const id = decodeURIComponent(pathname.slice(5));
	const value = encoded.get(id);
	if (pathname.startsWith("/gif/") && value) {
		response.writeHead(200, {
			"Content-Type": "image/gif",
			"Cache-Control": "no-store",
			"Content-Length": value.bytes.length,
		});
		response.end(value.bytes);
		return;
	}
	response.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
	response.end(
		"<!doctype html><meta charset=utf-8><title>wtfgif browser conformance</title><body></body>",
	);
});
const { origin } = server;

// Frames after the first depend on the previous canvas once the encoder
// stores only changed rectangles. Engines with WebCodecs' ImageDecoder also
// render and compare every composited frame, not just the first.
const engineNames = (
	process.env.BROWSER_CONFORMANCE_ENGINES ?? "chromium,firefox,webkit"
)
	.split(",")
	.map((name) => name.trim())
	.filter(Boolean);
const allBrowsers = { chromium, firefox, webkit };
const browsers = Object.fromEntries(
	engineNames.map((name) => {
		if (!(name in allBrowsers))
			throw new Error(`Unknown browser engine: ${name}`);
		return [name, allBrowsers[name]];
	}),
);
const everyFrameEngines = [];
try {
	for (const [name, browserType] of Object.entries(browsers)) {
		let browser;
		try {
			browser = await browserType.launch({ headless: true });
		} catch (error) {
			throw new Error(
				`${name} could not launch. Run "npx playwright install chromium firefox webkit". ${error.message}`,
			);
		}
		try {
			const page = await browser.newPage({
				viewport: { width: 900, height: 700 },
			});
			await page.goto(origin);
			for (const [id, expected] of encoded) {
				const actual = await page.evaluate(
					async ({ id, width, height }) => {
						const image = new Image();
						image.decoding = "sync";
						image.src = `/gif/${encodeURIComponent(id)}?cache=${Math.random()}`;
						await new Promise((resolve, reject) => {
							image.onload = resolve;
							image.onerror = () => reject(new Error(`could not load ${id}`));
						});
						const canvas = document.createElement("canvas");
						canvas.width = width;
						canvas.height = height;
						const context = canvas.getContext("2d", {
							willReadFrequently: true,
						});
						context.drawImage(image, 0, 0);
						const pixels = context.getImageData(0, 0, width, height).data;
						const digest = await crypto.subtle.digest("SHA-256", pixels);
						const hash = [...new Uint8Array(digest)]
							.map((byte) => byte.toString(16).padStart(2, "0"))
							.join("");
						document.body.replaceChildren(image);
						let frames = null;
						if (typeof ImageDecoder !== "undefined") {
							const response = await fetch(
								`/gif/${encodeURIComponent(id)}?frames=${Math.random()}`,
							);
							const decoder = new ImageDecoder({
								data: await response.arrayBuffer(),
								type: "image/gif",
							});
							await decoder.tracks.ready;
							frames = [];
							const frameCount = decoder.tracks.selectedTrack.frameCount;
							for (let frameIndex = 0; frameIndex < frameCount; frameIndex++) {
								const { image: frame } = await decoder.decode({
									frameIndex,
									completeFramesOnly: true,
								});
								context.clearRect(0, 0, width, height);
								context.drawImage(frame, 0, 0);
								frame.close();
								const framePixels = context.getImageData(
									0,
									0,
									width,
									height,
								).data;
								const frameDigest = await crypto.subtle.digest(
									"SHA-256",
									framePixels,
								);
								frames.push(
									[...new Uint8Array(frameDigest)]
										.map((byte) => byte.toString(16).padStart(2, "0"))
										.join(""),
								);
							}
							decoder.close();
						}
						return {
							hash,
							frames,
							naturalWidth: image.naturalWidth,
							naturalHeight: image.naturalHeight,
						};
					},
					{ id, width: expected.width, height: expected.height },
				);
				if (
					actual.naturalWidth !== expected.width ||
					actual.naturalHeight !== expected.height
				) {
					throw new Error(`${name}/${id}: browser dimensions differ`);
				}
				if (actual.hash !== expected.expectedSha256) {
					throw new Error(
						`${name}/${id}: canvas pixels differ from the independent decoder`,
					);
				}
				if (actual.frames) {
					if (actual.frames.length !== expected.frameSha256.length) {
						throw new Error(
							`${name}/${id}: browser decoded the wrong frame count`,
						);
					}
					actual.frames.forEach((hash, frame) => {
						if (hash !== expected.frameSha256[frame]) {
							throw new Error(
								`${name}/${id}: composited frame ${frame} differs from the independent decoder`,
							);
						}
					});
				}
			}
			if (
				[...encoded.keys()].length > 0 &&
				!everyFrameEngines.includes(name) &&
				(await page.evaluate(() => typeof ImageDecoder !== "undefined"))
			) {
				everyFrameEngines.push(name);
			}
			await page.screenshot({
				path: join(outputDir, `browser-conformance-${name}.png`),
			});
		} finally {
			await browser.close();
		}
	}
} finally {
	await server.close();
}

console.log(
	`Browser conformance passed: ${encoded.size} fixtures in ${Object.keys(browsers).join(", ")}; every composited frame checked in ${everyFrameEngines.join(", ") || "no engine"}`,
);
