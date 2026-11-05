import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, resolve, sep } from "node:path";
import { build } from "./build.mjs";

const directory = build();
const types = {
    ".html": "text/html; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".mjs": "text/javascript; charset=utf-8",
    ".wasm": "application/wasm",
};

const server = createServer(async (request, response) => {
    try {
        const pathname = new URL(request.url, "http://localhost").pathname;
        const filename = resolve(directory, `.${decodeURIComponent(pathname === "/" ? "/index.html" : pathname)}`);
        if (!filename.startsWith(directory + sep)) {
            response.writeHead(404).end();
            return;
        }

        const body = await readFile(filename);
        response.writeHead(200, {
            "Content-Type": types[extname(filename)] ?? "application/octet-stream",
            "Cache-Control": "no-store",
        });
        response.end(body);
    } catch {
        response.writeHead(404).end();
    }
});

server.listen(Number(process.env.PORT ?? 8080), "0.0.0.0", () => {
    console.log(`http://0.0.0.0:${server.address().port}`);
});
