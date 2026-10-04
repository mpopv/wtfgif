import { spawnSync } from "node:child_process";

export function alternatingOrder(values, iteration, offset = 0) {
	return (iteration + offset) & 1 ? values.toReversed() : values;
}

export function runJsonWorker({
	worker,
	args = [],
	nodeArgs = [],
	cwd,
	env = {},
	label = worker,
	maxBuffer = 16 * 1024 * 1024,
}) {
	const result = spawnSync(process.execPath, [...nodeArgs, worker, ...args], {
		cwd,
		encoding: "utf8",
		env: { ...process.env, ...env },
		maxBuffer,
	});
	if (result.status !== 0) {
		throw new Error(
			[`Worker failed: ${label}`, result.stdout, result.stderr].join("\n"),
		);
	}
	try {
		return JSON.parse(result.stdout);
	} catch (error) {
		throw new Error(
			`Worker returned invalid JSON: ${label}\n${result.stdout}`,
			{
				cause: error,
			},
		);
	}
}

export function assertStableOutput(previous, current, label) {
	if (!previous) return;
	if (
		current.bytes !== previous.bytes ||
		current.outputSha256 !== previous.outputSha256
	) {
		throw new Error(`${label} output changed between fresh processes`);
	}
}
