# @sandom/png

Status: extracted from Sandom (`vendor/png-wasm-rs/`, `vendor/png-codec.wasm`)
with byte-identical output. Not yet published.

This project owns one import-free WebAssembly module, `dist/png-codec.wasm`,
that decodes PNG files to native-precision samples with their colour metadata
and encodes RGBA or RGB samples to deterministic PNG files. It does not convert
colour, interpret ICC profiles, composite, or resample. The package contains no
JavaScript, TypeScript or `.d.ts` files: a host writes its own binding against
the contract record `src/abi.json` ([docs/abi.md](docs/abi.md)). Tests and
repository tools use JavaScript.

The codec is Rust built on the vetted, unmodified `miniz_oxide` DEFLATE
implementation; the PNG container and sample policy are this package's own
source. See [the provenance record](docs/legal/png-codec.md).

## Packages

- npm: `@sandom/png`
- library set: `https://cdn.urania-libs.com`, under `sandom-png/dist/png-codec.wasm`

The package ships `dist/png-codec.wasm`, its build record
`dist/png-codec.build.json`, `src/abi.json`, `docs/`, and the licence files.

## Consumers

Sandom's handwritten binding (`src/png-codec.js`, `src/png-abi.js`) loads the
module from the library set, or from this npm package in an npm install.

## Commands

```sh
deno task build          # rustc 1.92.0 + wasm32-unknown-unknown → dist/
deno task build:check    # rebuild; dist/ must match byte for byte
deno task test           # raw-ABI tests against dist/png-codec.wasm
deno task package:check  # npm pack surface and import-free binary
```

`PNG_RUST_TOOLCHAIN` selects a rustup toolchain; the build still refuses any
compiler other than `rustc 1.92.0 (ded5c06cf 2025-12-08)`.

See [DESIGN.md](DESIGN.md).
