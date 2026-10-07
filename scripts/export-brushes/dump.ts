import { BRUSH_PRESETS } from '../../../happypaint/frontend/src/engine/brush/brush-presets.ts';
import { getTip } from '../../../happypaint/frontend/src/engine/brush/brush-tips.ts';
import { BRUSH_GRAINS, getGrain, grainTile } from '../../../happypaint/frontend/src/engine/brush/brush-grain.ts';
const out = process.argv[2] ?? 'brushdump';
const ids = new Set(BRUSH_PRESETS.map((p) => p.tipId));
const meta: any = { tips: {}, grains: {} };
for (const id of ids) {
  const t = getTip(id);
  if (t.id !== id || t.kind !== 'image') continue;
  const m = t.generate();
  await Bun.write(out + '/tip-' + id + '.bin', m.data);
  meta.tips[id] = { size: m.size, color: !!m.rgb };
}
const grainIds = new Set([...BRUSH_GRAINS.map((g) => g.id), ...BRUSH_PRESETS.map((p) => p.grain?.id).filter(Boolean)]);
for (const id of grainIds) {
  const g = getGrain(id as string);
  if (!g) continue;
  const t = grainTile(g);
  await Bun.write(out + '/grain-' + g.id + '.bin', t.data);
  meta.grains[g.id] = { size: t.size, name: g.name };
}
await Bun.write(out + '/meta.json', JSON.stringify(meta));
console.log(Object.keys(meta.tips).length, 'tips', Object.keys(meta.grains).length, 'grains');
