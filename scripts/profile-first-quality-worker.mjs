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
let wasm;
const imports = {
	"./wtfgif_core_bg.js": {
		__wbindgen_init_externref_table() {
			const table = wasm.__wbindgen_externrefs;
			const offset = table.grow(4);
			table.set(0, undefined);
			table.set(offset, undefined);
			table.set(offset + 1, null);
			table.set(offset + 2, true);
			table.set(offset + 3, false);
		},
	},
};
wasm = new WebAssembly.Instance(module, imports).exports;
wasm.__wbindgen_start();

const started = performance.now();
const inputPointer = wasm.indexed_lzw_input_scratch_reserve(rgba.length);
const reserved = performance.now();
new Uint8Array(wasm.memory.buffer, inputPointer, rgba.length).set(rgba);
const copiedInput = performance.now();
const outputLength = (
	process.env.PROFILE_GENERAL === "1"
		? wasm.encode_rgba_quality_gif_constant_delay_scratch_from_input
		: wasm.encode_rgba_quality_low_res_constant_delay_scratch_from_input
)(rgba.length, 128, 128, 8, 10, 0, 179);
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
