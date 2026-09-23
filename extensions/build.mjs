import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "browser");
const out = resolve(dirname(fileURLToPath(import.meta.url)), "dist");
const shared = ["background.js", "content.js", "popup.html", "popup.js"];
const base = JSON.parse(readFileSync(resolve(root, "manifest.json"), "utf8"));

const firefox = structuredClone(base);
firefox.permissions = [...new Set([...(firefox.permissions || []), "contextMenus"])];

const chromium = structuredClone(base);
delete chromium.browser_specific_settings;
chromium.background = { service_worker: "background.js" };
chromium.permissions = [...new Set([...(chromium.permissions || []), "contextMenus"])];
chromium.key =
  process.env.CONGMIAO_EXTENSION_KEY ||
  "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAoL8CasT+hBQZrGCg9Nm1B/O0/56jpfjezR96bXlELgGE5teQ3q+GcDNVzRnsH/3s3cO1QmylglbfsgFeUpKjozCTsb0HmDpbN4YM6nAvoOd/z+G5laQpgS9obVHmxhgZcsDzpWLswg9qw+WazZvWfqQwln0PHP0PO0iWiGvyQtzhjOvmkr8QoOCZn+K1Y9gqc3VKmKw61uS5u3sKOJB08WEMfidRp5hCdNSg3MSNSZLu0LvqkreeRwkx0I0o6T9+zesOGXQPdSacvjVq25GIm0V+8kiIFrqK/9Mcp7eiDpoftqrSVRrPEocd40FeU1+soJE9tUz4WJwAtF3auz3dDQIDAQAB";

rmSync(out, { recursive: true, force: true });
for (const [name, manifest] of [
  ["firefox", firefox],
  ["chromium", chromium],
]) {
  const dir = resolve(out, name);
  mkdirSync(dir, { recursive: true });
  for (const file of shared) cpSync(resolve(root, file), resolve(dir, file));
  writeFileSync(resolve(dir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
}
console.log(`wrote ${out}/firefox and ${out}/chromium`);
