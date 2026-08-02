import { WasmCoreModule, WasmEncodeCoreModule } from "../types";
import { supportsWasmSimd } from "./simd";

let cachedWasmCoreModule: WasmEncodeCoreModule | null | undefined;
let wasmInitPromise: Promise<void> | null = null;
let fallbackWasmCoreModuleGetter: (() => WasmCoreModule | null) | null = null;

export type WasmEncodeWebModule = WasmEncodeCoreModule & {
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

const asWasmEncodeCoreModule = (
	value: unknown,
): WasmEncodeCoreModule | null => {
	if (
		!value ||
		typeof (value as WasmEncodeCoreModule)
				.encode_rgba_quality_gif_scratch_from_input !== "function"
	) {
		return null;
	}
	return value as WasmEncodeCoreModule;
};

const loadWasmCoreModule = (): WasmEncodeCoreModule | null => {
	if (cachedWasmCoreModule !== undefined) {
		return cachedWasmCoreModule;
	}

	const loaded =
		(supportsWasmSimd()
			? tryRequire("./wasm-encode-simd/wtfgif_core.js")
			: null) ??
		tryRequire("./wasm-encode/wtfgif_core.js") ??
		tryRequire("../crates/wtfgif-core/pkg-encode/wtfgif_core.js") ??
		tryRequire("../../crates/wtfgif-core/pkg-encode/wtfgif_core.js");
	cachedWasmCoreModule = asWasmEncodeCoreModule(loaded);
	return cachedWasmCoreModule;
};

export function setWasmEncodeCoreModule(
	module: WasmEncodeCoreModule | null,
): void {
	cachedWasmCoreModule = module;
}

export function getWasmEncodeCoreModule(): WasmEncodeCoreModule | null {
	// The full package registers its decoder-capable module as a fallback. It
	// must win here so importing `wtfgif` never loads both Wasm artifacts; the
	// encode-only entry has no fallback and therefore loads its small module.
	return fallbackWasmCoreModuleGetter?.() ?? loadWasmCoreModule() ?? null;
}

/**
 * The full package entry registers its decoder-capable module as a lazy
 * fallback. Keeping this callback here avoids importing the full runtime from
 * the encode-only entry, while preserving the normal `wtfgif` behavior.
 */
export function setWasmEncodeFallback(
	getter: (() => WasmCoreModule | null) | null,
): void {
	fallbackWasmCoreModuleGetter = getter;
}

const preloadedNodeWasmCore =
	typeof process !== "undefined" && process.versions?.node
		? Promise.resolve().then(() => getWasmEncodeCoreModule())
		: null;

const loadBrowserWasmCoreModule = async (
	moduleOrPath?: unknown,
): Promise<WasmEncodeCoreModule | null> => {
	if (typeof WebAssembly === "undefined") {
		return null;
	}

	const moduleUrls = [
		...(supportsWasmSimd() ? ["./wasm-encode-web-simd/wtfgif_core.js"] : []),
		"./wasm-encode-web/wtfgif_core.js",
	].map((path) => new URL(path, import.meta.url).href);
	for (const moduleUrl of moduleUrls) {
		try {
			const loaded = (await import(/* @vite-ignore */ moduleUrl)) as WasmEncodeWebModule;
			if (typeof loaded.default === "function") {
				await loaded.default(moduleOrPath);
			}
			const wasm = asWasmEncodeCoreModule(loaded);
			if (wasm) {
				return wasm;
			}
		} catch {
			// A package built without the optional SIMD artifact falls through to
			// the portable scalar module.
		}
	}
	return null;
};

export const initializeGlobalWasm = (moduleOrPath?: unknown): Promise<void> => {
	if (wasmInitPromise) {
		return wasmInitPromise;
	}

	wasmInitPromise = (async () => {
		const providedModule = asWasmEncodeCoreModule(moduleOrPath);
		if (providedModule) {
			const providedWebModule = moduleOrPath as WasmEncodeWebModule;
			if (typeof providedWebModule.default === "function") {
				await providedWebModule.default();
			}
			setWasmEncodeCoreModule(providedModule);
			return;
		}
		if (cachedWasmCoreModule === null) {
			cachedWasmCoreModule = undefined;
		}
		if (moduleOrPath !== undefined) {
			const browserModule = await loadBrowserWasmCoreModule(moduleOrPath);
			if (browserModule) {
				setWasmEncodeCoreModule(browserModule);
				return;
			}
		}
		if (preloadedNodeWasmCore) {
			const preloaded = await preloadedNodeWasmCore;
			if (preloaded) {
				setWasmEncodeCoreModule(preloaded);
				return;
			}
		}
		const loaded = getWasmEncodeCoreModule();
		if (loaded) {
			return;
		}
		const browserModule = await loadBrowserWasmCoreModule(moduleOrPath);
		if (browserModule) {
			setWasmEncodeCoreModule(browserModule);
		}
	})().finally(() => {
		wasmInitPromise = null;
	});

	return wasmInitPromise;
};

export async function initializeWasmModule(
	module: WasmEncodeWebModule,
	moduleOrPath?: unknown,
): Promise<void> {
	if (typeof module.default === "function") {
		await module.default(
			moduleOrPath === undefined ? undefined : { module_or_path: moduleOrPath },
		);
	}
	const wasm = asWasmEncodeCoreModule(module);
	if (!wasm) {
		throw new Error("The supplied module is not a wtfgif encode WebAssembly module");
	}
	setWasmEncodeCoreModule(wasm);
}

export function getWasmFeatures(): {
	supported: boolean;
	simd: boolean;
	threads: boolean;
} {
	return {
		supported: typeof WebAssembly !== "undefined",
		simd: supportsWasmSimd(),
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
		initialized: getWasmEncodeCoreModule() !== null,
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
	return getWasmEncodeCoreModule() !== null;
}

export function cleanupWasm(): void {
	cachedWasmCoreModule = null;
	wasmInitPromise = null;
}
