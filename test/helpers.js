// Test-side view of the contract record. Field offsets are the field's index
// in src/abi.json times four, as docs/abi.md states.
const record = JSON.parse(await Deno.readTextFile(new URL("../src/abi.json", import.meta.url)));

export const ABI = Object.freeze({
  ...record,
  fields: Object.fromEntries(record.fields.map((name, index) => [name, index * 4])),
  recordBytes: record.fields.length * 4,
});

export const wasmBytes = await Deno.readFile(new URL("../dist/png-codec.wasm", import.meta.url));

export function assert(ok, message) {
  if (!ok) throw new Error(message);
}

export async function instantiate() {
  const { instance } = await WebAssembly.instantiate(wasmBytes, {});
  const e = instance.exports;
  assert(e.png_initialize() === 1, "initialization failed");
  return e;
}

export function writeBytes(e, bytes) {
  const handle = e.png_alloc(bytes.length);
  assert(handle, "allocation failed");
  new Uint8Array(e.memory.buffer, e.png_ptr(handle), bytes.length).set(bytes);
  return handle;
}

export function resultBytes(e, handle) {
  const view = new DataView(e.memory.buffer, e.png_ptr(handle), ABI.recordBytes);
  return new Uint8Array(
    e.memory.buffer,
    view.getUint32(ABI.fields.pixelOffset, true),
    view.getUint32(ABI.fields.pixelLength, true),
  ).slice();
}

export function errorRecord(e) {
  return Array.from(new Uint32Array(e.memory.buffer, e.png_error(), 4));
}

// Encodes width×height straight-alpha RGBA8 samples and returns the PNG bytes.
export function encodeRgba8(e, samples, width, height) {
  const pixels = writeBytes(e, samples);
  const request = new Uint8Array(ABI.recordBytes), view = new DataView(request.buffer);
  const fields = {
    version: ABI.version, width, height, rowStride: width * 4,
    sampleType: ABI.enums.sampleType.indexOf("uint8"), bitsPerSample: 8,
    channelLayout: ABI.enums.channelLayout.indexOf("rgba"),
    alphaMode: ABI.enums.alphaMode.indexOf("straight"),
    pixelHandle: pixels, pixelLength: samples.length, compressionLevel: 6,
  };
  for (const [key, value] of Object.entries(fields)) view.setUint32(ABI.fields[key], value, true);
  const requestHandle = writeBytes(e, request);
  const result = e.png_encode(requestHandle);
  assert(result, `encode failed: ${errorRecord(e)}`);
  const png = resultBytes(e, result);
  for (const handle of [result, requestHandle, pixels]) assert(e.png_release(handle) === 1, "release failed");
  return png;
}
