// Accept an encoder change by what its GIFs decode to, not by their bytes.
//
// The lossless gate requires every corpus fixture to decode to exactly the
// same composited frames, delays, and loop count as the baseline build. Use it
// for bitstream, layout, and speed changes that must not alter pixels.
//
// The quality gate is for palette or mapping changes that may choose
// different pixels. Shape, delays, and binary alpha must still be exact, and
// RGB PSNR and SSIM may fall by at most the configured tolerances.
//
//   EQUIVALENCE_BASELINE=path/to/baseline/dist npm run validate:equivalence
//   EQUIVALENCE_GATE=quality EQUIVALENCE_BASELINE=... npm run validate:equivalence
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { ALPHA_THRESHOLD, loadBenchmarkCorpus } from "../bench/lib/corpus.mjs";
import {
	decodeCompositedGif,
	geometricMean,
	validateAndMeasure,
} from "../bench/lib/metrics.mjs";
import { root } from "../lib/paths.mjs";

const baselineDir = process.env.EQUIVALENCE_BASELINE;
if (!baselineDir) {
	throw new Error(
		"EQUIVALENCE_BASELINE must name a baseline build's dist directory",
	);
}
const candidateDir = process.env.EQUIVALENCE_CANDIDATE ?? join(root, "dist");
const gate = process.env.EQUIVALENCE_GATE ?? "lossless";
if (gate !== "lossless" && gate !== "quality") {
	throw new Error('EQUIVALENCE_GATE must be "lossless" or "quality"');
}
const maxPsnrLoss = Number(process.env.EQUIVALENCE_MAX_PSNR_LOSS ?? 0.05);
const maxSsimLoss = Number(process.env.EQUIVALENCE_MAX_SSIM_LOSS ?? 0.0005);
const candidateOptions =
	process.env.EQUIVALENCE_INDEPENDENT_FRAMES === "1"
		? { independentFrames: true }
		: {};

async function loadEncoder(dir) {
	const module = await import(
		pathToFileURL(join(resolve(dir), "encode.mjs")).href
	);
	await module.initializeWasmGlobally();
	return module;
}

function sameBytes(left, right) {
	if (left.length !== right.length) return false;
	for (let index = 0; index < left.length; index += 1) {
		if (left[index] !== right[index]) return false;
	}
	return true;
}

const baseline = await loadEncoder(baselineDir);
const candidate = await loadEncoder(candidateDir);
const corpus = loadBenchmarkCorpus({
	includeStress: process.env.EQUIVALENCE_INCLUDE_STRESS === "1",
});
const rows = [];
const failures = [];
for (const value of corpus) {
	const options = {
		alphaThreshold: ALPHA_THRESHOLD,
		delay: value.delay,
		frameCount: value.frameCount,
		frames: value.rgba,
		height: value.height,
		loop: 0,
		width: value.width,
	};
	const before = baseline.encodeRgbaGifFrames(options);
	const after = candidate.encodeRgbaGifFrames({
		...options,
		...candidateOptions,
	});
	// Both throw unless shape, frame count, delays, and binary alpha are exact.
	const beforeQuality = validateAndMeasure(before, value, ALPHA_THRESHOLD);
	const afterQuality = validateAndMeasure(after, value, ALPHA_THRESHOLD);
	const pixelsMatch = sameBytes(
		decodeCompositedGif(before).pixels,
		decodeCompositedGif(after).pixels,
	);
	const psnrLoss = beforeQuality.psnrDb - afterQuality.psnrDb;
	const ssimLoss = beforeQuality.ssim - afterQuality.ssim;
	if (gate === "lossless" && !pixelsMatch) {
		failures.push(`${value.id}: decoded frames differ from the baseline`);
	}
	if (
		gate === "quality" &&
		!pixelsMatch &&
		(!(psnrLoss <= maxPsnrLoss) || !(ssimLoss <= maxSsimLoss))
	) {
		failures.push(
			`${value.id}: PSNR fell ${psnrLoss.toFixed(3)} dB and SSIM fell ${ssimLoss.toFixed(5)}`,
		);
	}
	rows.push({
		fixture: value.id,
		baselineBytes: before.length,
		candidateBytes: after.length,
		sizeRatio: Number((before.length / after.length).toFixed(2)),
		pixels: pixelsMatch ? "identical" : "different",
		baselinePsnr: Number(beforeQuality.psnrDb.toFixed(2)),
		candidatePsnr: Number(afterQuality.psnrDb.toFixed(2)),
		baselineSsim: Number(beforeQuality.ssim.toFixed(4)),
		candidateSsim: Number(afterQuality.ssim.toFixed(4)),
	});
}

console.table(rows);
console.log(
	`Geometric-mean size ratio (baseline / candidate): ${geometricMean(rows.map((row) => row.baselineBytes / row.candidateBytes)).toFixed(2)}x`,
);
if (failures.length > 0) {
	throw new Error(
		`${gate} equivalence gate failed:\n${failures.map((line) => `  ${line}`).join("\n")}`,
	);
}
console.log(
	`${gate === "lossless" ? "Lossless" : "Quality"} equivalence gate passed: ${rows.length} fixtures`,
);
