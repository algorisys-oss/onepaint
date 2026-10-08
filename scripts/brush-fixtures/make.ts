// Builds small brush files for onepaint's import tests and records what
// happypaint's own parsers make of them (the goldens):
//   cd ../happypaint/frontend && bun run ../../onepaint/scripts/brush-fixtures/make.ts <out-dir>
// Writes v1.abr, v2.abr, v6.abr, one.brush, set.brushset, chalk.happybrush and
// golden.json into <out-dir>.
import { deflateSync } from 'node:zlib';
// Run from happypaint/frontend: its modules resolve against the working dir.
const src = `${process.cwd()}/src`;
const { parseAbr } = await import(`${src}/storage/brush-import/abr`);
const { mapAbrDescriptor } = await import(`${src}/storage/brush-import/abr-mapping`);
const { parseProcreateFile } = await import(`${src}/storage/brush-import/procreate`);
const { writeZip } = await import(`${src}/storage/brush-import/zip`);
const { BRUSH_PRESETS } = await import(`${src}/engine/brush/brush-presets`);

const out = process.argv[2] ?? '.';

// ── Big-endian byte writer ──────────────────────────────────────────────────
class W {
  b: number[] = [];
  u8(v: number) { this.b.push(v & 255); return this; }
  i16(v: number) { return this.u8(v >> 8).u8(v); }
  i32(v: number) { return this.u8(v >>> 24).u8(v >>> 16).u8(v >>> 8).u8(v); }
  f64(v: number) { const d = new DataView(new ArrayBuffer(8)); d.setFloat64(0, v); for (let i = 0; i < 8; i++) this.u8(d.getUint8(i)); return this; }
  bytes(a: ArrayLike<number>) { for (let i = 0; i < a.length; i++) this.u8(a[i]); return this; }
  ascii(s: string) { for (const c of s) this.u8(c.charCodeAt(0)); return this; }
  ustr(s: string) { this.i32(s.length + 1); for (const c of s) this.i16(c.charCodeAt(0)); return this.i16(0); }
  pascal(s: string) { this.u8(s.length); return this.ascii(s); }
  pad4() { while (this.b.length % 4) this.u8(0); return this; }
  get len() { return this.b.length; }
  out() { return new Uint8Array(this.b); }
}

function packbits(row: number[]): number[] {
  // Simple encoder: runs of ≥3 as repeats, everything else as literals.
  const o: number[] = [];
  let i = 0;
  while (i < row.length) {
    let run = 1;
    while (i + run < row.length && row[i + run] === row[i] && run < 128) run++;
    if (run >= 3) { o.push((1 - run) & 255, row[i]); i += run; continue; }
    let j = i;
    const lit: number[] = [];
    while (j < row.length && lit.length < 128) {
      if (j + 2 < row.length && row[j] === row[j + 1] && row[j] === row[j + 2]) break;
      lit.push(row[j++]);
    }
    o.push(lit.length - 1, ...lit);
    i = j;
  }
  return o;
}

function ellipse(w: number, h: number): number[] {
  const d: number[] = [];
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const u = (x + 0.5 - w / 2) / (w / 2), v = (y + 0.5 - h / 2) / (h / 2);
    const r = Math.sqrt(u * u + v * v);
    d.push(r >= 1 ? 0 : Math.round(255 * Math.min(1, (1 - r) * 3)));
  }
  return d;
}

function sampled8rle(w: W, data: number[], width: number, height: number) {
  w.i16(8).u8(1);
  const rows: number[][] = [];
  for (let y = 0; y < height; y++) rows.push(packbits(data.slice(y * width, (y + 1) * width)));
  for (const r of rows) w.i16(r.length);
  for (const r of rows) w.bytes(r);
}

// ── v1 / v2 ─────────────────────────────────────────────────────────────────
function computed(version: number, name: string, dia: number, hard: number, spacing: number, angle = 0, round = 100) {
  const r = new W().i32(0).i16(spacing);
  if (version === 2) r.ustr(name);
  r.u8(1).i16(dia).i16(round).i16(angle).i16(hard);
  return r.out();
}
function sampledV12(version: number, name: string, data: number[], width: number, height: number, rle: boolean) {
  const r = new W().i32(0).i16(25);
  if (version === 2) r.ustr(name);
  r.u8(1).i16(0).i16(0).i16(height).i16(width);
  r.i32(0).i32(0).i32(height).i32(width);
  if (rle) sampled8rle(r, data, width, height);
  else r.i16(8).u8(0).bytes(data);
  return r.out();
}
function abrV12(version: number, brushes: [number, Uint8Array][]) {
  const w = new W().i16(version).i16(brushes.length);
  for (const [type, rec] of brushes) w.i16(type).i32(rec.length).bytes(rec);
  return w.out();
}

