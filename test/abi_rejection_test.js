import { assertEquals } from "@std/assert";
import { ABI, assert, wasmBytes } from "./helpers.js";
export async function runPngAbiRejectionSmoke(wasmBytes) {
  const {instance}=await WebAssembly.instantiate(wasmBytes,{}),e=instance.exports;e.png_initialize();
  const samples=e.png_alloc(9);assert(samples,'sample allocation failed');
  new Uint8Array(e.memory.buffer,e.png_ptr(samples),9).set([7,19,31,128,0,0,0,0,0]);
  const fields={version:ABI.version,width:1,height:1,rowStride:4,sampleType:ABI.enums.sampleType.indexOf('uint8'),bitsPerSample:8,channelLayout:ABI.enums.channelLayout.indexOf('rgba'),alphaMode:ABI.enums.alphaMode.indexOf('straight'),pixelHandle:samples,pixelLength:4,compressionLevel:6};
  const cases=[
    ['version',{version:999},'abiMismatch'],
    ['span',{pixelOffset:8,pixelLength:4},'invalidRequest'],
    ['alignment',{pixelOffset:1,pixelLength:8,rowStride:8,sampleType:ABI.enums.sampleType.indexOf('uint16'),bitsPerSample:16},'invalidRequest'],
    ['sample',{sampleType:99},'invalidRequest'],
    ['flags',{flags:128},'invalidRequest'],
    ['primaries',{primaries:99},'invalidRequest'],
    ['transfer',{transfer:99},'invalidRequest'],
    ['white',{whitePoint:99},'invalidRequest'],
    ['level',{compressionLevel:10},'invalidRequest'],
    ['missing-handle',{pixelHandle:0xffffffff},'invalidHandle'],
  ];
  const resultBytes=h=>{const p=e.png_ptr(h),v=new DataView(e.memory.buffer,p,ABI.recordBytes);return new Uint8Array(e.memory.buffer,v.getUint32(ABI.fields.pixelOffset,true),v.getUint32(ABI.fields.pixelLength,true)).slice();};
  for(const [name,overrides,code]of cases){const record=new Uint8Array(ABI.recordBytes),view=new DataView(record.buffer);
    for(const [key,value]of Object.entries({...fields,...overrides}))view.setUint32(ABI.fields[key],value,true);
    const request=e.png_alloc(record.length);assert(request,'request allocation failed');new Uint8Array(e.memory.buffer,e.png_ptr(request),record.length).set(record);
    const result=e.png_encode(request);assert(result===0,`${name}: invalid raw request accepted`);
    const status=new Uint32Array(e.memory.buffer,e.png_error(),4)[0];assert(status===ABI.status[code],`${name}: wrong status ${status}`);
    record.fill(0);for(const [key,value]of Object.entries(fields))view.setUint32(ABI.fields[key],value,true);
    new Uint8Array(e.memory.buffer,e.png_ptr(request),record.length).set(record);
    const recovered=e.png_encode(request);assert(recovered,`${name}: failure poisoned encode`);
    const png=resultBytes(recovered),input=e.png_alloc(png.length);assert(input,'recovery input allocation failed');
    new Uint8Array(e.memory.buffer,e.png_ptr(input),png.length).set(png);
    const decoded=e.png_decode(input);assert(decoded,`${name}: recovery output is not PNG`);
    assert(resultBytes(decoded).every((v,i)=>v===[7,19,31,128][i]),`${name}: recovery samples changed`);
    for(const h of [decoded,input,recovered,request])assert(e.png_release(h),'release failed');
  }
  assert(e.png_release(samples),'release failed');return {rejections:cases.length,recovery:true};
}
Deno.test("invalid encode requests reject with their status and do not poison the instance", async () => {
  assertEquals(await runPngAbiRejectionSmoke(wasmBytes), { rejections: 10, recovery: true });
});
