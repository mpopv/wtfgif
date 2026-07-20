import { GifReader, GifWriter } from "./vendor/omggif.mjs";
import {
	decodeGifFramesRgba,
	getFastBackendStatus,
	initializeWasmGlobally,
	remuxGifPixelPerfect,
} from "./vendor/wtfgif.mjs";

const fixtures = [
	{
		id: "18d",
		name: "18d",
		file: "18d.gif",
		source: "18d30677-d255-4cc9-9933-c8d35306c1d5.gif",
		proof: 738.64,
	},
	{
		id: "clap",
		name: "Clap",
		file: "clap.gif",
		source: "Clap-1x.gif",
		proof: 309.84,
	},
	{
		id: "homer",
		name: "Homer",
		file: "homer.gif",
		source: "Disappear Homer Simpson GIF.gif",
		proof: 983.53,
	},
	{
		id: "chipmunk",
		name: "Chipmunk",
		file: "chipmunk.gif",
		source: "Dramatic Chipmunk GIF.gif",
		proof: 768.77,
	},
	{
		id: "gigachad",
		name: "GIGACHAD",
		file: "gigachad.gif",
		source: "GIGACHAD-4x.gif",
		proof: 607.23,
	},
	{
		id: "nodders",
		name: "NODDERS",
		file: "nodders.gif",
		source: "NODDERS-2x.gif",
		proof: 512.23,
	},
	{
		id: "proud",
		name: "Proud",
		file: "proud.gif",
		source: "Proud Of You Yes GIF.gif",
		proof: 681.13,
	},
	{
		id: "catjam",
		name: "catJAM",
		file: "catjam.gif",
		source: "catJAM-3x.gif",
		proof: 519.37,
	},
	{
		id: "excuseme",
		name: "excuse me",
		file: "excuseme.gif",
		source: "excuseme.gif",
		proof: 480.06,
	},
	{
		id: "party-blob",
		name: "party blob",
		file: "party-blob.gif",
		source: "party_blob.gif",
		proof: 955.88,
	},
	{
		id: "partyparrot",
		name: "partyparrot",
		file: "partyparrot.gif",
		source: "partyparrot.gif",
		proof: 244.09,
	},
	{
		id: "tenor",
		name: "tenor",
		file: "tenor.gif",
		source: "tenor.gif",
		proof: 1238.09,
	},
];

const elements = {
	arena: document.querySelector("#arena"),
	countdown: document.querySelector("#countdown"),
	fixtureMeta: document.querySelector("#fixture-meta"),
	fixtureName: document.querySelector("#fixture-name"),
	fixturePreview: document.querySelector("#fixture-preview"),
	fixtureStrip: document.querySelector("#fixture-strip"),
	liveSpeed: document.querySelector("#live-speed"),
	omgTime: document.querySelector("#omg-time"),
	omgTrack: document.querySelector("#omg-track"),
	omgVerdict: document.querySelector("#omg-verdict"),
	pixelVerdict: document.querySelector("#pixel-verdict"),
	proofSpeed: document.querySelector("#proof-speed"),
	raceButton: document.querySelector("#race-button"),
	resultCallout: document.querySelector("#result-callout"),
	wasmStatus: document.querySelector("#wasm-status"),
	wasmStatusText: document.querySelector("#wasm-status-text"),
	wtfTime: document.querySelector("#wtf-time"),
	wtfTrack: document.querySelector("#wtf-track"),
	wtfVerdict: document.querySelector("#wtf-verdict"),
};

let selectedFixture = fixtures[2];
let selectedBytes = null;
let raceCount = 0;
let busy = false;

const nextFrame = () =>
	new Promise((resolve) => requestAnimationFrame(() => resolve()));

const sleep = (duration) =>
	new Promise((resolve) => window.setTimeout(resolve, duration));