const tipA = ellipse(24, 16);
const tipB: number[] = [];
for (let i = 0; i < 10 * 12; i++) tipB.push((i * 37) % 256);

const v1 = abrV12(1, [[1, computed(1, '', 19, 50, 25)], [2, sampledV12(1, '', tipA, 24, 16, false)]]);
const v2 = abrV12(2, [
  [1, computed(2, 'Round 9', 9, 80, 30, 45, 50)],
  [2, sampledV12(2, 'Sampled ellipse', tipA, 24, 16, true)],
  [3, new Uint8Array([1, 2, 3, 4])],
]);

// ── v6 (subversion 2) ───────────────────────────────────────────────────────
const UUID1 = 'f1c2a3b4-0000-1111-2222-333344445555';
const UUID2 = 'a9b8c7d6-9999-8888-7777-666655554444';

function sampRecord(uuid: string, width: number, height: number, body: (w: W) => void) {
  const r = new W();
  const id = new W().pascal(uuid);
  while (id.len < 37) id.u8(0);
  r.bytes(id.out());
  r.bytes(new Uint8Array(264));
  r.i32(0).i32(0).i32(height).i32(width);
  body(r);
  const rec = r.out();
  const w = new W().i32(rec.length).bytes(rec).pad4();
  return w.out();
}

// Action Descriptor writer.
type V = ['Objc', string, [string, V][]] | ['VlLs', V[]] | ['doub', number] | ['UntF', string, number] | ['TEXT', string] | ['enum', string, string] | ['long', number] | ['bool', boolean] | ['comp', number] | ['tdta', number[]];
function key(w: W, k: string) {
  if (k.length === 4) w.i32(0).ascii(k);
  else w.i32(k.length).ascii(k);
}
function val(w: W, v: V) {
  w.ascii(v[0]);
  valBody(w, v);
}
function valBody(w: W, v: V) {
  switch (v[0]) {
    case 'Objc': w.ustr(''); key(w, v[1]); w.i32(v[2].length); for (const [k, x] of v[2]) { key(w, k); val(w, x); } break;
    case 'VlLs': w.i32(v[1].length); for (const x of v[1]) val(w, x); break;
    case 'doub': w.f64(v[1]); break;
    case 'UntF': w.ascii(v[1]).f64(v[2]); break;
    case 'TEXT': w.ustr(v[1]); break;
    case 'enum': key(w, v[1]); key(w, v[2]); break;
    case 'long': w.i32(v[1]); break;
    case 'bool': w.u8(v[1] ? 1 : 0); break;
    case 'comp': w.i32(0).i32(v[1]); break;
    case 'tdta': w.i32(v[1].length).bytes(v[1]); break;
  }
}
const pct = (n: number): V => ['UntF', '#Prc', n];
const variance = (ctl: number, jitter: number): V => ['Objc', 'brVr', [['bVTy', ['long', ctl]], ['fStp', ['long', 25]], ['jitter', pct(jitter)]]];

const chalkPreset: V = ['Objc', 'brushPreset', [
  ['Nm  ', ['TEXT', 'Chalk Dust']],
  ['Brsh', ['Objc', 'sampledBrush', [
    ['Dmtr', ['UntF', '#Pxl', 45]], ['Angl', ['UntF', '#Ang', 30]], ['Rndn', pct(60)], ['Spcn', pct(40)],
    ['Intr', ['bool', true]], ['flipX', ['bool', true]], ['flipY', ['bool', false]], ['sampledData', ['TEXT', UUID1]],
  ]]],
  ['mode', ['enum', 'BlnM', 'Nrml']],
  ['Hrdn', ['doub', 0.5]],
  ['useTipDynamics', ['bool', true]],
  ['szVr', variance(2, 20)], ['minimumDiameter', pct(30)],
  ['angleDynamics', variance(7, 10)],
  ['roundnessDynamics', variance(0, 25)], ['minimumRoundness', pct(40)],
  ['tiltScale', pct(100)],
  ['usePaintDynamics', ['bool', true]],
  ['opVr', variance(2, 0)], ['prVr', variance(0, 30)],
  ['useScatter', ['bool', true]], ['scatterDynamics', variance(0, 120)], ['Cnt ', ['long', 2]],
  ['Wtdg', ['bool', true]], ['Rpt ', ['bool', true]], ['Nose', ['bool', true]],
  ['dualBrush', ['Objc', 'dualBrush', [['useDualBrush', ['bool', true]], ['Brsh', ['Objc', 'sampledBrush', [['sampledData', ['TEXT', UUID2]]]]]]]],
  ['useTexture', ['bool', true]], ['useColorDynamics', ['bool', false]], ['useBrushPose', ['bool', true]],
  ['blob', ['tdta', [1, 2, 3]]], ['big', ['comp', 7]],
]];
const leafPreset: V = ['Objc', 'brushPreset', [
  ['Nm  ', ['TEXT', 'Leaf']],
  ['Brsh', ['Objc', 'sampledBrush', [['Dmtr', ['UntF', '#Pxl', 20]], ['Spcn', pct(25)], ['Intr', ['bool', false]], ['sampledData', ['TEXT', UUID2]]]]],
]];
const roundPreset: V = ['Objc', 'brushPreset', [
  ['Nm  ', ['TEXT', 'Hard Round']],
  ['Brsh', ['Objc', 'computedBrush', [['Dmtr', ['UntF', '#Pxl', 10]], ['Hrdn', pct(100)]]]],
]];

