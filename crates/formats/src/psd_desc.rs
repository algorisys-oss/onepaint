//! Photoshop action descriptors (the "Descriptor structure" of the PSD
//! spec): read into a tree and written back, for blocks that are whole
//! descriptors (layer effects `lfx2`, spec 56). Only the item types those
//! blocks use are known; any other stops the read.

/// One item's value.
#[derive(Clone, Debug, PartialEq)]
pub enum DVal {
    /// `Objc` (or `GlbO`): a nested descriptor.
    Obj(Desc),
    /// `VlLs`: a list of values.
    List(Vec<DVal>),
    /// `doub`.
    Doub(f64),
    /// `UntF`: a unit (`#Prc`, `#Ang`, `#Pxl`, …) and a value.
    Unit([u8; 4], f64),
    /// `TEXT`.
    Text(String),
    /// `enum`: its type and value ids.
    Enum(String, String),
    /// `long`.
    Long(i32),
    /// `comp`.
    Comp(i64),
    /// `bool`.
    Bool(bool),
    /// `type` (or `GlbC`): a class's name and id.
    Class(String, String),
    /// `tdta`: raw bytes.
    Data(Vec<u8>),
    /// `UnFl`: a unit and values.
    Units([u8; 4], Vec<f64>),
}

/// A descriptor: its name, class id and items in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Desc {
    pub name: String,
    pub class: String,
    pub items: Vec<(String, DVal)>,
}

impl Desc {
    pub fn new(class: &str) -> Self {
        Desc { name: String::new(), class: class.into(), items: Vec::new() }
    }

