import {
	GifWriter,
	ImageQ,
	GifReader as OmgGifReader,
} from "./vendor/omggif.mjs";
import {
	encodeRgbaGifFrames,
	getFastBackendStatus,
	initializeWasmGlobally,
	remuxGifPixelPerfect,
	GifReader as WtfGifReader,
} from "./vendor/wtfgif.mjs";

const BUILT_IN_IMAGES = [
	["Pogu", "./assets/01-pogu.png"],
	["Blobcat", "./assets/02-blobcat.png"],
	["Portrait", "./assets/03-diabeetus.jpg"],
	["Reaction", "./assets/04-bryce.png"],
	["Potion", "./assets/05-potion.png"],
	["Side eye", "./assets/06-sideeye.png"],
	["Illustration", "./assets/07-bug.png"],
	["Seahorse", "./assets/08-seahorse.png"],
];

const GIF_FIXTURES = [
	["Homer", "homer.gif"],
	["GIGACHAD", "gigachad.gif"],
	["partyparrot", "partyparrot.gif"],
	["tenor", "tenor.gif"],
];

const ALPHA_THRESHOLD = Math.trunc(255 * 0.7);
const ENCODE_SAMPLES = 7;
const DECODE_SAMPLES = 9;

const elements = {
	contractDescription: document.querySelector("#contract-description"),
	contractKicker: document.querySelector("#contract-kicker"),
	countdown: document.querySelector("#countdown"),
	encodeControls: document.querySelector("#encode-controls"),
	fixtureStrip: document.querySelector("#fixture-strip"),
	frameStrip: document.querySelector("#frame-strip"),
	gifControls: document.querySelector("#gif-controls"),
	imageUpload: document.querySelector("#image-upload"),
	liveSpeed: document.querySelector("#live-speed"),
	metricBytes: document.querySelector("#metric-bytes"),
	metricQuality: document.querySelector("#metric-quality"),
	metricValidation: document.querySelector("#metric-validation"),
	modeButtons: [...document.querySelectorAll("[data-mode]")],
	omgDetail: document.querySelector("#omg-detail"),
	omgLabel: document.querySelector("#omg-label"),
	omgTime: document.querySelector("#omg-time"),
	omgTrack: document.querySelector("#omg-track"),
	omgVerdict: document.querySelector("#omg-verdict"),
	outputPreview: document.querySelector("#output-preview"),
	outputPreviewWrap: document.querySelector("#output-preview-wrap"),
	pixelVerdict: document.querySelector("#pixel-verdict"),
	raceButton: document.querySelector("#race-button"),
	raceNote: document.querySelector("#race-note"),
	restoreSample: document.querySelector("#restore-sample"),
	resultCallout: document.querySelector("#result-callout"),
	sizeSelect: document.querySelector("#size-select"),
	sourcePreview: document.querySelector("#source-preview"),
	wasmStatus: document.querySelector("#wasm-status"),
	wasmStatusText: document.querySelector("#wasm-status-text"),
	workloadMeta: document.querySelector("#workload-meta"),
	workloadName: document.querySelector("#workload-name"),
	wtfDetail: document.querySelector("#wtf-detail"),
	wtfLabel: document.querySelector("#wtf-label"),
	wtfTime: document.querySelector("#wtf-time"),
	wtfTrack: document.querySelector("#wtf-track"),
	wtfVerdict: document.querySelector("#wtf-verdict"),
};

const state = {
	busy: false,
	encodeFixture: null,
	gifBytes: null,
	images: [],
	mode: "encode",
	outputUrl: null,
	raceCount: 0,
	selectedGif: GIF_FIXTURES[0],
	usingBuiltIn: true,
	wasmReady: false,
};

const nextFrame = () =>
	new Promise((resolve) => requestAnimationFrame(resolve));
const sleep = (duration) =>
	new Promise((resolve) => setTimeout(resolve, duration));

