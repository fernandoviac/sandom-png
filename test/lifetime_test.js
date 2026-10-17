// Handle lifetime through the raw exports. Moved from Sandom's
// tools/png-codec-smoke.js (rawLifetimeSmoke); the input PNG is now encoded
// by the module itself instead of Sandom's fixture builder.
import { assert, encodeRgba8, instantiate, resultBytes, writeBytes } from "./helpers.js";

Deno.test("results survive memory growth and sibling release; released handles stay dead", async () => {
  const e = await instantiate();
  const png = encodeRgba8(e, new Uint8Array([7, 19, 31, 128]), 1, 1);
  const input = writeBytes(e, png);
  const a = e.png_decode(input), b = e.png_decode(input);
  assert(a && b, "raw decode failed");
  const before = resultBytes(e, a), growth = e.png_alloc(16 * 1024 * 1024);
  assert(growth, "growth allocation failed");
  assert(resultBytes(e, a).every((v, i) => v === before[i]), "growth corrupted retained result");
  assert(e.png_release(a) === 1, "release failed");
  assert(e.png_ptr(a) === 0, "stale handle accepted");
  assert(resultBytes(e, b).every((v, i) => v === before[i]), "release corrupted sibling result");
  assert(e.png_decode(b) === 0, "result accepted as input handle");
  assert(e.png_alloc(0xffffffff) === 0, "unbounded allocation accepted");
  for (const h of [growth, b, input]) assert(e.png_release(h) === 1, "release failed");
  assert(e.png_release(input) === 0, "double release accepted");
});

Deno.test("an RGBA8 image round-trips through encode and decode", async () => {
  const e = await instantiate();
  const samples = Uint8Array.from({ length: 9 * 7 * 4 }, (_, i) => (i * 137 + 29) & 255);
  const input = writeBytes(e, encodeRgba8(e, samples, 9, 7));
  const decoded = e.png_decode(input);
  assert(decoded, "decode failed");
  assert(resultBytes(e, decoded).every((v, i) => v === samples[i]), "round trip changed samples");
  for (const h of [decoded, input]) assert(e.png_release(h) === 1, "release failed");
});
