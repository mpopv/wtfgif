import { readFileSync } from "node:fs";
import { performance } from "node:perf_hooks";

const rgba = new Uint8Array(
	readFileSync(
		new URL("../test/rgba/makeemoji-128x128x8.rgba", import.meta.url),
	),
);
const wasmBytes = readFileSync(
	process.env.PROFILE_WASM_PATH ??
		new URL(
			"../crates/wtfgif-core/pkg-quality-simd/wtfgif_core_bg.wasm",
			import.meta.url,
		),
);
const module = new WebAssembly.Module(wasmBytes);
const instance = new WebAssembly.Instance(module, {});
const wasm = instance.exports;

const started = performance.now();
const inputPointer = wasm.indexed_lzw_input_scratch_reserve(rgba.length);
const reserved = performance.now();
new Uint8Array(wasm.memory.buffer, inputPointer, rgba.length).set(rgba);
const copiedInput = performance.now();
const outputLength =
	wasm.encode_rgba_quality_gif_constant_delay_scratch_from_input(
		rgba.length,
		128,
		128,
		8,
		10,
		0,
		179,
	);
const encoded = performance.now();
const output = new Uint8Array(
	wasm.memory.buffer,
	wasm.gif_output_scratch_ptr(),
	outputLength,
).slice();
const copiedOutput = performance.now();

process.stdout.write(
	JSON.stringify({
		reserveMs: reserved - started,
		inputCopyMs: copiedInput - reserved,
		wasmMs: encoded - copiedInput,
		outputCopyMs: copiedOutput - encoded,
		totalMs: copiedOutput - started,
		outputBytes: output.length,
	}),
);
