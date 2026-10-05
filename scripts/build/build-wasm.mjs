import { buildWasmVariant } from "./build-wasm-variant.mjs";
import { selectWasmVariants } from "./wasm-variants.mjs";

for (const variant of selectWasmVariants(process.argv.slice(2))) {
	buildWasmVariant(variant);
}
