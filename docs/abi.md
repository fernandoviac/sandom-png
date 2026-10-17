# `png-codec.wasm` ABI

`src/abi.json` is the contract record; this page explains it. ABI version 1.

## Module surface

The module has no imports. Instantiate it with an empty import object.

| Export | Signature | Meaning |
| --- | --- | --- |
| `memory` | memory | The module's only memory; grows on allocation, at most `maximumPages` (16,384 pages, 1 GiB). |
| `png_version` | `() → u32` | ABI version (`version` in `src/abi.json`). |
| `png_record_bytes` | `() → u32` | Record size in bytes: `fields.length × 4` (176). |
| `png_initialize` | `() → u32` | Creates the instance state and the retained compressor. Idempotent; returns 1. Before it, the allocation, release and codec calls fail with `abiMismatch`. |
| `png_alloc` | `(length) → handle` | Reserves a zero-filled input allocation of `length` bytes. |
| `png_ptr` | `(handle) → u32` | Current memory offset of an allocation, input or result. |
| `png_len` | `(handle) → u32` | Byte length of an allocation, input or result. |
| `png_release` | `(handle) → u32` | Frees an allocation; 1 on success. |
| `png_decode` | `(input) → result` | Decodes the PNG bytes held by an input allocation. |
| `png_encode` | `(request) → result` | Encodes from a request record held by an input allocation. |
| `png_error` | `() → u32` | Offset of the four-word error record of the last call. |
| `__data_end`, `__heap_base` | global | Linker globals; not part of the contract. |

A call that fails returns 0 and fills the error record; a call that succeeds
clears it. A failure leaves the instance usable.

## Handles and memory

Handles are non-zero, monotonically increasing integers scoped to one
instance; they are never reused. Each allocation has a role: an *input*
(from `png_alloc`) or a *result* (from `png_decode` or `png_encode`).
`png_decode` and `png_encode` take only inputs; passing a result fails with
`invalidHandle`. Inputs are borrowed for the one synchronous call that reads
them.

A result owns its record, pixel and profile bytes until `png_release`. Any
call can grow memory, so a host re-reads `memory.buffer` and `png_ptr` after
every call and copies out what it keeps before releasing. Releasing a handle
that is absent, already released, or never existed fails with `invalidHandle`.
Retained bytes across all live allocations are capped at `retainedBytes`.

## Records

A record is `fields.length` little-endian `u32` words; field *i* is at byte
offset *i × 4*. Enumerated fields hold the index of the value in the matching
`enums` list. Fields whose names end in `Low`/`High` are the two halves of a
little-endian `f64` (`gammaLow` at the lower offset).

A **result record** sits at the start of the result allocation. `pixelOffset`
and `profileOffset` are absolute memory offsets inside that allocation;
`pixelLength`/`profileLength` are byte lengths. A decode result describes the
image (`width`, `height`, `rowStride`, `sampleType`, `bitsPerSample`,
`channelLayout`, `alphaMode`, `primaries`, `transfer`, `whitePoint`, `flags`,
gamma, cICP codes, mastering-display and content-light-level values). An
encode result carries only `version`, and the PNG file as its pixel span.

A **request record** is a 176-byte input allocation. `pixelHandle` and
`profileHandle` name other input allocations; `pixelOffset`/`profileOffset`
are offsets *within* those allocations.

| Request field | Values |
| --- | --- |
| `version` | 1 |
| `width`, `height` | 1 … 2³¹−1 |
| `sampleType` | `uint8`, `uint16` or `float32` (samples little-endian; float clamped to 0…1, NaN as 0) |
| `bitsPerSample` | 8 or 16; `uint8` requires 8 and `uint16` requires 16 |
| `channelLayout`, `alphaMode` | `rgba` + `straight` (colour type 6) or `rgb` + `opaque` (colour type 2) |
| `rowStride` | Bytes between rows in the pixel span; at least one row, a multiple of the sample size. `pixelLength` must equal `(height − 1) × rowStride` plus one row, and `pixelOffset` must be sample-aligned |
| `mode` | 0: untagged, no colour chunk; `flags` and profile must be empty. 1: tagged by `primaries`/`transfer` |
| `primaries`, `transfer` (mode 1) | `srgb`/`srgb` → `sRGB`; `display-p3`/`srgb` → `iCCP` from the supplied profile; `rec2020`/`pq` or `hlg` → `cICP` |
| `flags` (mode 1) | `masteringDisplay` → `mDCv` from `masteringMax`/`masteringMin` (cd/m²); `contentLightLevel` → `cLLi` from `contentMax`/`frameAverage` |
| `compressionLevel` | 0 … 9; advisory, every value selects the same pinned compressor |

## Errors

The error record is `status`, `domain`, `field`, `offset` (`errorRecord`).
`status` is a value of `status`; `domain` is a value of `errorDomains`. In the
`pngChunk` domain `field` is the chunk type as a big-endian four-byte tag
(`IHDR`, or the pseudo-tags `size`, `sign`, `head`, `filt`, `desc`, `vers`, `zlib`)
and `offset` is the byte position in the input. `validationOrder` lists the
order in which decode checks fail; `parserRules` lists the container rules.
