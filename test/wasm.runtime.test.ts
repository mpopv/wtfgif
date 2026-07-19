import { afterEach, describe, expect, test } from "vitest";
import type { WasmCoreModule } from "../src/types";
import {
	cleanupWasm,
	getWasmFeatures,
	getWasmStatus,
	initializeGlobalWasm,
	isWasmReady,
	setWasmCoreModule,
} from "../src/wasm/runtime";

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
			simd: false,
			threads: typeof SharedArrayBuffer !== "undefined",
		});
	});
});
