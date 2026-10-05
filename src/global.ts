import * as wtfgif from "wtfgif";

if (typeof window !== "undefined") {
	window.wtfgif = wtfgif;
}

declare global {
	interface Window {
		wtfgif: typeof wtfgif;
	}
}

export * from "wtfgif";
