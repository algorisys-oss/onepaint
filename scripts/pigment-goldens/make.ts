// Export pigmentMix goldens from happypaint (run from ../happypaint/frontend):
//   bun ../../onepaint/scripts/pigment-goldens/make.ts > ../../onepaint/crates/color/tests/fixtures/pigment.json
import { pigmentMix } from './src/engine/color/pigment';

const colors: [number, number, number][] = [
  [1, 1, 0], [0, 0, 1], [1, 0, 0], [0, 1, 0], [1, 1, 1], [0, 0, 0], [0.5, 0.5, 0.5],
  [0.9, 0.3, 0.1], [0.2, 0.6, 0.8], [0.7, 0.1, 0.6], [0.05, 0.4, 0.1], [0.95, 0.85, 0.7],
];
const ts = [0, 0.1, 0.25, 0.5, 0.75, 0.9, 1];
const out: { a: number[]; b: number[]; t: number; o: number[] }[] = [];
for (const a of colors) for (const b of colors) for (const t of ts) out.push({ a, b, t, o: pigmentMix(a, b, t) });
console.log(JSON.stringify(out));