    /// The first item keyed `key`.
    pub fn get(&self, key: &str) -> Option<&DVal> {
        self.items.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn push(&mut self, key: &str, v: DVal) -> &mut Self {
        self.items.push((key.into(), v));
        self
    }

    pub fn f64(&self, key: &str) -> Option<f64> {
        match self.get(key)? {
            DVal::Doub(v) | DVal::Unit(_, v) => Some(*v),
            DVal::Long(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            DVal::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// An `enum` item's value id.
    pub fn enum_val(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            DVal::Enum(_, v) => Some(v),
            _ => None,
        }
    }

    pub fn obj(&self, key: &str) -> Option<&Desc> {
        match self.get(key)? {
            DVal::Obj(d) => Some(d),
            _ => None,
        }
    }

    pub fn list(&self, key: &str) -> Option<&[DVal]> {
        match self.get(key)? {
            DVal::List(l) => Some(l),
            _ => None,
        }
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            DVal::Text(s) => Some(s),
            _ => None,
        }
    }
}

struct Reader<'a> {
    d: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.d.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f64(&mut self) -> Option<f64> {
        self.take(8).map(|b| f64::from_be_bytes(b.try_into().unwrap_or([0; 8])))
    }

    fn four(&mut self) -> Option<[u8; 4]> {
        self.take(4).map(|b| [b[0], b[1], b[2], b[3]])
    }

    /// A unicode string: its length (with the trailing null) and UTF-16 units.
    fn unicode(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        let b = self.take(n.checked_mul(2)?)?;
        let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
        Some(String::from_utf16_lossy(&units[..end]))
    }

    /// A key or class id: a length, or 0 for a four-character code.
    fn id(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        let b = self.take(if n == 0 { 4 } else { n })?;
        Some(String::from_utf8_lossy(b).into_owned())
    }

    fn desc(&mut self) -> Option<Desc> {
        let name = self.unicode()?;
        let class = self.id()?;
        let n = self.u32()? as usize;
        let mut items = Vec::with_capacity(n.min(256));
        for _ in 0..n {
            let key = self.id()?;
            let v = self.value()?;
            items.push((key, v));
        }
        Some(Desc { name, class, items })
    }

    fn value(&mut self) -> Option<DVal> {
        let ty = self.four()?;
        Some(match &ty {
            b"Objc" | b"GlbO" => DVal::Obj(self.desc()?),
            b"VlLs" => {
                let n = self.u32()? as usize;
                let mut v = Vec::with_capacity(n.min(256));
                for _ in 0..n {
                    v.push(self.value()?);
                }
                DVal::List(v)
            }
            b"doub" => DVal::Doub(self.f64()?),
            b"UntF" => DVal::Unit(self.four()?, self.f64()?),
            b"TEXT" => DVal::Text(self.unicode()?),
            b"enum" => DVal::Enum(self.id()?, self.id()?),
            b"long" => DVal::Long(self.u32()? as i32),
            b"comp" => DVal::Comp(i64::from_be_bytes(self.take(8)?.try_into().ok()?)),
            b"bool" => DVal::Bool(self.take(1)?[0] != 0),
            b"type" | b"GlbC" => DVal::Class(self.unicode()?, self.id()?),
            b"tdta" => {
                let n = self.u32()? as usize;
                DVal::Data(self.take(n)?.to_vec())
            }
            b"UnFl" => {
                let unit = self.four()?;
                let n = self.u32()? as usize;
                let mut v = Vec::with_capacity(n.min(256));
                for _ in 0..n {
                    v.push(self.f64()?);
                }
                DVal::Units(unit, v)
            }
            _ => return None,
        })
    }
}

/// Read a descriptor (name, class id, items) from `d` at `at`; `None` on a
/// short block or an item type this reader doesn't know.
pub fn read(d: &[u8], at: usize) -> Option<(Desc, usize)> {
    let mut r = Reader { d, at };
    let desc = r.desc()?;
    Some((desc, r.at))
}

fn put_unicode(o: &mut Vec<u8>, s: &str) {
    let u: Vec<u16> = s.encode_utf16().collect();
    o.extend((u.len() as u32 + 1).to_be_bytes());
    for c in u {
        o.extend(c.to_be_bytes());
    }
    o.extend([0, 0]);
}

/// A key or class id: four characters as a code, longer ones with their length.
fn put_id(o: &mut Vec<u8>, k: &str) {
    if k.len() == 4 {
        o.extend(0u32.to_be_bytes());
    } else {
        o.extend((k.len() as u32).to_be_bytes());
    }
    o.extend(k.as_bytes());
}

fn put_value(o: &mut Vec<u8>, v: &DVal) {
    match v {
        DVal::Obj(d) => {
            o.extend(*b"Objc");
            write_into(o, d);
        }
        DVal::List(l) => {
            o.extend(*b"VlLs");
            o.extend((l.len() as u32).to_be_bytes());
            for v in l {
                put_value(o, v);
            }
        }
        DVal::Doub(x) => {
            o.extend(*b"doub");
            o.extend(x.to_be_bytes());
        }
        DVal::Unit(u, x) => {
            o.extend(*b"UntF");
            o.extend(u);
            o.extend(x.to_be_bytes());
        }
        DVal::Text(s) => {
            o.extend(*b"TEXT");
            put_unicode(o, s);
        }
        DVal::Enum(t, v) => {
            o.extend(*b"enum");
            put_id(o, t);
            put_id(o, v);
        }
        DVal::Long(x) => {
            o.extend(*b"long");
            o.extend(x.to_be_bytes());
        }
        DVal::Comp(x) => {
            o.extend(*b"comp");
            o.extend(x.to_be_bytes());
        }
        DVal::Bool(b) => {
            o.extend(*b"bool");
            o.push(u8::from(*b));
        }
        DVal::Class(n, c) => {
            o.extend(*b"type");
            put_unicode(o, n);
            put_id(o, c);
        }
        DVal::Data(b) => {
            o.extend(*b"tdta");
            o.extend((b.len() as u32).to_be_bytes());
            o.extend(b);
        }
        DVal::Units(u, v) => {
            o.extend(*b"UnFl");
            o.extend(u);
            o.extend((v.len() as u32).to_be_bytes());
            for x in v {
                o.extend(x.to_be_bytes());
            }
        }
    }
}

/// Append `d` (name, class id, items).
pub fn write_into(o: &mut Vec<u8>, d: &Desc) {
    put_unicode(o, &d.name);
    put_id(o, &d.class);
    o.extend((d.items.len() as u32).to_be_bytes());
    for (k, v) in &d.items {
        put_id(o, k);
        put_value(o, v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_round_trip() {
        let mut clr = Desc::new("RGBC");
        clr.push("Rd  ", DVal::Doub(255.0)).push("Grn ", DVal::Doub(10.5)).push("Bl  ", DVal::Doub(0.0));
        let mut d = Desc::new("Lefx");
        d.push("Scl ", DVal::Unit(*b"#Prc", 100.0))
            .push("masterFXSwitch", DVal::Bool(true))
            .push("Md  ", DVal::Enum("BlnM".into(), "Mltp".into()))
            .push("Clr ", DVal::Obj(clr))
            .push("list", DVal::List(vec![DVal::Long(-3), DVal::Text("héllo".into())]))
            .push("raw", DVal::Data(vec![1, 2, 3]));
        let mut o = Vec::new();
        write_into(&mut o, &d);
        let (back, end) = read(&o, 0).unwrap();
        assert_eq!(back, d);
        assert_eq!(end, o.len());
        assert_eq!(back.obj("Clr ").and_then(|c| c.f64("Grn ")), Some(10.5));
        assert_eq!(back.enum_val("Md  "), Some("Mltp"));
        // An item type this reader doesn't know stops it.
        let mut bad = o.clone();
        let at = bad.windows(4).position(|w| w == b"tdta").unwrap();
        bad[at..at + 4].copy_from_slice(b"ObAr");
        assert!(read(&bad, 0).is_none());
    }
}
