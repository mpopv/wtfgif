import type { WasmCoreModule, WasmEncodeCoreModule } from "../types";

type WarmupModule = WasmCoreModule | WasmEncodeCoreModule;

// Use enough distinct colors to enter the normal quality quantizer, rather
// than warming only the exact-palette branch with a one-color image.
const WARMUP_WIDTH = 32;
const WARMUP_HEIGHT = 32;
const WARMUP_RGBA = (() => {
	const rgba = new Uint8Array(WARMUP_WIDTH * WARMUP_HEIGHT * 4);
	for (let pixel = 0; pixel < WARMUP_WIDTH * WARMUP_HEIGHT; pixel += 1) {
		const offset = pixel * 4;
		rgba[offset] = pixel & 0xff;
		rgba[offset + 1] = (pixel >> 2) & 0xff;
		rgba[offset + 2] = (pixel >> 4) & 0xff;
		rgba[offset + 3] = 255;
	}
	return rgba;
})();
const WARMUP_DELAYS = new Uint16Array([0]);
const WARMUP_PALETTE = new Uint32Array([0, 0]);
const WARMUP_PIXEL = WARMUP_RGBA.subarray(0, 4);

/**
 * Pay Wasm's first-call JIT and allocator cost while the page is initializing.
 * All symbols are optional so this remains a no-op for test doubles and the
 * encode-only/full-module split stays intact.
 */
export function warmupWasmCore(module: WarmupModule | null): void {
	if (!module) {
		return;
	}

	try {
		const reserve = module.indexed_lzw_input_scratch_reserve;
		const memory = module.wasm_memory;
		const encode = module.encode_rgba_quality_gif_scratch_from_input;
		if (reserve && memory && encode) {
			const pointer = reserve(WARMUP_RGBA.byteLength);
			const wasmMemory = memory();
			new Uint8Array(
				wasmMemory.buffer,
				pointer,
				WARMUP_RGBA.byteLength,
			).set(WARMUP_RGBA);
			encode(
				WARMUP_RGBA.byteLength,
				WARMUP_WIDTH,
				WARMUP_HEIGHT,
				1,
				WARMUP_DELAYS,
				-1,
				179,
			);
		}

		// Also touch the explicit-palette/literal entry used by the drop-in
		// indexed and RGBA APIs. It is a separate Rust export from quality
		// quantization and otherwise pays its own first-call compilation cost.
		module.encode_rgba_literal_gif?.(
			WARMUP_PIXEL,
			1,
			1,
			1,
			WARMUP_PALETTE,
			0,
			-1,
		);
		module.encode_rgba_literal_gif_with_options?.(
			WARMUP_PIXEL,
			1,
			1,
			1,
			WARMUP_PALETTE,
			WARMUP_DELAYS,
			-1,
			128,
		);

		// The full package can also warm the decoder's Wasm entry points. The
		// tiny valid GIF keeps this bounded and does not retain any user data.
		if ("WtfGifCore" in module && module.WtfGifCore) {
			const core = new module.WtfGifCore(USE_WARMUP_GIF);
			core.decode_frame_rgba(0);
			core.free();
		}
	} catch {
		// Initialization must still succeed for older/custom modules that do not
		// expose every optional scratch symbol.
	}
}

const USE_WARMUP_GIF = new Uint8Array([
	0x47, 0x49, 0x46, 0x38, 0x39, 0x61,
	0x01, 0x00, 0x01, 0x00,
	0x80, 0x00, 0x00,
	0x00, 0x00, 0x00,
	0xff, 0xff, 0xff,
	0x2c, 0x00, 0x00, 0x00, 0x00,
	0x01, 0x00, 0x01, 0x00, 0x00,
	0x02, 0x02, 0x4c, 0x01, 0x00,
	0x3b,
]);