function median(values) {
	const sorted = [...values].sort((left, right) => left - right);
	return sorted[Math.floor(sorted.length / 2)];
}

function formatBytes(bytes) {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024)
		return `${(bytes / 1024).toFixed(bytes < 10_240 ? 1 : 0)} KB`;
	return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function formatTime(milliseconds) {
	if (milliseconds < 0.01) return "<0.01 ms";
	if (milliseconds < 1) return `${milliseconds.toFixed(3)} ms`;
	if (milliseconds < 100) return `${milliseconds.toFixed(2)} ms`;
	return `${milliseconds.toFixed(1)} ms`;
}

function measure(operation, sampleCount) {
	let output;
	for (let warmup = 0; warmup < 2; warmup += 1) output = operation();

	const probeStart = performance.now();
	output = operation();
	const probeDuration = Math.max(performance.now() - probeStart, 0.01);
	const batchSize = Math.max(1, Math.min(64, Math.ceil(14 / probeDuration)));
	const samples = [];
	for (let sample = 0; sample < sampleCount; sample += 1) {
		const started = performance.now();
		for (let batch = 0; batch < batchSize; batch += 1) output = operation();
		samples.push((performance.now() - started) / batchSize);
	}
	return { duration: median(samples), output, batchSize };
}

function resetResult() {
	elements.omgTime.textContent = "—";
	elements.wtfTime.textContent = "—";
	elements.omgTrack.style.width = "0%";
	elements.wtfTrack.style.width = "0%";
	elements.omgVerdict.textContent = "Waiting on the run";
	elements.wtfVerdict.textContent = state.wasmReady
		? "Rust/Wasm ready"
		: "Rust/Wasm preparing";
	elements.liveSpeed.textContent = "—";
	elements.pixelVerdict.textContent = "Run the demo to validate both outputs.";
	elements.metricBytes.textContent = "—";
	elements.metricQuality.textContent = "—";
	elements.metricValidation.textContent = "—";
	elements.resultCallout.removeAttribute("data-result");
	elements.outputPreviewWrap.hidden = true;
}

function setOutputPreview(bytes) {
	if (state.outputUrl) URL.revokeObjectURL(state.outputUrl);
	state.outputUrl = URL.createObjectURL(
		new Blob([bytes], { type: "image/gif" }),
	);
	elements.outputPreview.src = state.outputUrl;
	elements.outputPreviewWrap.hidden = false;
}

function animateTracks(omggifDuration, wtfgifDuration) {
	const longest = Math.max(omggifDuration, wtfgifDuration);
	elements.omgTrack.style.width = `${Math.max(3, (omggifDuration / longest) * 100)}%`;
	elements.wtfTrack.style.width = `${Math.max(3, (wtfgifDuration / longest) * 100)}%`;
}

async function showCountdown() {
	for (const value of ["3", "2", "1", "GO"]) {
		elements.countdown.textContent = value;
		elements.countdown.classList.remove("is-visible");
		void elements.countdown.offsetWidth;
		elements.countdown.classList.add("is-visible");
		await sleep(value === "GO" ? 230 : 280);
	}
	elements.countdown.classList.remove("is-visible");
	elements.countdown.textContent = "";
}

function loadImage(url, name, owned = false) {
	return new Promise((resolve, reject) => {
		const image = new Image();
		image.decoding = "async";
		image.onload = () => resolve({ image, name, owned, url });
		image.onerror = () => reject(new Error(`Could not decode ${name}`));
		image.src = url;
	});
}

function releaseOwnedImages() {
	for (const item of state.images) {
		if (item.owned) URL.revokeObjectURL(item.url);
	}
}

function renderFrameStrip() {
	elements.frameStrip.replaceChildren();
	if (state.mode !== "encode") return;
	state.images.forEach((item, index) => {
		const frame = document.createElement("div");
		frame.className = "frame-thumb";
		frame.style.animationDelay = `${index * 28}ms`;
		const image = document.createElement("img");
		image.src = item.url;
		image.alt = "";
		const number = document.createElement("span");
		number.textContent = String(index + 1).padStart(2, "0");
		frame.append(image, number);
		elements.frameStrip.append(frame);
	});
}

