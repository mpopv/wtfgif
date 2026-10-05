import { createServer } from "node:http";

export async function startLocalHttpServer(handler) {
	const server = createServer((request, response) => {
		Promise.resolve()
			.then(() => handler(request, response))
			.catch((error) => {
				if (response.headersSent) {
					response.destroy(
						error instanceof Error ? error : new Error(String(error)),
					);
					return;
				}
				response
					.writeHead(500)
					.end(error instanceof Error ? error.message : String(error));
			});
	});
	await new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(0, "127.0.0.1", resolve);
	});
	const address = server.address();
	if (!address || typeof address === "string") {
		server.close();
		throw new Error("Local server did not bind to a TCP port");
	}
	return {
		origin: `http://127.0.0.1:${address.port}`,
		port: address.port,
		close: () =>
			new Promise((resolve, reject) => {
				server.close((error) => (error ? reject(error) : resolve()));
			}),
	};
}

export function contentType(file) {
	if (file.endsWith(".html")) return "text/html; charset=utf-8";
	if (file.endsWith(".mjs") || file.endsWith(".js")) {
		return "text/javascript; charset=utf-8";
	}
	if (file.endsWith(".wasm")) return "application/wasm";
	if (file.endsWith(".gif")) return "image/gif";
	return "application/octet-stream";
}