function block(keyName: string, body: Uint8Array) {
  return new W().ascii('8BIM').ascii(keyName).i32(body.length).bytes(body).out();
}

const samp = new W()
  .bytes(sampRecord(UUID1, 24, 16, (w) => sampled8rle(w, tipA, 24, 16)))
  .bytes(sampRecord(UUID2, 10, 12, (w) => { w.i16(16).u8(0); for (const v of tipB) w.u8(v).u8(0x7f); }))
  .out();
const desc = new W().i32(16);
valBody(desc, ['Objc', 'null', [['Brsh', ['VlLs', [leafPreset, roundPreset, chalkPreset]]]]]);

function pattern(name: string, mode: number, w: number, h: number, planes: { data: number[]; zlib: boolean }[]) {
  const r = new W().i32(1).i32(mode).i16(h).i16(w).ustr(name).pascal('pat-' + name);
  const vma = new W();
  for (const p of planes) {
    const data = p.zlib ? Array.from(deflateSync(new Uint8Array(p.data))) : p.data;
    const ch = new W().i32(8).i32(0).i32(0).i32(h).i32(w).i16(8).u8(p.zlib ? 1 : 0).bytes(data);
    vma.i32(1).i32(ch.len).bytes(ch.out());
  }
  r.i32(3).i32(vma.len + 20).i32(0).i32(0).i32(h).i32(w).i32(planes.length).bytes(vma.out());
  const rec = r.out();
  return new W().i32(rec.length).bytes(rec).pad4().out();
}
const gray: number[] = [];
for (let i = 0; i < 64; i++) gray.push(i * 4);
const rp: number[] = [], gp: number[] = [], bp: number[] = [];
for (let i = 0; i < 16; i++) { rp.push(i * 16); gp.push(255 - i * 16); bp.push(128); }
const patt = new W()
  .bytes(pattern('Gradient', 1, 8, 8, [{ data: gray, zlib: true }]))
  .bytes(pattern('Colour', 3, 4, 4, [{ data: rp, zlib: false }, { data: gp, zlib: false }, { data: bp, zlib: false }]))
  .out();

const v6 = new W().i16(6).i16(2)
  .bytes(block('samp', samp))
  .bytes(block('patt', patt))
  .bytes(block('desc', desc.out()))
  .out();