function normalizeImages() {
	const size = Number(elements.sizeSelect.value);
	const canvas = document.createElement("canvas");
	canvas.width = size;
	canvas.height = size;
	const context = canvas.getContext("2d", { willReadFrequently: true });
	context.imageSmoothingEnabled = true;
	context.imageSmoothingQuality = "high";
	const frameBytes = size * size * 4;
	const rgba = new Uint8Array(frameBytes * state.images.length);

	state.images.forEach(({ image }, frame) => {
		context.clearRect(0, 0, size, size);
		const scale = Math.min(
			size / image.naturalWidth,
			size / image.naturalHeight,
		);
		const width = image.naturalWidth * scale;
		const height = image.naturalHeight * scale;
		context.drawImage(
			image,
			(size - width) / 2,
			(size - height) / 2,
			width,
			height,
		);
		rgba.set(context.getImageData(0, 0, size, size).data, frame * frameBytes);
	});

	state.encodeFixture = {
		frameCount: state.images.length,
		height: size,
		rgba,
		width: size,
	};
	elements.sourcePreview.src = state.images[0]?.url ?? "";
	elements.workloadName.textContent = state.usingBuiltIn
		? "Eight real MakeEmoji images"
		: `${state.images.length} uploaded image${state.images.length === 1 ? "" : "s"}`;
	elements.workloadMeta.textContent = `${state.images.length} frame${state.images.length === 1 ? "" : "s"} / ${size}×${size} RGBA / ${formatBytes(rgba.length)}`;
	renderFrameStrip();
	resetResult();
	updateRaceAvailability();
}

async function restoreBuiltInImages() {
	setBusy(true);
	try {
		releaseOwnedImages();
		state.images = await Promise.all(
			BUILT_IN_IMAGES.map(([name, url]) => loadImage(url, name)),
		);
		state.usingBuiltIn = true;
		normalizeImages();
	} finally {
		setBusy(false);
	}
}

async function loadUploadedImages(files) {
	const selected = [...files].slice(0, 24);
	if (selected.length === 0) return;
	setBusy(true);
	try {
		const nextImages = await Promise.all(
			selected.map((file) => {
				const url = URL.createObjectURL(file);
				return loadImage(url, file.name, true).catch((error) => {
					URL.revokeObjectURL(url);
					throw error;
				});
			}),
		);
		releaseOwnedImages();
		state.images = nextImages;
		state.usingBuiltIn = false;
		normalizeImages();
	} catch (error) {
		showFailure(error);
	} finally {
		setBusy(false);
		elements.imageUpload.value = "";
	}
}

function pointColor(point) {
	return (point.r << 16) | (point.g << 8) | point.b;
}

function padPalette(palette) {
	const padded = [...palette];
	if (padded.length < 2) padded.push(0);
	while (padded.length < 256 && (padded.length & (padded.length - 1)) !== 0)
		padded.push(0);
	return padded;
}

