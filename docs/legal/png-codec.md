# PNG codec provenance

Moved from Sandom's `docs/legal/rust-image-decoders.md` (PNG codec section)
and `docs/legal/generated-artifacts.md` (artifact row and identity) when the
codec left Sandom's `vendor/png-wasm-rs/`. The binary is byte-identical to
Sandom's last committed `vendor/png-codec.wasm`.

| Field | Value |
| --- | --- |
| Package source | `Cargo.toml`, `Cargo.lock`, `src/lib.rs`, `src/codec.rs`, `src/abi.rs` (written from `src/abi.json` by `tools/generate-abi.js`), Apache-2.0 |
| Runtime artifact | `dist/png-codec.wasm` — SHA-256 `fecc43c21b5c1f2e34b953d4232c0e3bc2851da0f04ffefbd059cccf0549a2e3`, 149,840 bytes |
| Build record | `dist/png-codec.build.json`: compiler, target, per-source digests, binary digest, locked crate closure |
| Lockfile | SHA-256 `9a27b3a0fc6a29e3cafb40727cf523e6c8873528b7ac3ffdb9ffcec4be359c82` |
| Compiler / target | `rustc 1.92.0 (ded5c06cf 2025-12-08)`, `wasm32-unknown-unknown` |
| Imports | None |

The complete third-party closure is unmodified `miniz_oxide 0.8.9` →
`adler2 2.0.1`, with registry checksums in `Cargo.lock`. `miniz_oxide` offers
MIT, Zlib or Apache-2.0; `adler2` offers 0BSD, MIT or Apache-2.0. Exact offered
license texts are retained in [licenses/rust-crates/](licenses/rust-crates/).
The package `NOTICE` includes the selected MIT notice for `miniz_oxide` and
0BSD notice for `adler2`.

The package owns PNG chunk, sample, Adam7 and filter interpretation and
deterministic encoding. It uses miniz DEFLATE at fixed level 6, window bits 15
and strategy 0; the request's `compressionLevel` is validated and advisory.
ICC bytes cross unchanged; the codec does not interpret a profile.

## Rebuild

`tools/build-wasm.js` gates the exact compiler version, refreshes `src/abi.rs`,
remaps the checkout and Cargo home source paths, sets `SOURCE_DATE_EPOCH=0`
and disables incremental compilation. Release settings are optimization level
3, LTO, one codegen unit, aborting panics and stripping; the linker uses a
1 MiB stack and 1 GiB maximum memory. The build compiles the result with
`WebAssembly.compile` and refuses any import or an unexpected export.

```sh
rustup target add --toolchain 1.92.0 wasm32-unknown-unknown
deno task build
# An installed alias is allowed only when the exact compiler gate passes:
PNG_RUST_TOOLCHAIN=stable deno task build
deno task build:check
deno task test
```

A changed manifest, lockfile, source, toolchain or output digest is a new
runtime artifact: commit the source, `dist/png-codec.wasm` and
`dist/png-codec.build.json` together, and update this record.
