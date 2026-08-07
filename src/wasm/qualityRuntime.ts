import type { WasmQualityCoreModule } from "../types";
import { isWasmQualityCoreModule } from "./contracts";
import { getWasmStatus as getSharedWasmStatus } from "./features";
import type { WasmWebBinding } from "./moduleRuntime";
import { loadRawQualityNode } from "./rawQuality";
import { supportsWasmSimd } from "./simd";

const BROWSER_SCALAR_PATH = "./wasm-quality-web/wtfgif_core.js";
const BROWSER_SIMD_PATH = "./wasm-quality-web-simd/wtfgif_core.js";

let cached: WasmQualityCoreModule | null | undefined;
let initPromise: Promise<void> | null = null;

export type WasmQualityWebModule = WasmWebBinding<WasmQualityCoreModule>;

function validate(value: unknown): WasmQualityCoreModule {
	if (!isWasmQualityCoreModule(value)) {
		throw new Error(
			"The supplied module does not implement the wtfgif quality encode-core WebAssembly contract",
		);
	}
	return value;
}

async function loadBrowserCandidate(
	path: string,
	moduleOrPath?: unknown,
): Promise<WasmQualityCoreModule> {
	const moduleUrl = new URL(path, import.meta.url).href;
	const loaded = (await import(
		/* @vite-ignore */ moduleUrl
	)) as WasmQualityWebModule;
	return initializeBrowserModule(loaded, moduleOrPath);
}

async function initializeBrowserModule(
	module: WasmQualityWebModule,
	moduleOrPath?: unknown,
): Promise<WasmQualityCoreModule> {
	if (typeof module.default !== "function") return validate(module);
	const exports = (await module.default(
		moduleOrPath === undefined ? undefined : { module_or_path: moduleOrPath },
	)) as { memory?: WebAssembly.Memory } | undefined;
	if (!(exports?.memory instanceof WebAssembly.Memory)) {
		throw new Error(
			"The wtfgif quality encoder did not export WebAssembly memory",
		);
	}
	return validate({ ...module, wasm_memory: () => exports.memory! });
}

export function setWasmQualityCoreModule(
	module: WasmQualityCoreModule | null,
): void {
	cached = module === null ? null : validate(module);
}

export function getWasmQualityCoreModule(): WasmQualityCoreModule | null {
	if (cached === undefined) cached = loadRawQualityNode() ?? null;
	return cached;
}

export function initializeGlobalWasm(moduleOrPath?: unknown): Promise<void> {
	if (initPromise) return initPromise;
	initPromise = (async () => {
		if (isWasmQualityCoreModule(moduleOrPath)) {
			setWasmQualityCoreModule(moduleOrPath);
			return;
		}
		if (
			moduleOrPath !== null &&
			typeof moduleOrPath === "object" &&
			typeof (moduleOrPath as WasmQualityWebModule).default === "function"
		) {
			setWasmQualityCoreModule(
				await initializeBrowserModule(moduleOrPath as WasmQualityWebModule),
			);
			return;
		}
		if (cached === null) cached = undefined;
		if (moduleOrPath === undefined && getWasmQualityCoreModule()) return;
		if (typeof WebAssembly === "undefined") return;
		if (supportsWasmSimd() && moduleOrPath === undefined) {
			try {
				setWasmQualityCoreModule(await loadBrowserCandidate(BROWSER_SIMD_PATH));
				return;
			} catch {
				// SIMD is optional; the scalar binding is the portable fallback.
			}
		}
		setWasmQualityCoreModule(
			await loadBrowserCandidate(BROWSER_SCALAR_PATH, moduleOrPath),
		);
	})().finally(() => {
		initPromise = null;
	});
	return initPromise;
}

export async function initializeWasmModule(
	module: WasmQualityWebModule,
	moduleOrPath?: unknown,
): Promise<void> {
	setWasmQualityCoreModule(await initializeBrowserModule(module, moduleOrPath));
}

export function getWasmStatus() {
	return getSharedWasmStatus(getWasmQualityCoreModule() !== null);
}

export function cleanupWasm(): void {
	cached = null;
	initPromise = null;
}
