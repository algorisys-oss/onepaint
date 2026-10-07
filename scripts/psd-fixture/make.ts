// Writes a PSD with ag-psd (happypaint's library) for onepaint's import test:
//   cd ../../../happypaint/frontend && bun run ../../onepaint/scripts/psd-fixture/make.ts <out.psd>
import { writePsd } from 'ag-psd';

const W = 64, H = 48;
function rgba(w: number, h: number, f: (x: number, y: number) => [number, number, number, number]) {
  const data = new Uint8ClampedArray(w * h * 4);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) data.set(f(x, y), (y * w + x) * 4);
  return { width: w, height: h, data } as any;
}
const psd: any = {
  width: W, height: H,
  children: [
    { name: 'Background', imageData: rgba(W, H, () => [255, 255, 255, 255]), left: 0, top: 0 },
    { name: 'Red box', imageData: rgba(20, 10, () => [255, 0, 0, 255]), left: 5, top: 6, opacity: 0.5, blendMode: 'multiply' },
    {
      name: 'Folder', opened: true, blendMode: 'screen',
      children: [
        { name: 'Blue ✓', imageData: rgba(10, 10, () => [0, 0, 255, 128]), left: 30, top: 20, hidden: true },
        { name: 'Clip', imageData: rgba(10, 10, () => [0, 255, 0, 255]), left: 30, top: 20, clipping: true },
      ],
    },
    {
      name: 'Masked', imageData: rgba(W, H, () => [10, 20, 30, 255]), left: 0, top: 0,
      mask: { imageData: rgba(W, H, (x) => (x < 32 ? [255, 255, 255, 255] : [0, 0, 0, 255])), left: 0, top: 0, defaultColor: 255, disabled: true },
    },
  ],
};
const out = process.argv[2] ?? 'fixture.psd';
await Bun.write(out, new Uint8Array(writePsd(psd, { generateThumbnail: false })));
console.log('wrote', out);
