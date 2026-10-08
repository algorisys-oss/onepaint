import json, sys, os
from PIL import Image
src, dst = sys.argv[1], sys.argv[2]
meta = json.load(open(os.path.join(src, 'meta.json')))
rs = ['// Generated from happypaint by scripts/export-brushes (tips and grains are its own deterministic generators).', '',
      'pub static TIPS: &[(&str, u32, &[u8])] = &[']
for tid, m in sorted(meta['tips'].items()):
    n = m['size']; data = open(os.path.join(src, f'tip-{tid}.bin'), 'rb').read()
    assert len(data) == n * n, tid
    Image.frombytes('L', (n, n), data).save(os.path.join(dst, 'tips', f'{tid}.png'), optimize=True)
    rs.append(f'    ("{tid}", {n}, include_bytes!("../assets/tips/{tid}.png")),')
rs += ['];', '', '/// Colour tips: straight sRGB pixels and their coverage-weighted luma.',
       'pub static COLOR_TIPS: &[(&str, f32, &[u8])] = &[']
for tid, m in sorted(meta['tips'].items()):
    if not m.get('color'):
        continue
    n = m['size']; rgb = open(os.path.join(src, f'tip-{tid}.rgb.bin'), 'rb').read()
    assert len(rgb) == n * n * 3, tid
    Image.frombytes('RGB', (n, n), rgb).save(os.path.join(dst, 'tips', f'{tid}.rgb.png'), optimize=True)
    rs.append(f'    ("{tid}", {float(f"{m['lum']:.6f}")}, include_bytes!("../assets/tips/{tid}.rgb.png")),')
rs += ['];', '', 'pub static GRAINS: &[(&str, &str, u32, &[u8])] = &[']
for gid, m in sorted(meta['grains'].items()):
    n = m['size']; data = open(os.path.join(src, f'grain-{gid}.bin'), 'rb').read()
    assert len(data) == n * n, gid
    Image.frombytes('L', (n, n), data).save(os.path.join(dst, 'grains', f'{gid}.png'), optimize=True)
    rs.append(f'    ("{gid}", "{m["name"]}", {n}, include_bytes!("../assets/grains/{gid}.png")),')
rs.append('];')
open(os.path.join(dst, 'assets.rs'), 'w').write('\n'.join(rs) + '\n')
