import { createRequire } from "node:module";
import ssimPackage from "ssim.js";

const require = createRequire(import.meta.url);
const { GifReader } = require("omggif");
const { ssim } = ssimPackage;

function clearRect(canvas, screenWidth, info) {
	for (let y = info.y; y < info.y + info.height; y += 1) {
		canvas.fill(
			0,
			(y * screenWidth + info.x) * 4,
			(y * screenWidth + info.x + info.width) * 4,
		);
	}
}

export function decodeCompositedGif(bytes) {
	const reader = new GifReader(bytes);
	const frameBytes = reader.width * reader.height * 4;
	const pixels = new Uint8Array(frameBytes * reader.numFrames());
	const canvas = new Uint8Array(frameBytes);
	const delays = new Uint16Array(reader.numFrames());
	let previousInfo = null;
	let previousRestore = null;
	for (let frame = 0; frame < reader.numFrames(); frame += 1) {
		if (previousInfo?.disposal === 2)
			clearRect(canvas, reader.width, previousInfo);
		else if (previousInfo?.disposal === 3 && previousRestore)
			canvas.set(previousRestore);
		const info = reader.frameInfo(frame);
		const restore = info.disposal === 3 ? canvas.slice() : null;
		reader.decodeAndBlitFrameRGBA(frame, canvas);
		pixels.set(canvas, frame * frameBytes);
		delays[frame] = info.delay;
		previousInfo = info;
		previousRestore = restore;
	}
	return {
		width: reader.width,
		height: reader.height,
		frameCount: reader.numFrames(),
		delays,
		pixels,
	};
}

function delayAt(delay, frame) {
	return typeof delay === "number" ? delay : delay[frame];
}

function blackComposite(frame, alphaThreshold) {
	const output = new Uint8Array(frame.length);
	for (let offset = 0; offset < frame.length; offset += 4) {
		if (frame[offset + 3] >= alphaThreshold) {
			output[offset] = frame[offset];
			output[offset + 1] = frame[offset + 1];
			output[offset + 2] = frame[offset + 2];
		}
		output[offset + 3] = 255;
	}
	return output;
}

/**
 * Decode a GIF, check it against the source, and measure its quality. Strict
 * mode rejects any delay or binary-alpha difference. Lenient mode, for
 * encoders that cannot keep exact transparency or timing, records them
 * instead; a pixel decoded transparent where the source is opaque counts as
 * black in PSNR.
 */
export function validateAndMeasure(
	bytes,
	value,
	alphaThreshold,
	{ strict = true } = {},
) {
	const decoded = decodeCompositedGif(bytes);
	if (decoded.width !== value.width || decoded.height !== value.height) {
		throw new Error(`${value.id}: decoded dimensions do not match the source`);
	}
	if (decoded.frameCount !== value.frameCount) {
		throw new Error(
			`${value.id}: decoded frame count does not match the source`,
		);
	}
	let delaysExact = true;
	for (let frame = 0; frame < value.frameCount; frame += 1) {
		if (decoded.delays[frame] !== delayAt(value.delay, frame)) {
			if (strict) {
				throw new Error(`${value.id}: decoded delay differs at frame ${frame}`);
			}
			delaysExact = false;
		}
	}
	let alphaMatches = 0;
	let squaredError = 0;
	let sampleCount = 0;
	let opaquePixels = 0;
	for (let offset = 0; offset < value.rgba.length; offset += 4) {
		const expectedAlpha = value.rgba[offset + 3] >= alphaThreshold ? 255 : 0;
		const decodedOpaque = decoded.pixels[offset + 3] !== 0;
		if (decoded.pixels[offset + 3] === expectedAlpha) {
			alphaMatches += 1;
		} else if (strict) {
			throw new Error(
				`${value.id}: decoded alpha differs at pixel ${offset / 4}`,
			);
		}
		if (expectedAlpha === 0) continue;
		opaquePixels += 1;
		for (let channel = 0; channel < 3; channel += 1) {
			const shown = decodedOpaque ? decoded.pixels[offset + channel] : 0;
			const difference = value.rgba[offset + channel] - shown;
			squaredError += difference * difference;
			sampleCount += 1;
		}
	}
	const psnrDb =
		squaredError === 0 || sampleCount === 0
			? Number.POSITIVE_INFINITY
			: 20 * Math.log10(255 / Math.sqrt(squaredError / sampleCount));

	const frameBytes = value.width * value.height * 4;
	const frameSsim = [];
	for (let frame = 0; frame < value.frameCount; frame += 1) {
		const start = frame * frameBytes;
		const source = blackComposite(
			value.rgba.subarray(start, start + frameBytes),
			alphaThreshold,
		);
		const output = blackComposite(
			decoded.pixels.subarray(start, start + frameBytes),
			1,
		);
		frameSsim.push(
			ssim(
				{ data: source, width: value.width, height: value.height },
				{ data: output, width: value.width, height: value.height },
			).mssim,
		);
	}

	return {
		alphaAgreementPercent: (alphaMatches / (value.rgba.length / 4)) * 100,
		delaysExact,
		opaquePixels,
		psnrDb,
		ssim: frameSsim.reduce((sum, score) => sum + score, 0) / frameSsim.length,
		frameSsim,
	};
}

export function percentile(values, quantile) {
	const sorted = values.toSorted((left, right) => left - right);
	return sorted[
		Math.min(sorted.length - 1, Math.ceil(sorted.length * quantile) - 1)
	];
}

export function percentileNearest(values, quantile) {
	const sorted = values.toSorted((left, right) => left - right);
	return sorted[Math.round((sorted.length - 1) * quantile)];
}

export function median(values) {
	return values.toSorted((left, right) => left - right)[
		Math.floor(values.length / 2)
	];
}

export function geometricMean(values) {
	return Math.exp(
		values.reduce((sum, value) => sum + Math.log(value), 0) / values.length,
	);
}

export function formatFixed(value, digits, width = 0, suffix = "") {
	return `${value.toFixed(digits)}${suffix}`.padStart(width);
}