function quantizeImageQGlobal(rgba) {
	const pixelCount = rgba.length / 4;
	const transparent = new Uint8Array(pixelCount);
	let opaqueCount = 0;
	for (let pixel = 0; pixel < pixelCount; pixel += 1) {
		if (rgba[pixel * 4 + 3] < ALPHA_THRESHOLD) transparent[pixel] = 1;
		else opaqueCount += 1;
	}

	if (opaqueCount === 0) {
		return {
			indexed: new Uint8Array(pixelCount).fill(1),
			palette: [0, 0],
			transparentIndex: 1,
		};
	}

	const opaqueRgba = new Uint8Array(opaqueCount * 4);
	let opaqueOffset = 0;
	for (let pixel = 0; pixel < pixelCount; pixel += 1) {
		if (transparent[pixel]) continue;
		const source = pixel * 4;
		const target = opaqueOffset * 4;
		opaqueRgba[target] = rgba[source];
		opaqueRgba[target + 1] = rgba[source + 1];
		opaqueRgba[target + 2] = rgba[source + 2];
		opaqueRgba[target + 3] = 255;
		opaqueOffset += 1;
	}

	const points = ImageQ.utils.PointContainer.fromUint8Array(
		opaqueRgba,
		opaqueCount,
		1,
	);
	const paletteObject = ImageQ.buildPaletteSync([points], {
		paletteQuantization: "rgbquant",
		colors: opaqueCount === pixelCount ? 256 : 255,
	});
	const quantized = ImageQ.applyPaletteSync(points, paletteObject, {
		imageQuantization: "nearest",
	});
	const palette = paletteObject
		.getPointContainer()
		.getPointArray()
		.map(pointColor);
	const colorToIndex = new Map(palette.map((color, index) => [color, index]));
	const quantizedColors = quantized.getPointArray();
	const transparentIndex =
		opaqueCount === pixelCount ? undefined : palette.length;
	if (transparentIndex !== undefined) palette.push(0);

	const indexed = new Uint8Array(pixelCount);
	opaqueOffset = 0;
	for (let pixel = 0; pixel < pixelCount; pixel += 1) {
		if (transparent[pixel]) indexed[pixel] = transparentIndex;
		else {
			indexed[pixel] =
				colorToIndex.get(pointColor(quantizedColors[opaqueOffset])) ?? 0;
			opaqueOffset += 1;
		}
	}
	return { indexed, palette: padPalette(palette), transparentIndex };
}

function encodeOmggifIndexed(fixture, quantized) {
	const output = new Uint8Array(
		quantized.indexed.length * 2 + fixture.frameCount * 1024 + 8192,
	);
	const writer = new GifWriter(output, fixture.width, fixture.height, {
		loop: 0,
		palette: quantized.palette,
	});
	const framePixels = fixture.width * fixture.height;
	for (let frame = 0; frame < fixture.frameCount; frame += 1) {
		writer.addFrame(
			0,
			0,
			fixture.width,
			fixture.height,
			quantized.indexed.subarray(
				frame * framePixels,
				(frame + 1) * framePixels,
			),
			{
				delay: 10,
				disposal: 2,
				transparent: quantized.transparentIndex,
			},
		);
	}
	return output.slice(0, writer.end());
}

function encodeOmggifQuality(fixture) {
	return encodeOmggifIndexed(fixture, quantizeImageQGlobal(fixture.rgba));
}

function encodeWtfgif(fixture) {
	return encodeRgbaGifFrames({
		alphaThreshold: ALPHA_THRESHOLD,
		backend: "wasm",
		delay: 10,
		frameCount: fixture.frameCount,
		frames: fixture.rgba,
		height: fixture.height,
		loop: 0,
		paletteMode: "global",
		quantization: "quality",
		width: fixture.width,
	});
}

function clearFrameRect(canvas, canvasWidth, info) {
	for (let row = info.y; row < info.y + info.height; row += 1) {
		canvas.fill(
			0,
			(row * canvasWidth + info.x) * 4,
			(row * canvasWidth + info.x + info.width) * 4,
		);
	}
}

function decodeAll(Reader, data) {
	const reader = new Reader(data);
	const frameBytes = reader.width * reader.height * 4;
	const canvas = new Uint8Array(frameBytes);
	const restore = new Uint8Array(frameBytes);
	const frames = new Uint8Array(frameBytes * reader.numFrames());
	for (let frame = 0; frame < reader.numFrames(); frame += 1) {
		const info = reader.frameInfo(frame);
		if (info.disposal === 3) restore.set(canvas);
		reader.decodeAndBlitFrameRGBA(frame, canvas);
		frames.set(canvas, frame * frameBytes);
		if (info.disposal === 2) clearFrameRect(canvas, reader.width, info);
		else if (info.disposal === 3) canvas.set(restore);
	}
	reader.dispose?.();
	return {
		frameCount: frames.length / frameBytes,
		frames,
		height: reader.height,
		width: reader.width,
	};
}

