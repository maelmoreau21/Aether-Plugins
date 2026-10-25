// Optimize compiler output before size validation, hashing and signing.
import binaryen from 'binaryen';
import { readFile, writeFile, rename } from 'node:fs/promises';
import path from 'node:path';

const files = process.argv.slice(2);
if (!files.length) throw new Error('Usage: node scripts/optimize-wasm.mjs <module.wasm> [...]');
binaryen.setOptimizeLevel(2);
binaryen.setShrinkLevel(2); // Binaryen -Oz
for (const file of files) {
  if (!file.endsWith('.wasm')) throw new Error(`Expected a .wasm file: ${file}`);
  const original = await readFile(file);
  const module = binaryen.readBinary(original);
  try {
    // Match Rust wasm32 features; do not enable new instruction families.
    module.setFeatures(binaryen.Features.MVP | binaryen.Features.MutableGlobals |
      binaryen.Features.SignExt | binaryen.Features.NontrappingFPToInt |
      binaryen.Features.BulkMemory | binaryen.Features.BulkMemoryOpt | binaryen.Features.Multivalue |
      binaryen.Features.ReferenceTypes);
    module.optimize();
    if (!module.validate()) throw new Error(`Invalid optimized module: ${file}`);
    const optimized = module.emitBinary();
    if (!WebAssembly.validate(optimized)) throw new Error(`Invalid WebAssembly: ${file}`);
    if (optimized.length < original.length) {
      // Atomic replacement of compiler output only; published packages are untouched.
      const temporary = `${file}.optimized`;
      await writeFile(temporary, optimized, { flag: 'wx' });
      await rename(temporary, file);
    }
    console.log(`${path.basename(file)}: ${original.length} -> ${Math.min(original.length, optimized.length)} bytes`);
  } finally {
    module.dispose();
  }
}
