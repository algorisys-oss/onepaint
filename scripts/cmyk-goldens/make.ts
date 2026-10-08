// Export CMYK goldens from happypaint (run from ../happypaint/frontend):
//   bun ../../onepaint/scripts/cmyk-goldens/make.ts > ../../onepaint/crates/color/tests/fixtures/cmyk.json
import { separateRgb, cmykToProofRgb, rgbaToCmykBytes, builtinPressProfile } from './src/engine/color/cmyk';
import { encodeCmykTiff } from './src/utils/tiff-cmyk';

const colors: [number, number, number][] = [
  [1, 1, 1], [0, 0, 0], [0.5, 0.5, 0.5], [1, 0, 0], [0, 1, 0], [0, 0, 1], [0.8, 0.7, 0.95], [0.2, 0.6, 0.3], [0.95, 0.85, 0.4], [0.1, 0.1, 0.4],
];
const sep = colors.map((c) => ({ rgb: c, cmyk: separateRgb(c[0], c[1], c[2]), proof: cmykToProofRgb(separateRgb(c[0], c[1], c[2])) }));
const rgba = new Uint8ClampedArray([255, 0, 0, 255, 0, 128, 255, 255, 30, 200, 90, 255, 250, 250, 250, 255]);
const lutCmyk = Array.from(rgbaToCmykBytes(rgba));
const p = builtinPressProfile();
const tiff = await encodeCmykTiff(rgba, 2, 2, 300).arrayBuffer();
const probe = [0, 1000, 5000, 20000, 35936].map((i) => Array.from(p.proofLut.slice(i * 4, i * 4 + 4)));
console.log(JSON.stringify({ sep, lutCmyk, white: p.white, black: p.black, tiff: Array.from(new Uint8Array(tiff)), probe }));