function assertSameAnimation(left, right, label) {
	if (
		left.width !== right.width ||
		left.height !== right.height ||
		left.frameCount !== right.frameCount ||
		left.frames.length !== right.frames.length
	) {
		throw new Error(`${label}: animation shape differs`);
	}
	for (let index = 0; index < left.frames.length; index += 1) {
		if (left.frames[index] !== right.frames[index]) {
			throw new Error(
				`${label}: RGBA differs at byte ${index.toLocaleString()}`,
			);
		}
	}
}

function psnrAgainstSource(source, decoded) {
	let squaredError = 0;
	let samples = 0;
	for (let offset = 0; offset < source.length; offset += 4) {
		if (source[offset + 3] < ALPHA_THRESHOLD) continue;
		for (let channel = 0; channel < 3; channel += 1) {
			const difference = source[offset + channel] - decoded[offset + channel];
			squaredError += difference * difference;
			samples += 1;
		}
	}
	if (squaredError === 0 || samples === 0) return Number.POSITIVE_INFINITY;
	return 20 * Math.log10(255 / Math.sqrt(squaredError / samples));
}

function reencodeOmggif(data) {
	const reader = new OmgGifReader(data);
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
		const key = `${info.palette_offset}:${info.palette_size}`;
		let paletteInfo = paletteCache.get(key);
		if (!paletteInfo) {
			const palette = new Array(info.palette_size);
			const colorToIndex = new Map();
			for (let index = 0; index < info.palette_size; index += 1) {
				const offset = info.palette_offset + index * 3;
				const color =
					(data[offset] << 16) | (data[offset + 1] << 8) | data[offset + 2];
				palette[index] = color;
				if (!colorToIndex.has(color)) colorToIndex.set(color, index);
			}
			paletteInfo = { colorToIndex, palette };
			paletteCache.set(key, paletteInfo);
		}
		rgba.fill(0);
		reader.decodeAndBlitFrameRGBA(frame, rgba);
		const indices = new Uint8Array(info.width * info.height);
		let target = 0;
		for (let y = 0; y < info.height; y += 1) {
			let offset = ((info.y + y) * reader.width + info.x) * 4;
			for (let x = 0; x < info.width; x += 1) {
				if (rgba[offset + 3] === 0 && info.transparent_index !== null)
					indices[target] = info.transparent_index;
				else {
					const color =
						(rgba[offset] << 16) | (rgba[offset + 1] << 8) | rgba[offset + 2];
					const index = paletteInfo.colorToIndex.get(color);
					if (index === undefined)
						throw new Error("Decoded color is absent from the frame palette");
					indices[target] = index;
				}
				target += 1;
				offset += 4;
			}
		}
		writer.addFrame(info.x, info.y, info.width, info.height, indices, {
			delay: info.delay,
			disposal: info.disposal,
			palette: paletteInfo.palette,
			transparent: info.transparent_index,
		});
	}
	return output.slice(0, writer.end());
}

function displayResult(omg, wtf, summary) {
	const ratio = omg.duration / Math.max(wtf.duration, 0.0001);
	elements.omgTime.textContent = formatTime(omg.duration);
	elements.wtfTime.textContent = formatTime(wtf.duration);
	elements.liveSpeed.textContent = `${ratio.toLocaleString(undefined, {
		maximumFractionDigits: ratio >= 100 ? 0 : 1,
	})}×`;
	elements.pixelVerdict.textContent = summary;
	elements.resultCallout.dataset.result = "pass";
	animateTracks(omg.duration, wtf.duration);
}

