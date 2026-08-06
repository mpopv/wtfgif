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
	if (typeof loaded.default === "function") {
		await loaded.default(moduleOrPath);
	}
	return validate(loaded);
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
			const module = moduleOrPath as WasmQualityWebModule;
			if (typeof module.default === "function") await module.default();
			setWasmQualityCoreModule(module);
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
	if (typeof module.default === "function") {
		await module.default(
			moduleOrPath === undefined ? undefined : { module_or_path: moduleOrPath },
		);
	}
	setWasmQualityCoreModule(module);
}

export function getWasmStatus() {
	return getSharedWasmStatus(getWasmQualityCoreModule() !== null);
}

export function cleanupWasm(): void {
	cached = null;
	initPromise = null;
}
