import { WasmCoreModule } from "../types";

let cachedWasmCoreModule: WasmCoreModule | null | undefined;
let wasmInitPromise: Promise<void> | null = null;

export type WasmWebModule = WasmCoreModule & {
	default?: (moduleOrPath?: unknown) => Promise<unknown>;
};

type NodeModuleBuiltin = {
	createRequire?: (filename: string | URL) => NodeRequire;
};

type ProcessWithBuiltinModule = NodeJS.Process & {
	getBuiltinModule?: (id: string) => NodeModuleBuiltin | undefined;
};

const synchronousRequire = (() => {
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
})();

const tryRequire = (id: string): unknown => {
	try {
		return synchronousRequire?.(id) ?? null;
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
		tryRequire("./wasm-core/wtfgif_core.js") ??
		tryRequire("../crates/wtfgif-core/pkg/wtfgif_core.js") ??
		tryRequire("../../crates/wtfgif-core/pkg/wtfgif_core.js") ??
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

// Node's generated wasm-pack binding loads synchronously. Start that module
// load as soon as this runtime module is evaluated so package parsing and wasm
// compilation can overlap; initialization still awaits this promise before a
// caller can encode. This performs no encode/decode/remux work.
const preloadedNodeWasmCore =
	typeof process !== "undefined" && process.versions?.node
		? Promise.resolve().then(() => getWasmCoreModule())
		: null;

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
			return;
		}
		if (cachedWasmCoreModule === null) {
			cachedWasmCoreModule = undefined;
		}
		if (moduleOrPath !== undefined) {
			const browserModule = await loadBrowserWasmCoreModule(moduleOrPath);
			if (browserModule) {
				setWasmCoreModule(browserModule);
				return;
			}
		}
		if (preloadedNodeWasmCore) {
			const preloaded = await preloadedNodeWasmCore;
			if (preloaded) {
				setWasmCoreModule(preloaded);
				return;
			}
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
