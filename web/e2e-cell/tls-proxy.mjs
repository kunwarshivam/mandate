import { readFileSync } from "node:fs";
import { request } from "node:http";
import { createServer } from "node:https";

/**
 * The cell gesture's https front for the API (lane E2E, DEC-2406), started by
 * `playwright.cell.config.ts` before `next dev`: it listens on `node tls-proxy.mjs <https origin>
 * <http upstream> <key> <cert>` and forwards every request, byte for byte, to the API's local
 * port-forward.
 *
 * Why: the page is served at the API's https app origin (`https://localhost:4317`, which the API's
 * CORS and CSRF checks admit), and the port-forward is plain `http://localhost:8787`. Chromium's
 * schemeful same-site counts those as cross-site, so it never sends the API's `SameSite=Strict`
 * session cookie back, and every signed-in read answers 401. Behind this front the API is
 * `https://localhost:<port>`, same-site with the page, and the API itself is unchanged.
 *
 * Loopback only: it listens on the origin's host, refuses an upstream that is not http on
 * `localhost` or `127.0.0.1`, and logs nothing, so no cookie or token is written anywhere.
 */
const [listen, upstream, key, cert] = process.argv.slice(2);
const front = new URL(listen ?? "");
const target = new URL(upstream ?? "");
if (front.protocol !== "https:" || front.hostname !== "localhost" || !front.port) {
  throw new Error(`the front must be https://localhost:<port>, not ${listen}`);
}
if (target.protocol !== "http:" || !["localhost", "127.0.0.1"].includes(target.hostname) || !target.port) {
  throw new Error(`the upstream must be http on loopback with a port, not ${upstream}`);
}

createServer({ key: readFileSync(key), cert: readFileSync(cert) }, (inbound, outbound) => {
  const forwarded = request(
    {
      hostname: target.hostname,
      port: target.port,
      path: inbound.url,
      method: inbound.method,
      headers: { ...inbound.headers, host: target.host },
    },
    (answer) => {
      outbound.writeHead(answer.statusCode ?? 502, answer.headers);
      answer.pipe(outbound);
    },
  );
  forwarded.on("error", () => {
    outbound.writeHead(502);
    outbound.end();
  });
  inbound.pipe(forwarded);
}).listen(Number(front.port), front.hostname);
