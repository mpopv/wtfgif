import { afterEach, describe, expect, test, vi } from "vitest";
import type { WasmCoreModule } from "../src/types";
import {
	cleanupWasm,
	getWasmFeatures,
	getWasmStatus,
	initializeGlobalWasm,
	initializeWasmModule,
	isWasmReady,
	setWasmCoreModule,
} from "../src/wasm/runtime";
import { supportsWasmSimd } from "../src/wasm/simd";

const fakeCoreModule = {
	WtfGifCore: class {
		width = () => 1;
		height = () => 1;
		frame_count = () => 0;
		metadata_json = () => "{}";
		decode_frame_indices = () => new Uint8Array();
		decode_frame_rgba = () => new Uint8Array();
		decode_frame_bgra = () => new Uint8Array();
		prepare_composited_rgba = () => new Uint32Array();
		prepare_composited_bgra = () => new Uint32Array();
		prepare_composited_delta_rgba = () => new Uint32Array();
		prepare_composited_delta_bgra = () => new Uint32Array();
		free = () => undefined;
	},
} satisfies WasmCoreModule;

afterEach(() => {
	cleanupWasm();
});

describe("WebAssembly runtime integration", () => {
	test("accepts a provided core module through the compatibility initializer", async () => {
		cleanupWasm();
		await initializeGlobalWasm(fakeCoreModule);

		expect(isWasmReady()).toBe(true);
		expect(getWasmStatus()).toMatchObject({
			supported: true,
			initialized: true,
			workerPoolAvailable: false,
		});
	});

	test("does not execute unrelated work during initialization", async () => {
		const reencodeCall = vi.fn(() => new Uint8Array());
		const remuxCall = vi.fn(() => new Uint8Array());
		const decodeCall = vi.fn(() => new Uint32Array());
		await initializeGlobalWasm({
			...fakeCoreModule,
			reencode_gif_pixel_perfect: reencodeCall,
			remux_gif_pixel_perfect: remuxCall,
			decode_all_rgba: decodeCall,
		});

		expect(reencodeCall).not.toHaveBeenCalled();
		expect(remuxCall).not.toHaveBeenCalled();
		expect(decodeCall).not.toHaveBeenCalled();
	});

	test("initializes statically imported bindings for edge runtimes", async () => {
		const init = vi.fn(async () => undefined);
		const module = { ...fakeCoreModule, default: init };
		const compiledModule = {} as WebAssembly.Module;

		await initializeWasmModule(module, compiledModule);

		expect(init).toHaveBeenCalledWith({ module_or_path: compiledModule });
		expect(isWasmReady()).toBe(true);
	});

	test("cleanup disables the configured core", () => {
		setWasmCoreModule(fakeCoreModule);
		expect(isWasmReady()).toBe(true);

		cleanupWasm();
		expect(isWasmReady()).toBe(false);
		expect(getWasmStatus().initialized).toBe(false);
	});

	test("reports runtime WebAssembly capabilities", () => {
		expect(getWasmFeatures()).toEqual({
			supported: typeof WebAssembly !== "undefined",
			simd: supportsWasmSimd(),
			threads: typeof SharedArrayBuffer !== "undefined",
		});
	});
});
