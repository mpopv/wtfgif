import { WasmCoreModule } from "../types";

let cachedWasmCoreModule: WasmCoreModule | null | undefined;
let wasmInitPromise: Promise<void> | null = null;

type WasmWebModule = WasmCoreModule & {
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
	const loaded = (await import(moduleUrl)) as WasmWebModule;
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
			return;
		}
		if (cachedWasmCoreModule === null) {
			cachedWasmCoreModule = undefined;
		}
		if (getWasmCoreModule()) {
			return;
		}
		const browserModule = await loadBrowserWasmCoreModule(moduleOrPath);
		if (browserModule) {
			setWasmCoreModule(browserModule);
		}
	})().finally(() => {
		wasmInitPromise = null;
	});

	return wasmInitPromise;
};

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
