# Design

## Surface

The module exports allocation, decode, encode and release functions over
integer handles, and a fixed 44-word record that describes an image or an
encode request. `src/abi.json` is the contract record: field order, enums,
flags, status codes, limits, validation order, container rules and the pinned
compression configuration. `docs/abi.md` is the written contract. A host
binding copies bytes in, calls one function, copies the result out and
releases it; it makes no codec decisions.

## Decode

Decoding checks the signature, then every chunk in file order: bounds, type
byte case, CRC, `IHDR` first and valid, `PLTE`/`tRNS`/colour chunks before
image data, one contiguous `IDAT` run, `IEND` last with no trailing bytes.
Unknown ancillary chunks are skipped after their CRC; unknown critical chunks
are unsupported. Image data inflates to exactly the scanline length the header
implies, so short, long or trailing zlib data is malformed, and every
allocation is bounded by `limits` before it is made.

Samples keep their precision: 16-bit images decode to little-endian `uint16`,
8-bit and lower to `uint8`, sub-byte grey scaled to 0…255, indexed images to
RGBA through `PLTE` and `tRNS`. Adam7 is de-interlaced. Colour metadata
follows PNG precedence: `cICP` over `iCCP` over `sRGB` over `cHRM`/`gAMA`; an
untagged image is reported as assumed sRGB with a flag. `iCCP` profile bytes
inflate within `profileBytes` and are returned unchanged. `mDCv` and `cLLi`
values are reported as recorded.

## Encode

Encoding accepts straight-alpha RGBA or opaque RGB samples as `uint8`,
`uint16` or `float32`, writes 8- or 16-bit PNG, applies the Sub filter to every
row and compresses one `IDAT` with `miniz_oxide` at level 6, window bits 15,
strategy 0. The same input produces the same bytes; `compressionLevel` is
validated and advisory. A tagged request writes `sRGB`, a Display P3 `iCCP`
from supplied profile bytes, or `cICP` for Rec. 2020 PQ/HLG, then optional
`mDCv`/`cLLi`. An untagged request writes no colour chunk.

## Runtime

One instance holds a registry of role-checked allocations and one compressor,
created by `png_initialize` before any result can be published. Handles are
never reused; results own their bytes until released; a failed call leaves
the instance usable. The module imports nothing.

## Language

Repository-owned WebAssembly written from scratch is WAT across the
programme. This codec is Rust because it is built on a vetted third-party
implementation, `miniz_oxide`; the crate closure is pinned by `Cargo.lock` and
recorded in `docs/legal/png-codec.md`. `tools/generate-abi.js` writes the
Rust constants in `src/abi.rs` from `src/abi.json`; it emits no host source.

## Exclusions

The package does not convert or manage colour, interpret ICC profiles,
premultiply, animate (APNG chunks are ignored), or load itself: hosts own
instantiation and the binding.
