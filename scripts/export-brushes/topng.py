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
rs += ['];', '', 'pub static GRAINS: &[(&str, &str, u32, &[u8])] = &[']
for gid, m in sorted(meta['grains'].items()):
    n = m['size']; data = open(os.path.join(src, f'grain-{gid}.bin'), 'rb').read()
    assert len(data) == n * n, gid
    Image.frombytes('L', (n, n), data).save(os.path.join(dst, 'grains', f'{gid}.png'), optimize=True)
    rs.append(f'    ("{gid}", "{m["name"]}", {n}, include_bytes!("../assets/grains/{gid}.png")),')
rs.append('];')
open(os.path.join(dst, 'assets.rs'), 'w').write('\n'.join(rs) + '\n')
