const expected = new Set([
  "package.json",
  "README.md",
  "DESIGN.md",
  "LICENSE",
  "NOTICE",
  "dist/png-codec.wasm",
  "dist/png-codec.build.json",
  "src/abi.json",
  "docs/abi.md",
  "docs/legal/png-codec.md",
  "docs/legal/licenses/rust-crates/adler2-2.0.1-0BSD.txt",
  "docs/legal/licenses/rust-crates/adler2-2.0.1-APACHE.txt",
  "docs/legal/licenses/rust-crates/adler2-2.0.1-MIT.txt",
  "docs/legal/licenses/rust-crates/miniz_oxide-0.8.9-APACHE.txt",
  "docs/legal/licenses/rust-crates/miniz_oxide-0.8.9-MIT.txt",
  "docs/legal/licenses/rust-crates/miniz_oxide-0.8.9-ZLIB.txt",
]);
const result = await new Deno.Command("npm", {
  args: ["pack", "--dry-run", "--json", "--ignore-scripts"],
  stdout: "piped",
  stderr: "piped",
}).output();
if (!result.success) throw new Error(new TextDecoder().decode(result.stderr));
const report = JSON.parse(new TextDecoder().decode(result.stdout));
const files = new Set(report[0]?.files?.map(({ path }) => path));
for (const path of expected) {
  if (!files.has(path)) throw new Error(`Package is missing ${path}`);
}
for (const path of files) {
  if (!expected.has(path)) {
    throw new Error(`Package contains unexpected ${path}`);
  }
  if (/\.(?:js|mjs|cjs|ts|tsx|jsx|d\.ts|c|cc|cpp|h|hpp|rs)$/.test(path)) {
    throw new Error(`Package contains prohibited runtime source ${path}`);
  }
}
const packageJson = JSON.parse(
  await Deno.readTextFile(new URL("../package.json", import.meta.url)),
);
if (packageJson.exports?.["./png-codec.wasm"] !== "./dist/png-codec.wasm") {
  throw new Error("Package binary export is missing");
}
if (packageJson.exports?.["./abi.json"] !== "./src/abi.json") {
  throw new Error("Package ABI record export is missing");
}
const wasm = await Deno.readFile(
  new URL("../dist/png-codec.wasm", import.meta.url),
);
const imports = WebAssembly.Module.imports(new WebAssembly.Module(wasm));
if (imports.length !== 0) {
  throw new Error(`Unexpected import surface: ${JSON.stringify(imports)}`);
}
console.log(
  JSON.stringify({
    kind: "sandom/png-package-check",
    files: files.size,
    imports,
  }),
);
