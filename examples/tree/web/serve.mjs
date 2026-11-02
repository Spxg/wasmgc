import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, resolve, sep } from "node:path";
import { build } from "./build.mjs";

const directory = resolve(build(), "../..");
const types = new Map([
    [".html", "text/html; charset=utf-8"],
    [".css", "text/css; charset=utf-8"],
    [".js", "text/javascript; charset=utf-8"],
    [".mjs", "text/javascript; charset=utf-8"],
    [".json", "application/json"],
    [".svg", "image/svg+xml"],
    [".png", "image/png"],
    [".ico", "image/x-icon"],
    [".woff", "font/woff"],
    [".woff2", "font/woff2"],
    [".ttf", "font/ttf"],
    [".wasm", "application/wasm"],
]);

const server = createServer(async (request, response) => {
    try {
        const url = new URL(request.url, "http://localhost");
        let filename = resolve(directory, `.${decodeURIComponent(url.pathname)}`);
        if (filename !== directory && !filename.startsWith(directory + sep)) {
            response.writeHead(404).end("Not found");
            return;
        }

        if ((await stat(filename)).isDirectory()) {
            if (!url.pathname.endsWith("/")) {
                response.writeHead(301, { Location: `${url.pathname}/${url.search}` }).end();
                return;
            }
            filename = resolve(filename, "index.html");
        }

        const body = await readFile(filename);
        response.writeHead(200, {
            "Content-Type": types.get(extname(filename)) ?? "application/octet-stream",
            "Content-Length": body.length,
            "Cache-Control": "no-store",
        });
        response.end(request.method === "HEAD" ? undefined : body);
    } catch (error) {
        if (error.code === "ENOENT" || error.code === "ENOTDIR") {
            response.writeHead(404).end("Not found");
        } else {
            console.error(error.message);
            response.writeHead(500).end("Could not load the example");
        }
    }
});

server.on("error", error => {
    console.error(`Could not start the tree server: ${error.message}`);
    process.exitCode = 1;
});

server.listen(Number(process.env.PORT ?? 4173), "127.0.0.1", () => {
    console.log(`Tree: http://localhost:${server.address().port}/examples/tree/`);
});