async function runEncodeRace() {
	const fixture = state.encodeFixture;
	if (!fixture) throw new Error("No RGBA frames are ready");
	const omgOperation = () => encodeOmggifQuality(fixture);
	const wtfOperation = () => encodeWtfgif(fixture);
	let omg;
	let wtf;
	if (state.raceCount % 2 === 0) {
		omg = measure(omgOperation, ENCODE_SAMPLES);
		await nextFrame();
		wtf = measure(wtfOperation, ENCODE_SAMPLES);
	} else {
		wtf = measure(wtfOperation, ENCODE_SAMPLES);
		await nextFrame();
		omg = measure(omgOperation, ENCODE_SAMPLES);
	}
	const omgDecoded = decodeAll(OmgGifReader, omg.output);
	const wtfDecoded = decodeAll(OmgGifReader, wtf.output);
	if (
		omgDecoded.width !== fixture.width ||
		wtfDecoded.width !== fixture.width ||
		omgDecoded.frameCount !== fixture.frameCount ||
		wtfDecoded.frameCount !== fixture.frameCount
	) {
		throw new Error("An encoder returned the wrong animation dimensions");
	}
	const omgPsnr = psnrAgainstSource(fixture.rgba, omgDecoded.frames);
	const wtfPsnr = psnrAgainstSource(fixture.rgba, wtfDecoded.frames);

	elements.omgVerdict.textContent = `${formatBytes(omg.output.length)} / ${omg.batchSize}× timer batch`;
	elements.wtfVerdict.textContent = `${formatBytes(wtf.output.length)} / ${wtf.batchSize}× timer batch`;
	elements.metricBytes.textContent = `${formatBytes(wtf.output.length)} (${(wtf.output.length / omg.output.length).toFixed(2)}× baseline)`;
	elements.metricQuality.textContent = `${Number.isFinite(wtfPsnr) ? wtfPsnr.toFixed(1) : "∞"} dB (${(wtfPsnr - omgPsnr).toFixed(1)} vs baseline)`;
	elements.metricValidation.textContent = "Both outputs decoded";
	displayResult(
		omg,
		wtf,
		`PASS / both GIFs decoded; quality measured against the source RGBA frames.`,
	);
	setOutputPreview(wtf.output);
}

async function runDecodeRace() {
	const data = state.gifBytes;
	if (!data) throw new Error("No GIF fixture is ready");
	const omgOperation = () => decodeAll(OmgGifReader, data);
	const wtfOperation = () => decodeAll(WtfGifReader, data);
	let omg;
	let wtf;
	if (state.raceCount % 2 === 0) {
		omg = measure(omgOperation, DECODE_SAMPLES);
		await nextFrame();
		wtf = measure(wtfOperation, DECODE_SAMPLES);
	} else {
		wtf = measure(wtfOperation, DECODE_SAMPLES);
		await nextFrame();
		omg = measure(omgOperation, DECODE_SAMPLES);
	}
	assertSameAnimation(omg.output, wtf.output, "Decode parity");
	elements.omgVerdict.textContent = `${omg.output.frameCount} frames / ${omg.batchSize}× timer batch`;
	elements.wtfVerdict.textContent = `${wtf.output.frameCount} frames / ${wtf.batchSize}× timer batch`;
	elements.metricBytes.textContent = formatBytes(data.length);
	elements.metricQuality.textContent = "Lossless decode";
	elements.metricValidation.textContent = `${wtf.output.frames.length.toLocaleString()} RGBA bytes`;
	displayResult(omg, wtf, "PASS / every composited RGBA byte matches omggif.");
}

