// Writes the module's ABI constants (src/abi.rs) from the contract record
// (src/abi.json). Hosts write their own bindings against src/abi.json; this
// script emits no host source.
const root = new URL('../', import.meta.url);
const abi = JSON.parse(await Deno.readTextFile(new URL('src/abi.json', root)));
const upper = (s) => s.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toUpperCase();
const fields = Object.fromEntries(abi.fields.map((name, index) => [name, index * 4]));
let rust = '// Generated from src/abi.json by tools/generate-abi.js; do not edit.\n';
rust += `pub const VERSION: u32 = ${abi.version};\npub const RECORD_WORDS: usize = ${abi.fields.length};\n`;
for (const [name, value] of Object.entries(fields)) rust += `pub const F_${upper(name)}: usize = ${value / 4};\n`;
for (const [name, value] of Object.entries(abi.limits)) rust += `pub const LIMIT_${upper(name)}: usize = ${value};\n`;
for (const [name, value] of Object.entries(abi.flags)) rust += `pub const FLAG_${upper(name)}: u32 = ${value};\n`;
for (const [name, value] of Object.entries(abi.status)) rust += `pub const STATUS_${upper(name)}: u32 = ${value};\n`;
for (const [name, values] of Object.entries(abi.enums)) {
  rust += `pub const ${upper(name)}_NAMES: &[&str] = &${JSON.stringify(values)};\n`;
  for (const [index, value] of values.entries()) rust += `pub const ${upper(name)}_${upper(value.replaceAll('-', '_'))}: u32 = ${index};\n`;
}
const output = new URL('src/abi.rs', root);
if (Deno.args.includes('--check')) {
  if (await Deno.readTextFile(output) !== rust) throw new Error('src/abi.rs is stale; run tools/generate-abi.js');
} else {
  await Deno.writeTextFile(output, rust);
}
