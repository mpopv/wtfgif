import type { WasmEncodeCoreModule } from "../types";
import { isWasmEncodeCoreModule } from "./contracts";
import { getWasmCoreModule } from "./runtime";

let cached: WasmEncodeCoreModule | null = null;

export function setWasmEncodeCoreModule(
	module: WasmEncodeCoreModule | null,
): void {
	if (module !== null && !isWasmEncodeCoreModule(module)) {
		throw new Error(
			"The supplied module does not implement the wtfgif encode-core WebAssembly contract",
		);
	}
	cached = module;
}

export function getWasmEncodeCoreModule(): WasmEncodeCoreModule | null {
	return cached ?? getWasmCoreModule();
}
