import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { join } from "node:path";
import { build } from "./build.mjs";

const directory = build();
const files = new Map([
    ["/", ["index.html", "text/html; charset=utf-8"]],
    ["/index.html", ["index.html", "text/html; charset=utf-8"]],
    ["/style.css", ["style.css", "text/css; charset=utf-8"]],
    ["/main.mjs", ["main.mjs", "text/javascript; charset=utf-8"]],
    ["/palette.wasm", ["palette.wasm", "application/wasm"]],
]);

const server = createServer(async (request, response) => {
    const path = new URL(request.url, "http://localhost").pathname;
    const file = files.get(path);
    if (!file) {
        response.writeHead(404).end("Not found");
        return;
    }

    try {
        const [filename, type] = file;
        const body = await readFile(join(directory, filename));
        response.writeHead(200, {
            "Content-Type": type,
            "Content-Length": body.length,
            "Cache-Control": "no-store",
        });
        response.end(request.method === "HEAD" ? undefined : body);
    } catch (error) {
        console.error(error.message);
        response.writeHead(500).end("Could not load the example");
    }
});

server.on("error", error => {
    console.error(`Could not start the palette server: ${error.message}`);
    process.exitCode = 1;
});

server.listen(Number(process.env.PORT ?? 4173), "0.0.0.0", () => {
    console.log(`Palette: http://localhost:${server.address().port}`);
});
