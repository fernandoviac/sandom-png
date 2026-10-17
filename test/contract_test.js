// The committed binary agrees with its build record and with the contract
// record a host binds against. A rebuild is `deno task build:check`; this
// test needs no Rust toolchain.
import { assertEquals } from "@std/assert";
import { ABI, instantiate, wasmBytes } from "./helpers.js";

const root = new URL("../", import.meta.url);
const build = JSON.parse(await Deno.readTextFile(new URL("dist/png-codec.build.json", root)));

async function sha256(bytes) {
  return Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)), (b) => b.toString(16).padStart(2, "0")).join("");
}

Deno.test("the build record names the committed binary and sources", async () => {
  assertEquals(build.binaryDigest, await sha256(wasmBytes));
  assertEquals(build.binaryBytes, wasmBytes.byteLength);
  for (const { path, digest } of build.components) {
    assertEquals(await sha256(await Deno.readFile(new URL(path, root))), digest, path);
  }
});

Deno.test("the module is import-free and reports the recorded ABI version and record size", async () => {
  const module = await WebAssembly.compile(wasmBytes);
  assertEquals(WebAssembly.Module.imports(module), []);
  const e = await instantiate();
  assertEquals(e.png_version(), ABI.version);
  assertEquals(e.png_record_bytes(), ABI.recordBytes);
});
