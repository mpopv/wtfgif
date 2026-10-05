import path from "node:path";
import { fileURLToPath } from "node:url";

export const root = fileURLToPath(new URL("../../", import.meta.url));
export const fromRoot = (...segments) => path.join(root, ...segments);
