import { supportsWasmSimd } from "./simd";

export type WasmWebBinding<T> = T & {
	default?: (moduleOrPath?: unknown) => Promise<unknown>;
};

interface WasmRuntimePaths {
	nodeScalar: readonly string[];
	nodeSimd: readonly string[];
	browserScalar: string;
	browserSimd: string;
}

interface WasmRuntimeOptions<T> {
	name: string;
	paths: WasmRuntimePaths;
	isModule: (value: unknown) => value is T;
	loadNode?: () => T | undefined;
}

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
	if (createRequire) return createRequire(import.meta.url);
	return typeof require === "function" && typeof require.resolve === "function"
		? require
		: null;
})();

function isModuleMissing(error: unknown): boolean {
	if (!(error instanceof Error)) return false;
	const code = (error as Error & { code?: string }).code;
	return code === "MODULE_NOT_FOUND" || code === "ERR_MODULE_NOT_FOUND";
}

function requireCandidate(request: string): unknown | null {
	if (!synchronousRequire) return null;
	try {
		return synchronousRequire(request);
	} catch (error) {
		if (!isModuleMissing(error)) throw error;
		// A missing candidate and a candidate whose dependency failed both
		// surface as MODULE_NOT_FOUND from require(). Resolve only on that
		// slow/error path so an existing Wasm binding avoids two loader calls.
		try {
			synchronousRequire.resolve(request);
		} catch (resolveError) {
			if (isModuleMissing(resolveError)) return null;
			throw resolveError;
		}
		throw error;
	}
}

export interface WasmModuleRuntime<T> {
	cleanup(): void;
	get(): T | null;
	initialize(moduleOrPath?: unknown): Promise<void>;
	initializeModule(
		module: WasmWebBinding<T>,
		moduleOrPath?: unknown,
	): Promise<void>;
	set(module: T | null): void;
}

export function createWasmModuleRuntime<T>(
	options: WasmRuntimeOptions<T>,
): WasmModuleRuntime<T> {
	let cached: T | null | undefined;
	let initPromise: Promise<void> | null = null;

	const validate = (value: unknown): T => {
		if (!options.isModule(value)) {
			throw new Error(
				`The supplied module does not implement the ${options.name} WebAssembly contract`,
			);
		}
		return value;
	};

	const loadNode = (): T | null => {
		const custom = options.loadNode?.();
		if (custom !== undefined) return validate(custom);
		const groups = supportsWasmSimd()
			? [options.paths.nodeSimd, options.paths.nodeScalar]
			: [options.paths.nodeScalar];
		for (const candidates of groups) {
			for (const request of candidates) {
				const loaded = requireCandidate(request);
				if (loaded !== null) return validate(loaded);
			}
		}
		return null;
	};

	const loadBrowserCandidate = async (
		path: string,
		moduleOrPath?: unknown,
	): Promise<T> => {
		const moduleUrl = new URL(path, import.meta.url).href;
		const loaded = (await import(
			/* @vite-ignore */ moduleUrl
		)) as WasmWebBinding<T>;
		if (typeof loaded.default === "function") {
			await loaded.default(moduleOrPath);
		}
		return validate(loaded);
	};

	const loadBrowser = async (moduleOrPath?: unknown): Promise<T | null> => {
		if (typeof WebAssembly === "undefined") return null;
		if (supportsWasmSimd() && moduleOrPath === undefined) {
			try {
				return await loadBrowserCandidate(options.paths.browserSimd);
			} catch {
				// SIMD is an optional capability. Only this candidate is allowed to
				// fail before the scalar module is attempted.
			}
		}
		return loadBrowserCandidate(options.paths.browserScalar, moduleOrPath);
	};

	const get = (): T | null => {
		if (cached !== undefined) return cached;
		cached = loadNode();
		return cached;
	};

	const set = (module: T | null): void => {
		cached = module === null ? null : validate(module);
	};

	const initialize = (moduleOrPath?: unknown): Promise<void> => {
		if (initPromise) return initPromise;
		initPromise = (async () => {
			if (options.isModule(moduleOrPath)) {
				const webModule = moduleOrPath as WasmWebBinding<T>;
				if (typeof webModule.default === "function") {
					await webModule.default();
				}
				set(moduleOrPath);
				return;
			}
			if (cached === null) cached = undefined;
			if (moduleOrPath === undefined && get()) return;
			const browserModule = await loadBrowser(moduleOrPath);
			if (browserModule) set(browserModule);
		})().finally(() => {
			initPromise = null;
		});
		return initPromise;
	};

	const initializeModule = async (
		module: WasmWebBinding<T>,
		moduleOrPath?: unknown,
	): Promise<void> => {
		if (typeof module.default === "function") {
			await module.default(
				moduleOrPath === undefined
					? undefined
					: { module_or_path: moduleOrPath },
			);
		}
		set(validate(module));
	};

	return {
		cleanup: () => {
			cached = null;
			initPromise = null;
		},
		get,
		initialize,
		initializeModule,
		set,
	};
}
