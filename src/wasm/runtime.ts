import { WasmCoreModule } from "../types";

let cachedWasmCoreModule: WasmCoreModule | null | undefined;
let wasmInitPromise: Promise<void> | null = null;
const WASM_REENCODE_PRIMER = new Uint8Array([
	71, 73, 70, 56, 57, 97, 2, 0, 2, 0, 128, 0, 0, 0, 0, 0, 255, 255, 255,
	44, 0, 0, 0, 0, 2, 0, 2, 0, 0, 2, 3, 68, 24, 20, 0, 59,
]);
const WASM_PREPARE_ITERATIONS = 64;

export type WasmWebModule = WasmCoreModule & {
	default?: (moduleOrPath?: unknown) => Promise<unknown>;
};

type NodeModuleBuiltin = {
	createRequire?: (filename: string | URL) => NodeRequire;
};

type ProcessWithBuiltinModule = NodeJS.Process & {
	getBuiltinModule?: (id: string) => NodeModuleBuiltin | undefined;
};

const getSynchronousRequire = (): NodeRequire | null => {
	const processWithBuiltins =
		typeof process === "undefined"
			? undefined
			: (process as ProcessWithBuiltinModule);
	const createRequire =
		processWithBuiltins?.getBuiltinModule?.("node:module")?.createRequire;
	if (createRequire) {
		return createRequire(import.meta.url);
	}
	return typeof require === "function" ? require : null;
};

const tryRequire = (id: string): unknown => {
	try {
		return getSynchronousRequire()?.(id) ?? null;
	} catch {
		return null;
	}
};

const asWasmCoreModule = (value: unknown): WasmCoreModule | null => {
	if (!value || typeof (value as WasmCoreModule).WtfGifCore !== "function") {
		return null;
	}
	return value as WasmCoreModule;
};

const prepareWasmModule = (module: WasmCoreModule): void => {
	module.prepare_reencode_hot_path?.();
	const representativePrimers = module.reencode_hot_path_primer
		? [
				module.reencode_hot_path_primer(16, 2),
				module.reencode_hot_path_primer(32, 8),
				...(module.remux_hot_path_primer
					? [module.remux_hot_path_primer()]
					: []),
			]
		: [];
	if (module.reencode_gif_pixel_perfect) {
		for (const primer of representativePrimers) {
			for (let iteration = 0; iteration < 32; iteration++) {
				module.reencode_gif_pixel_perfect(primer);
				module.remux_gif_pixel_perfect?.(primer);
			}
		}
	}
	for (let iteration = 0; iteration < WASM_PREPARE_ITERATIONS; iteration++) {
		module.reencode_gif_pixel_perfect?.(WASM_REENCODE_PRIMER);
		module.remux_gif_pixel_perfect?.(WASM_REENCODE_PRIMER);
		module.decode_all_rgba?.(WASM_REENCODE_PRIMER);
	}
};

const loadWasmCoreModule = (): WasmCoreModule | null => {
	if (cachedWasmCoreModule !== undefined) {
		return cachedWasmCoreModule;
	}

	const loaded =
		tryRequire("../../crates/wtfgif-core/pkg/wtfgif_core.js") ??
		tryRequire("../crates/wtfgif-core/pkg/wtfgif_core.js") ??
		tryRequire("wtfgif/wasm-core");
	cachedWasmCoreModule = asWasmCoreModule(loaded);
	return cachedWasmCoreModule;
};

export function setWasmCoreModule(module: WasmCoreModule | null): void {
	cachedWasmCoreModule = module;
}

export function getWasmCoreModule(): WasmCoreModule | null {
	return loadWasmCoreModule();
}

const loadBrowserWasmCoreModule = async (
	moduleOrPath?: unknown,
): Promise<WasmCoreModule | null> => {
	if (typeof WebAssembly === "undefined") {
		return null;
	}

	const moduleUrl = new URL(
		"./wasm-web/wtfgif_core.js",
		import.meta.url,
	).href;
	const loaded = (await import(/* @vite-ignore */ moduleUrl)) as WasmWebModule;
	if (typeof loaded.default === "function") {
		await loaded.default(moduleOrPath);
	}
	return asWasmCoreModule(loaded);
};

export const initializeGlobalWasm = (
	moduleOrPath?: unknown,
): Promise<void> => {
	if (wasmInitPromise) {
		return wasmInitPromise;
	}

	wasmInitPromise = (async () => {
		const providedModule = asWasmCoreModule(moduleOrPath);
		if (providedModule) {
			const providedWebModule = moduleOrPath as WasmWebModule;
			if (typeof providedWebModule.default === "function") {
				await providedWebModule.default();
			}
			setWasmCoreModule(providedModule);
			prepareWasmModule(providedModule);
			return;
		}
		if (cachedWasmCoreModule === null) {
			cachedWasmCoreModule = undefined;
		}
		if (moduleOrPath !== undefined) {
			const browserModule = await loadBrowserWasmCoreModule(moduleOrPath);
			if (browserModule) {
				setWasmCoreModule(browserModule);
				prepareWasmModule(browserModule);
				return;
			}
		}
		if (getWasmCoreModule()) {
			return;
		}
		const browserModule = await loadBrowserWasmCoreModule(moduleOrPath);
		if (browserModule) {
			setWasmCoreModule(browserModule);
			prepareWasmModule(browserModule);
		}
	})().finally(() => {
		wasmInitPromise = null;
	});

	return wasmInitPromise;
};

export async function initializeWasmModule(
	module: WasmWebModule,
	moduleOrPath?: unknown,
): Promise<void> {
	if (typeof module.default === "function") {
		await module.default(
			moduleOrPath === undefined ? undefined : { module_or_path: moduleOrPath },
		);
	}
	const wasm = asWasmCoreModule(module);
	if (!wasm) {
		throw new Error("The supplied module is not a wtfgif WebAssembly module");
	}
	setWasmCoreModule(wasm);
	prepareWasmModule(wasm);
}

export function getWasmFeatures(): {
	supported: boolean;
	simd: boolean;
	threads: boolean;
} {
	return {
		supported: typeof WebAssembly !== "undefined",
		simd: false,
		threads:
			typeof SharedArrayBuffer !== "undefined" &&
			(typeof crossOriginIsolated === "undefined" || crossOriginIsolated),
	};
}

export function getWasmStatus(): {
	supported: boolean;
	simd: boolean;
	threads: boolean;
	initialized: boolean;
	workerPoolAvailable: boolean;
} {
	return {
		...getWasmFeatures(),
		initialized: getWasmCoreModule() !== null,
		workerPoolAvailable: false,
	};
}

export function getWasmInitPromise(): Promise<void> | null {
	return wasmInitPromise;
}

export function setWasmInitPromise(promise: Promise<void> | null): void {
	wasmInitPromise = promise;
}

export function isWasmReady(): boolean {
	return getWasmCoreModule() !== null;
}

export function cleanupWasm(): void {
	cachedWasmCoreModule = null;
	wasmInitPromise = null;
}