function formatBytes(bytes) {
	if (bytes < 1024) {
		return `${bytes} B`;
	}
	if (bytes < 1024 * 1024) {
		return `${(bytes / 1024).toFixed(bytes < 10 * 1024 ? 1 : 0)} KB`;
	}
	return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function formatTime(milliseconds) {
	if (milliseconds < 0.01) {
		return "<0.01 ms";
	}
	if (milliseconds < 1) {
		return `${milliseconds.toFixed(3)} ms`;
	}
	if (milliseconds < 100) {
		return `${milliseconds.toFixed(2)} ms`;
	}
	return `${milliseconds.toFixed(1)} ms`;
}

function renderFixtureButtons() {
	const fragment = document.createDocumentFragment();
	for (const fixture of fixtures) {
		const button = document.createElement("button");
		button.className = "fixture-button";
		button.type = "button";
		button.dataset.fixture = fixture.id;
		button.setAttribute(
			"aria-pressed",
			String(fixture.id === selectedFixture.id),
		);
		button.innerHTML = `
			<img src="./gifs/${fixture.file}" alt="" loading="lazy" />
			<span>${fixture.name}</span>
		`;
		button.addEventListener("click", () => selectFixture(fixture));
		fragment.append(button);
	}
	elements.fixtureStrip.append(fragment);
}

async function selectFixture(fixture) {
	if (busy || fixture.id === selectedFixture.id) {
		return;
	}
	selectedFixture = fixture;
	selectedBytes = null;
	for (const button of elements.fixtureStrip.querySelectorAll("button")) {
		button.setAttribute(
			"aria-pressed",
			String(button.dataset.fixture === fixture.id),
		);
	}
	await loadFixture();
}

function resetRaceBoard() {
	elements.omgTime.textContent = "—";
	elements.wtfTime.textContent = "—";
	elements.omgTrack.style.width = "0%";
	elements.wtfTrack.style.width = "0%";
	elements.omgVerdict.textContent = "Waiting on the grid";
	elements.wtfVerdict.textContent = "Rust/Wasm ready";
	elements.liveSpeed.textContent = "RACE!";
	elements.pixelVerdict.textContent =
		"Outputs will be verified pixel by pixel.";
}

async function loadFixture() {
	elements.raceButton.disabled = true;
	resetRaceBoard();
	elements.fixtureName.textContent = selectedFixture.name;
	elements.fixtureMeta.textContent = "Loading…";
	elements.fixturePreview.alt = `${selectedFixture.name} animated GIF fixture`;
	elements.fixturePreview.src = `./gifs/${selectedFixture.file}`;
	elements.proofSpeed.textContent = `${selectedFixture.proof.toLocaleString(
		undefined,
		{ minimumFractionDigits: 2, maximumFractionDigits: 2 },
	)}×`;

	try {
		const response = await fetch(`./gifs/${selectedFixture.file}`);
		if (!response.ok) {
			throw new Error(`GIF fetch failed (${response.status})`);
		}
		selectedBytes = new Uint8Array(await response.arrayBuffer());
		const reader = new GifReader(selectedBytes);
		elements.fixtureMeta.textContent = `${reader.width}×${reader.height} / ${reader.numFrames()} frames / ${formatBytes(selectedBytes.length)}`;
		elements.raceButton.disabled = false;
	} catch (error) {
		elements.fixtureMeta.textContent = "Fixture failed to load";
		elements.pixelVerdict.textContent =
			error instanceof Error ? error.message : String(error);
	}
}

function reencodeOmggif(data) {
	const reader = new GifReader(data);
	let totalFramePixels = 0;
	for (let frame = 0; frame < reader.numFrames(); frame += 1) {
		const info = reader.frameInfo(frame);
		totalFramePixels += info.width * info.height;
	}

	const output = new Uint8Array(totalFramePixels * 2 + data.length + 4096);
	const writer = new GifWriter(output, reader.width, reader.height, {
		loop: reader.loopCount(),
	});
	const rgba = new Uint8Array(reader.width * reader.height * 4);
	const paletteCache = new Map();

	for (let frame = 0; frame < reader.numFrames(); frame += 1) {
		const info = reader.frameInfo(frame);
		const paletteKey = `${info.palette_offset}:${info.palette_size}`;
		let paletteInfo = paletteCache.get(paletteKey);
		if (!paletteInfo) {
			const palette = new Array(info.palette_size);
			const colorToIndex = new Map();
			for (let index = 0; index < info.palette_size; index += 1) {
				const offset = info.palette_offset + index * 3;
				const color =
					(data[offset] << 16) |
					(data[offset + 1] << 8) |
					data[offset + 2];
				palette[index] = color;
				if (!colorToIndex.has(color)) {
					colorToIndex.set(color, index);
				}
			}
			paletteInfo = { palette, colorToIndex };
			paletteCache.set(paletteKey, paletteInfo);
		}

		rgba.fill(0);
		reader.decodeAndBlitFrameRGBA(frame, rgba);
		const indices = new Uint8Array(info.width * info.height);
		let outputIndex = 0;
		for (let y = 0; y < info.height; y += 1) {
			let offset = ((info.y + y) * reader.width + info.x) * 4;
			for (let x = 0; x < info.width; x += 1) {
				if (rgba[offset + 3] === 0 && info.transparent_index !== null) {
					indices[outputIndex] = info.transparent_index;
				} else {
					const color =
						(rgba[offset] << 16) |
						(rgba[offset + 1] << 8) |
						rgba[offset + 2];
					const index = paletteInfo.colorToIndex.get(color);
					if (index === undefined) {
						throw new Error("Decoded color is absent from the frame palette");
					}
					indices[outputIndex] = index;
				}
				outputIndex += 1;
				offset += 4;
			}
		}

		writer.addFrame(info.x, info.y, info.width, info.height, indices, {
			palette: paletteInfo.palette,
			delay: info.delay,
			disposal: info.disposal,
			transparent: info.transparent_index,
		});
	}

	return output.slice(0, writer.end());
}

function measure(operation) {
	const targetDuration = 35;
	const maximumIterations = 4096;
	let iterations = 1;
	let totalDuration = 0;
	let output;

	for (;;) {
		const started = performance.now();
		for (let iteration = 0; iteration < iterations; iteration += 1) {
			output = operation();
		}
		totalDuration = performance.now() - started;
		if (
			totalDuration >= targetDuration ||
			iterations >= maximumIterations
		) {
			break;
		}
		const projected = Math.ceil(
			(iterations * targetDuration) / Math.max(totalDuration, 0.1),
		);
		iterations = Math.min(
			maximumIterations,
			Math.max(iterations * 2, projected),
		);
	}

	return {
		duration: totalDuration / iterations,
		iterations,
		output,
	};
}

function decodedCopy(data) {
	const decoded = decodeGifFramesRgba(data);
	return {
		width: decoded.width,
		height: decoded.height,
		frameCount: decoded.frameCount,
		pixels: new Uint8Array(decoded.pixels),
	};
}

function assertPixelsEqual(expected, actual, label) {
	if (
		expected.width !== actual.width ||
		expected.height !== actual.height ||
		expected.frameCount !== actual.frameCount ||
		expected.pixels.length !== actual.pixels.length
	) {
		throw new Error(`${label} changed the animation structure`);
	}
	for (let index = 0; index < expected.pixels.length; index += 1) {
		if (expected.pixels[index] !== actual.pixels[index]) {
			throw new Error(`${label} changed RGBA byte ${index.toLocaleString()}`);
		}
	}
}

async function verifyOutputs(source, omggifOutput, wtfgifOutput) {
	const original = decodedCopy(source);
	await nextFrame();
	const omggif = decodedCopy(omggifOutput);
	assertPixelsEqual(original, omggif, "omggif");
	await nextFrame();
	const wtfgif = decodedCopy(wtfgifOutput);
	assertPixelsEqual(original, wtfgif, "wtfgif");
	return original.pixels.length;
}

async function showCountdown() {
	for (const value of ["3", "2", "1", "GO"]) {
		elements.countdown.textContent = value;
		elements.countdown.classList.remove("is-visible");
		void elements.countdown.offsetWidth;
		elements.countdown.classList.add("is-visible");
		await sleep(value === "GO" ? 360 : 470);
	}
	elements.countdown.classList.remove("is-visible");
	elements.countdown.textContent = "";
}

function animateTracks(omggifDuration, wtfgifDuration) {
	const longest = Math.max(omggifDuration, wtfgifDuration);
	const omgWidth = Math.max(4, (omggifDuration / longest) * 100);
	const wtfWidth = Math.max(4, (wtfgifDuration / longest) * 100);
	elements.omgTrack.style.width = `${omgWidth}%`;
	elements.wtfTrack.style.width = `${wtfWidth}%`;
}

async function runRace() {
	if (busy || !selectedBytes) {
		return;
	}
	busy = true;
	elements.raceButton.disabled = true;
	elements.fixtureStrip.setAttribute("aria-disabled", "true");
	resetRaceBoard();
	elements.omgVerdict.textContent = "Engine staged";
	elements.wtfVerdict.textContent = "Engine staged";

	try {
		await showCountdown();
		await nextFrame();

		let omggifResult;
		let wtfgifResult;
		if (raceCount % 2 === 0) {
			omggifResult = measure(() => reencodeOmggif(selectedBytes));
			await nextFrame();
			wtfgifResult = measure(() => remuxGifPixelPerfect(selectedBytes));
		} else {
			wtfgifResult = measure(() => remuxGifPixelPerfect(selectedBytes));
			await nextFrame();
			omggifResult = measure(() => reencodeOmggif(selectedBytes));
		}
		raceCount += 1;

		const ratio = omggifResult.duration / Math.max(wtfgifResult.duration, 0.001);
		elements.omgTime.textContent = formatTime(omggifResult.duration);
		elements.wtfTime.textContent = formatTime(wtfgifResult.duration);
		elements.omgVerdict.textContent = `${formatBytes(
			omggifResult.output.length,
		)} output / ${omggifResult.iterations.toLocaleString()} timed ${
			omggifResult.iterations === 1 ? "run" : "runs"
		}`;
		elements.wtfVerdict.textContent = `${formatBytes(
			wtfgifResult.output.length,
		)} output / ${wtfgifResult.iterations.toLocaleString()} timed ${
			wtfgifResult.iterations === 1 ? "run" : "runs"
		}`;
		elements.liveSpeed.textContent = `${ratio.toLocaleString(undefined, {
			maximumFractionDigits: ratio >= 100 ? 0 : 1,
		})}×`;
		elements.pixelVerdict.textContent = "Decoding every output pixel…";
		animateTracks(omggifResult.duration, wtfgifResult.duration);
		await nextFrame();

		const comparedBytes = await verifyOutputs(
			selectedBytes,
			omggifResult.output,
			wtfgifResult.output,
		);
		elements.pixelVerdict.textContent = `PASS / ${comparedBytes.toLocaleString()} RGBA bytes identical`;
		elements.resultCallout.dataset.result = "pass";
	} catch (error) {
		elements.liveSpeed.textContent = "FAIL";
		elements.pixelVerdict.textContent =
			error instanceof Error ? error.message : String(error);
		elements.resultCallout.dataset.result = "fail";
	} finally {
		busy = false;
		elements.raceButton.disabled = false;
		elements.fixtureStrip.removeAttribute("aria-disabled");
	}
}

async function initialize() {
	renderFixtureButtons();
	await loadFixture();
	const started = performance.now();
	try {
		await initializeWasmGlobally();
		const duration = performance.now() - started;
		const backend = getFastBackendStatus();
		if (!backend.available || backend.name !== "wtfgif-rust-wasm") {
			throw new Error("Rust/Wasm backend did not activate");
		}
		elements.wasmStatus.dataset.state = "ready";
		elements.wasmStatusText.textContent = `Rust/Wasm ready / ${duration.toFixed(
			1,
		)} ms page-load setup`;
		elements.wtfVerdict.textContent = "Rust/Wasm ready";
		elements.raceButton.disabled = false;
	} catch (error) {
		elements.wasmStatus.dataset.state = "error";
		elements.wasmStatusText.textContent = "Rust/Wasm unavailable";
		elements.pixelVerdict.textContent =
			error instanceof Error ? error.message : String(error);
		elements.raceButton.disabled = true;
	}
}

elements.raceButton.addEventListener("click", runRace);
initialize();
