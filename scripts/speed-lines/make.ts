// Export speed-line goldens from happypaint (run from ../happypaint/frontend):
//   bun ../../onepaint/scripts/speed-lines/make.ts > ../../onepaint/crates/stroke/tests/fixtures/speed_lines.json
import { focusLines, speedLines } from './src/engine/stroke/speed-lines';

const focus = focusLines({ center: { x: 400, y: 300 }, count: 24, innerRadius: 90, outerRadius: 500, jitter: 0.35, angleJitter: 0.7, seed: 1234 });
const speed = speedLines({ angle: 0.3, count: 16, width: 800, height: 600, jitter: 0.35, spacingJitter: 0.6, seed: 99 });
const pts = (s: typeof focus) => s.map((p) => p.anchors.map((a) => [a.x, a.y]));
console.log(JSON.stringify({ focus: pts(focus), speed: pts(speed) }));
