// Builds dist/png-codec.wasm from the Rust crate and records the build in
// dist/png-codec.build.json.
//
//   deno task build          rebuild and write dist/
//   deno task build:check    rebuild and require dist/ to match byte for byte
//
// PNG_RUST_TOOLCHAIN selects the rustup toolchain (default 1.92.0); the build
// refuses any compiler other than the pinned release. Paths are remapped so
// the binary carries neither this checkout's location nor the Cargo home.
const root = new URL("../", import.meta.url);
const rootPath = decodeURIComponent(root.pathname).replace(/\/$/, "");
const check = Deno.args.includes("--check");
const toolchain = Deno.env.get("PNG_RUST_TOOLCHAIN") ?? "1.92.0";
const expectedCompiler = "rustc 1.92.0 (ded5c06cf 2025-12-08)";
const target = "wasm32-unknown-unknown";
const exportsExpected = [
  "memory", "png_alloc", "png_decode", "png_encode", "png_error", "png_initialize",
  "png_len", "png_ptr", "png_record_bytes", "png_release", "png_version",
  "__data_end", "__heap_base",
];
const sources = ["Cargo.toml", "Cargo.lock", "src/abi.json", "src/abi.rs", "src/codec.rs", "src/lib.rs"];
const wasmURL = new URL("dist/png-codec.wasm", root);
const recordURL = new URL("dist/png-codec.build.json", root);

async function run(command, args, env = {}) {
  const result = await new Deno.Command(command, { args, cwd: rootPath, env, stdout: "piped", stderr: "inherit" }).output();
  if (!result.success) throw new Error(`${command} ${args.join(" ")} exited with ${result.code}`);
  return new TextDecoder().decode(result.stdout).trim();
}

async function sha256(bytes) {
  return Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)), (b) => b.toString(16).padStart(2, "0")).join("");
}

function lockedPackages(lock) {
  const packages = [];
  for (const block of lock.split("[[package]]").slice(1)) {
    const field = (name) => block.match(new RegExp(`^${name} = "([^"]*)"`, "m"))?.[1] ?? null;
    if (field("source") === null) continue;
    packages.push({ name: field("name"), version: field("version"), checksum: field("checksum") });
  }
  return packages;
}

const compiler = await run("rustc", [`+${toolchain}`, "--version"]);
if (compiler !== expectedCompiler) throw new Error(`PNG codec requires ${expectedCompiler}; selected ${compiler}`);

// Refreshes src/abi.rs from src/abi.json, or with --check requires it current.
await import("./generate-abi.js");

const cargoHome = Deno.realPathSync(Deno.env.get("CARGO_HOME") ?? `${Deno.env.get("HOME")}/.cargo`);
await run("cargo", [`+${toolchain}`, "build", "--locked", "--target", target, "--release"], {
  SOURCE_DATE_EPOCH: "0",
  CARGO_INCREMENTAL: "0",
  RUSTFLAGS: [
    `--remap-path-prefix=${rootPath}=/png-codec`,
    `--remap-path-prefix=${cargoHome}=/cargo`,
    "-C link-arg=--max-memory=1073741824",
    "-C link-arg=-zstack-size=1048576",
  ].join(" "),
});
const built = await Deno.readFile(new URL(`target/${target}/release/png_codec.wasm`, root));

const module = await WebAssembly.compile(built);
const imports = WebAssembly.Module.imports(module);
if (imports.length !== 0) throw new Error(`Unexpected import surface: ${JSON.stringify(imports)}`);
const exports = WebAssembly.Module.exports(module).map(({ name }) => name);
if (JSON.stringify(exports) !== JSON.stringify(exportsExpected)) {
  throw new Error(`Unexpected export surface: ${JSON.stringify(exports)}`);
}

const components = [];
for (const path of sources) components.push({ path, digest: await sha256(await Deno.readFile(new URL(path, root))) });
const record = {
  kind: "sandom/webassembly-build",
  version: 1,
  module: "png-codec",
  compiler,
  target,
  validator: "WebAssembly.compile",
  sourceDigest: await sha256(new TextEncoder().encode(components.map(({ path, digest }) => `${path}\0${digest}\n`).join(""))),
  binaryDigest: await sha256(built),
  binaryBytes: built.byteLength,
  components,
  dependencies: lockedPackages(await Deno.readTextFile(new URL("Cargo.lock", root))),
};
const recordText = `${JSON.stringify(record, null, 2)}\n`;

if (check) {
  const committed = await Deno.readFile(wasmURL);
  if (committed.byteLength !== built.byteLength || committed.some((b, i) => b !== built[i])) {
    throw new Error("dist/png-codec.wasm differs from a fresh build");
  }
  if (await Deno.readTextFile(recordURL) !== recordText) throw new Error("dist/png-codec.build.json differs from a fresh build");
} else {
  // A source checkout has no dist/: GitHub carries the source alone.
  await Deno.mkdir(new URL("dist/", root), { recursive: true });
  await Deno.writeFile(wasmURL, built);
  await Deno.writeTextFile(recordURL, recordText);
}
console.log(JSON.stringify({ module: "png-codec", check, binaryDigest: record.binaryDigest, binaryBytes: record.binaryBytes }));