async function runRemuxRace() {
	const data = state.gifBytes;
	if (!data) throw new Error("No GIF fixture is ready");
	const source = decodeAll(OmgGifReader, data);
	const omgOperation = () => reencodeOmggif(data);
	const wtfOperation = () => remuxGifPixelPerfect(data);
	let omg;
	let wtf;
	if (state.raceCount % 2 === 0) {
		omg = measure(omgOperation, ENCODE_SAMPLES);
		await nextFrame();
		wtf = measure(wtfOperation, ENCODE_SAMPLES);
	} else {
		wtf = measure(wtfOperation, ENCODE_SAMPLES);
		await nextFrame();
		omg = measure(omgOperation, ENCODE_SAMPLES);
	}
	assertSameAnimation(
		source,
		decodeAll(OmgGifReader, omg.output),
		"omggif reencode",
	);
	assertSameAnimation(
		source,
		decodeAll(OmgGifReader, wtf.output),
		"wtfgif remux",
	);
	elements.omgVerdict.textContent = `${formatBytes(omg.output.length)} / fresh LZW`;
	elements.wtfVerdict.textContent = `${formatBytes(wtf.output.length)} / original LZW preserved`;
	elements.metricBytes.textContent = formatBytes(wtf.output.length);
	elements.metricQuality.textContent = "Exact source pixels";
	elements.metricValidation.textContent = `${source.frames.length.toLocaleString()} RGBA bytes`;
	displayResult(
		omg,
		wtf,
		"PASS / structural remux only; this is not an arbitrary-image encode result.",
	);
	setOutputPreview(wtf.output);
}

function showFailure(error) {
	elements.liveSpeed.textContent = "FAIL";
	elements.pixelVerdict.textContent =
		error instanceof Error ? error.message : String(error);
	elements.resultCallout.dataset.result = "fail";
}

function setBusy(busy) {
	state.busy = busy;
	updateRaceAvailability();
}

function updateRaceAvailability() {
	const inputReady =
		state.mode === "encode"
			? Boolean(state.encodeFixture)
			: Boolean(state.gifBytes);
	elements.raceButton.disabled = state.busy || !state.wasmReady || !inputReady;
}

async function runRace() {
	if (state.busy || !state.wasmReady) return;
	setBusy(true);
	resetResult();
	try {
		await showCountdown();
		await nextFrame();
		if (state.mode === "encode") await runEncodeRace();
		else if (state.mode === "decode") await runDecodeRace();
		else await runRemuxRace();
		state.raceCount += 1;
	} catch (error) {
		showFailure(error);
	} finally {
		setBusy(false);
	}
}

function renderFixtureButtons() {
	elements.fixtureStrip.replaceChildren();
	for (const fixture of GIF_FIXTURES) {
		const button = document.createElement("button");
		button.className = "fixture-button";
		button.type = "button";
		button.textContent = fixture[0];
		button.dataset.file = fixture[1];
		button.setAttribute("aria-pressed", String(fixture === state.selectedGif));
		button.addEventListener("click", () => loadGifFixture(fixture));
		elements.fixtureStrip.append(button);
	}
}

async function loadGifFixture(fixture) {
	if (state.busy) return;
	state.selectedGif = fixture;
	for (const button of elements.fixtureStrip.querySelectorAll("button")) {
		button.setAttribute(
			"aria-pressed",
			String(button.dataset.file === fixture[1]),
		);
	}
	setBusy(true);
	try {
		const response = await fetch(`./gifs/${fixture[1]}`);
		if (!response.ok) throw new Error(`GIF fetch failed (${response.status})`);
		state.gifBytes = new Uint8Array(await response.arrayBuffer());
		if (state.mode !== "encode") showGifWorkload();
	} catch (error) {
		showFailure(error);
	} finally {
		setBusy(false);
	}
}

function showGifWorkload() {
	const reader = new OmgGifReader(state.gifBytes);
	elements.sourcePreview.src = `./gifs/${state.selectedGif[1]}`;
	elements.workloadName.textContent = state.selectedGif[0];
	elements.workloadMeta.textContent = `${reader.numFrames()} frames / ${reader.width}×${reader.height} / ${formatBytes(state.gifBytes.length)}`;
	renderFrameStrip();
	resetResult();
}

