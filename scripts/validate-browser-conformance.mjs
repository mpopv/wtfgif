import { createHash } from "node:crypto";
import { mkdirSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "playwright";
import { encodeWtfgif, initializeAdapter } from "./benchmark/adapters.mjs";
import { ALPHA_THRESHOLD, loadBenchmarkCorpus } from "./benchmark/corpus.mjs";
import { decodeCompositedGif } from "./benchmark/metrics.mjs";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
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
	const firstFrame = decoded.pixels.subarray(0, value.width * value.height * 4);
	encoded.set(value.id, {
		bytes,
		expectedSha256: sha256(firstFrame),
		width: value.width,
		height: value.height,
	});
}

const server = createServer((request, response) => {
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
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
const origin = `http://127.0.0.1:${address.port}`;

const browsers = { chromium, firefox, webkit };
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
						return {
							hash,
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
			}
			await page.screenshot({
				path: join(outputDir, `browser-conformance-${name}.png`),
			});
		} finally {
			await browser.close();
		}
	}
} finally {
	await new Promise((resolve, reject) =>
		server.close((error) => (error ? reject(error) : resolve())),
	);
}

console.log(
	`Browser conformance passed: ${encoded.size} fixtures in Chromium, Firefox, and WebKit`,
);
