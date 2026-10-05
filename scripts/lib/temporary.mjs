import { mkdtempSync, rmSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

export async function withTemporaryDirectory(prefix, operation) {
	const directory = await mkdtemp(path.join(tmpdir(), prefix));
	try {
		return await operation(directory);
	} finally {
		await rm(directory, { force: true, recursive: true, maxRetries: 5 });
	}
}

export function withTemporaryDirectorySync(prefix, operation) {
	const directory = mkdtempSync(path.join(tmpdir(), prefix));
	try {
		return operation(directory);
	} finally {
		rmSync(directory, { force: true, recursive: true, maxRetries: 5 });
	}
}