function setMode(mode) {
	state.mode = mode;
	for (const button of elements.modeButtons) {
		button.setAttribute("aria-selected", String(button.dataset.mode === mode));
	}
	elements.encodeControls.classList.toggle("is-hidden", mode !== "encode");
	elements.gifControls.classList.toggle("is-hidden", mode === "encode");
	if (mode === "encode") {
		elements.contractKicker.textContent = "RGBA → palette → GIF";
		elements.contractDescription.textContent =
			"Both clocks include palette creation and pixel mapping; wtfgif uses literal LZW by default.";
		elements.omgLabel.textContent = "image-q + omggif";
		elements.omgDetail.textContent = "global rgbquant / balanced LZW";
		elements.wtfLabel.textContent = "wtfgif";
		elements.wtfDetail.textContent = "quality/global / literal LZW";
		elements.raceButton.textContent = "Run encoder race";
		elements.raceNote.textContent =
			"Image decoding, resizing, and one-time Wasm initialization happen before the clock. Results are medians; engine order alternates between races.";
		normalizeImages();
	} else if (mode === "decode") {
		elements.contractKicker.textContent = "GIF → every composited RGBA frame";
		elements.contractDescription.textContent =
			"Both public GifReader APIs parse and decode the same real GIF into caller-owned pixel buffers.";
		elements.omgLabel.textContent = "omggif GifReader";
		elements.omgDetail.textContent = "JavaScript decode";
		elements.wtfLabel.textContent = "wtfgif GifReader";
		elements.wtfDetail.textContent = "Rust/Wasm decode";
		elements.raceButton.textContent = "Run decode race";
		elements.raceNote.textContent =
			"One-time Wasm initialization happens before the clock. Every composited RGBA byte must match omggif.";
		showGifWorkload();
	} else {
		elements.contractKicker.textContent = "Existing GIF → pixel-identical GIF";
		elements.contractDescription.textContent =
			"omggif decodes and recompresses; wtfgif validates and preserves existing LZW. This structural shortcut is intentionally not an encoder claim.";
		elements.omgLabel.textContent = "omggif reencode";
		elements.omgDetail.textContent = "decode + fresh LZW";
		elements.wtfLabel.textContent = "wtfgif remux";
		elements.wtfDetail.textContent = "validate + preserve LZW";
		elements.raceButton.textContent = "Run remux race";
		elements.raceNote.textContent =
			"This mode measures a lossless structural operation on an existing GIF. It does not represent arbitrary-image encoding.";
		showGifWorkload();
	}
	updateRaceAvailability();
}

async function initialize() {
	renderFixtureButtons();
	for (const button of elements.modeButtons)
		button.addEventListener("click", () => setMode(button.dataset.mode));
	elements.imageUpload.addEventListener("change", () =>
		loadUploadedImages(elements.imageUpload.files),
	);
	elements.restoreSample.addEventListener("click", restoreBuiltInImages);
	elements.sizeSelect.addEventListener("change", normalizeImages);
	elements.raceButton.addEventListener("click", runRace);

	const inputs = (async () => {
		await restoreBuiltInImages();
		await loadGifFixture(state.selectedGif);
	})();
	const started = performance.now();
	try {
		await initializeWasmGlobally();
		const duration = performance.now() - started;
		const backend = getFastBackendStatus();
		if (!backend.available || backend.name !== "wtfgif-rust-wasm")
			throw new Error("Rust/Wasm backend did not activate");
		state.wasmReady = true;
		elements.wasmStatus.dataset.state = "ready";
		elements.wasmStatusText.textContent = `Rust/Wasm ready / ${duration.toFixed(1)} ms setup`;
	} catch (error) {
		state.wasmReady = false;
		elements.wasmStatus.dataset.state = "error";
		elements.wasmStatusText.textContent = "Rust/Wasm unavailable";
		showFailure(error);
	}
	await inputs;
	setMode("encode");
	updateRaceAvailability();
}

initialize();