// ── PNG (8-bit gray or RGBA) ────────────────────────────────────────────────
const CRC = (() => { const t = new Uint32Array(256); for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; } return t; })();
function crc32(b: Uint8Array) { let c = 0xffffffff; for (const x of b) c = CRC[(c ^ x) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; }
function chunk(w: W, type: string, data: Uint8Array) {
  const td = new W().ascii(type).bytes(data).out();
  w.i32(data.length).bytes(td).i32(crc32(td));
}
function png(width: number, height: number, channels: 1 | 4, px: number[]) {
  const w = new W().bytes([137, 80, 78, 71, 13, 10, 26, 10]);
  chunk(w, 'IHDR', new W().i32(width).i32(height).u8(8).u8(channels === 1 ? 0 : 6).u8(0).u8(0).u8(0).out());
  const raw: number[] = [];
  for (let y = 0; y < height; y++) { raw.push(0); for (let x = 0; x < width * channels; x++) raw.push(px[y * width * channels + x]); }
  chunk(w, 'IDAT', new Uint8Array(deflateSync(new Uint8Array(raw))));
  chunk(w, 'IEND', new Uint8Array(0));
  return w.out();
}
// White disc on opaque black (Procreate's usual polarity).
const shape: number[] = [];
for (let y = 0; y < 32; y++) for (let x = 0; x < 32; x++) {
  const r = Math.hypot(x + 0.5 - 16, y + 0.5 - 16);
  const v = r < 10 ? 255 : 0;
  shape.push(v, v, v, 255);
}
const shapePng = png(32, 32, 4, shape);
const grainPx: number[] = [];
for (let i = 0; i < 16 * 16; i++) grainPx.push((i * 53) % 256);
const grainPng = png(16, 16, 1, grainPx);
// Dark ink on white (the other polarity).
const ink: number[] = [];
for (let y = 0; y < 20; y++) for (let x = 0; x < 20; x++) { const v = x > 5 && x < 14 ? 30 : 255; ink.push(v, v, v, 255); }
const inkPng = png(20, 20, 4, ink);

// ── bplist00 writer (NSKeyedArchiver shape) ─────────────────────────────────
type P = null | boolean | number | { real: number } | string | { uid: number } | P[] | { dict: [string, P][] };
function bplist(top: P): Uint8Array {
  const objs: Uint8Array[] = [];
  const add = (p: P): number => {
    const idx = objs.length;
    objs.push(new Uint8Array(0));
    const w = new W();
    const len = (type: number, n: number) => { if (n < 15) w.u8((type << 4) | n); else { w.u8((type << 4) | 15).u8(0x11).i16(n); } };
    if (p === null) w.u8(0x00);
    else if (p === true) w.u8(0x09);
    else if (p === false) w.u8(0x08);
    else if (typeof p === 'number') {
      if (p < 0) { w.u8(0x13).i32(-1).i32(p); }
      else if (p < 256) w.u8(0x10).u8(p);
      else if (p < 65536) w.u8(0x11).i16(p);
      else w.u8(0x12).i32(p);
    } else if (typeof p === 'string') {
      if (/^[\x00-\x7f]*$/.test(p)) { len(5, p.length); w.ascii(p); }
      else { len(6, p.length); for (const c of p) w.i16(c.charCodeAt(0)); }
    } else if (Array.isArray(p)) {
      const refs = p.map(add);
      len(10, refs.length); for (const r of refs) w.i16(r);
    } else if ('real' in p) { w.u8(0x23).f64(p.real); }
    else if ('uid' in p) { w.u8(0x80).u8(p.uid); }
    else {
      const ks = p.dict.map(([k]) => add(k));
      const vs = p.dict.map(([, v]) => add(v));
      len(13, ks.length); for (const r of ks) w.i16(r); for (const r of vs) w.i16(r);
    }
    objs[idx] = w.out();
    return idx;
  };
  add(top);
  const w = new W().ascii('bplist00');
  const offsets: number[] = [];
  for (const o of objs) { offsets.push(w.len); w.bytes(o); }
  const tableAt = w.len;
  for (const o of offsets) w.i32(o);
  w.bytes([0, 0, 0, 0, 0, 0]).u8(4).u8(2);
  w.i32(0).i32(objs.length).i32(0).i32(0).i32(0).i32(tableAt);
  return w.out();
}
function archive(fields: [string, P][], name?: string): Uint8Array {
  // $objects: [ '$null', root dict, name string?, class ]
  const objects: P[] = ['$null'];
  const root: [string, P][] = [];
  objects.push({ dict: root });
  if (name !== undefined) { objects.push(name); root.push(['name', { uid: 2 }]); }
  for (const f of fields) root.push(f);
  objects.push({ dict: [['$classname', 'SilicaBrush'], ['$classes', ['SilicaBrush', 'NSObject']]] });
  root.push(['$class', { uid: objects.length - 1 }]);
  return bplist({ dict: [['$archiver', 'NSKeyedArchiver'], ['$version', 100000], ['$top', { dict: [['root', { uid: 1 }]] }], ['$objects', objects]] });
}

const archiveA = archive([
  ['paintSize', { real: 0.4 }], ['plotSpacing', { real: 0.15 }], ['paintOpacity', { real: 0.8 }],
  ['dynamicsPressureSize', { real: 0.6 }], ['dynamicsPressureOpacity', { real: 0.5 }],
  ['dynamicsJitterSize', { real: 0.25 }], ['dynamicsJitterOpacity', { real: 0.1 }],
  ['plotJitter', { real: 1.4 }], ['shapeScatter', { real: 0.3 }], ['shapeRotation', { real: 1 }],
  ['textureScale', { real: 0.5 }], ['textureDepth', { real: 0.7 }], ['shapeCount', 3], ['flipY', true], ['bundledShapePath', null],
], 'Soft Chalk ✎');

async function zipBytes(files: { name: string; data: Uint8Array }[]) {
  return new Uint8Array(await writeZip(files).arrayBuffer());
}
const oneBrush = await zipBytes([
  { name: 'Brush.archive', data: archiveA },
  { name: 'Shape.png', data: shapePng },
  { name: 'Grain.png', data: grainPng },
]);
const brushset = await zipBytes([
  { name: 'brushset.archive', data: bplist({ dict: [['brushes', ['x']]] }) },
  { name: '6F9619FF-8B86-D011-B42D-00CF4FC964FF/Brush.archive', data: archive([['paintSize', { real: 0.1 }]], 'Ink Pen') },
  { name: '6F9619FF-8B86-D011-B42D-00CF4FC964FF/Shape.png', data: inkPng },
  { name: '6F9619FF-8B86-D011-B42D-00CF4FC964FF/QuickLook/Thumbnail.png', data: grainPng },
  { name: 'Inky.brush/Brush.archive', data: archive([['plotSpacing', { real: 0.05 }]]) },
  { name: 'Inky.brush/Shape.png', data: shapePng },
]);

// ── .happybrush, as happypaint's encodeHappyBrush writes it ─────────────────
const chalk = BRUSH_PRESETS.find((p) => p.id === 'chalk')!;
const { id: _id, name: _name, category: _cat, ...chalkSettings } = chalk as unknown as Record<string, unknown>;
const tip128: number[] = [], grain256: number[] = [];
for (let y = 0; y < 128; y++) for (let x = 0; x < 128; x++) tip128.push(Math.hypot(x - 63.5, y - 63.5) < 40 ? 255 : (x * 2) % 256 > 200 ? 90 : 0);
for (let i = 0; i < 256 * 256; i++) grain256.push((i * 7 + (i >> 8) * 13) % 256);
const manifest = {
  format: 'happybrush', version: 1, name: 'Chalk (mine)',
  settings: { ...chalkSettings, tipId: 'custom:embedded', grain: { id: 'customgrain:embedded', scale: 120, strength: 0.4, move: 1 } },
};
const happybrush = await zipBytes([
  { name: 'manifest.json', data: new TextEncoder().encode(JSON.stringify(manifest, null, 2)) },
  { name: 'tip.png', data: png(128, 128, 1, tip128) },
  { name: 'grain.png', data: png(256, 256, 1, grain256) },
]);

// ── Goldens from happypaint's own parsers ───────────────────────────────────
const abrJson = async (bytes: Uint8Array) => {
  const f = await parseAbr(bytes);
  return {
    tips: f.tips.map((t) => ({ name: t.name ?? null, width: t.width, height: t.height, uuid: t.uuid ?? null, data: Array.from(t.data), mapped: t.desc ? mapAbrDescriptor(t.desc) : null })),
    computed: f.computed.map((c) => ({ ...c, name: c.name ?? null })),
    patterns: f.patterns.map((p) => ({ ...p, data: Array.from(p.data) })),
    warnings: f.warnings,
  };
};
const plain = (v: unknown): unknown => {
  if (v instanceof Uint8Array) return { bytes: v.length };
  if (Array.isArray(v)) return v.map(plain);
  if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, plain(x)]));
  return v;
};
const procJson = async (bytes: Uint8Array, name: string) =>
  (await parseProcreateFile(bytes, name)).map((b) => ({
    name: b.name, shape: b.shapePng?.length ?? null, grain: b.grainPng?.length ?? null, params: b.params ? plain(b.params) : null,
  }));

const golden = {
  v1: await abrJson(v1),
  v2: await abrJson(v2),
  v6: await abrJson(v6),
  brush: await procJson(oneBrush, 'one.brush'),
  brushset: await procJson(brushset, 'set.brushset'),
  happybrushSettings: manifest.settings,
};

const files: [string, Uint8Array | string][] = [
  ['v1.abr', v1], ['v2.abr', v2], ['v6.abr', v6], ['one.brush', oneBrush], ['set.brushset', brushset],
  ['chalk.happybrush', happybrush], ['golden.json', JSON.stringify(golden, null, 1)],
];
for (const [n, d] of files) await Bun.write(`${out}/${n}`, d);
console.log('wrote', files.map((f) => f[0]).join(', '));
