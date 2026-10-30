// Compresses a file with brotli, as hard as it goes, to be sent as it is with
// `Content-Encoding: br`: node scripts/brotli.mjs <file> <compressed file>
import { readFileSync, writeFileSync } from "node:fs";
import { brotliCompressSync, constants } from "node:zlib";

const [from, to] = process.argv.slice(2);
const bytes = readFileSync(from);
const compressed = brotliCompressSync(bytes, {
  params: {
    [constants.BROTLI_PARAM_QUALITY]: constants.BROTLI_MAX_QUALITY,
    [constants.BROTLI_PARAM_SIZE_HINT]: bytes.length,
  },
});
writeFileSync(to, compressed);
const mib = (n) => `${(n / 1048576).toFixed(1)} MiB`;
console.log(`${to}: ${mib(bytes.length)}, ${mib(compressed.length)} compressed`);
