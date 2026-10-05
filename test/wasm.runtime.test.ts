import { afterEach, describe, expect, test, vi } from "vitest";
import {
	cleanupWasm,
	getWasmStatus,
	initializeGlobalWasm,
	initializeWasmModule,
	setWasmCoreModule,
} from "../src/wasm/runtime";
import { supportsWasmSimd } from "../src/wasm/simd";
import { createFakeWasmCoreModule } from "./helpers/fake-wasm";

const fakeCoreModule = createFakeWasmCoreModule();

afterEach(() => {
	cleanupWasm();
});

describe("WebAssembly runtime integration", () => {
	test("accepts a provided core module through the initializer", async () => {
		cleanupWasm();
		await initializeGlobalWasm(fakeCoreModule);

		expect(getWasmStatus()).toMatchObject({
			supported: true,
			initialized: true,
		});
	});

	test("does not execute unrelated work during initialization", async () => {
		const reencodeCall = vi.fn(() => new Uint8Array());
		const remuxCall = vi.fn(() => new Uint8Array());
		const decodeCall = vi.fn(() => new Uint32Array());
		await initializeGlobalWasm(
			createFakeWasmCoreModule({
				reencode_gif_pixel_perfect: reencodeCall,
				remux_gif_pixel_perfect: remuxCall,
				decode_all_rgba: decodeCall,
			}),
		);

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
		expect(getWasmStatus().initialized).toBe(true);
	});

	test("cleanup disables the configured core", () => {
		setWasmCoreModule(fakeCoreModule);
		expect(getWasmStatus().initialized).toBe(true);

		cleanupWasm();
		expect(getWasmStatus().initialized).toBe(false);
	});

	test("reports runtime WebAssembly capabilities", () => {
		expect(getWasmStatus()).toMatchObject({
			supported: typeof WebAssembly !== "undefined",
			simd: supportsWasmSimd(),
		});
	});
});
