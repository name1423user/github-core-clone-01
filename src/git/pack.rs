//! Packfile read (with OFS/REF_DELTA) and write (whole objects, no deltas).

use crate::store::{hash_object, ObjType, Oid};
use crate::util::sha1::{sha1, Sha1};
use crate::util::zlib;
use std::collections::HashMap;
use std::sync::Arc;

pub struct PackObject {
    pub oid: Oid,
    pub kind: ObjType,
    pub data: Arc<[u8]>,
}

fn obj_type(code: u8) -> Option<ObjType> {
    Some(match code {
        1 => ObjType::Commit,
        2 => ObjType::Tree,
        3 => ObjType::Blob,
        4 => ObjType::Tag,
        _ => return None,
    })
}

enum Body {
    Whole(ObjType, Vec<u8>),
    Ofs(usize, Vec<u8>),
    Ref(Oid, Vec<u8>),
}

/// Parse a pack. `external` supplies REF_DELTA bases the pack leaves out (thin packs).
/// Every object is inflated and hashed exactly once.
pub fn read_pack(buf: &[u8], external: impl Fn(&Oid) -> Option<(ObjType, Arc<[u8]>)>) -> Result<Vec<PackObject>, String> {
    if buf.len() < 32 || &buf[..4] != b"PACK" {
        return Err("bad pack header".into());
    }
    let version = u32::from_be_bytes(buf[4..8].try_into().unwrap());
    if version != 2 && version != 3 {
        return Err(format!("unsupported pack version {version}"));
    }
    let count = u32::from_be_bytes(buf[8..12].try_into().unwrap()) as usize;
    let end = buf.len() - 20;
    if sha1(&buf[..end]) != buf[end..] {
        return Err("pack checksum mismatch".into());
    }
    let mut entries: Vec<(usize, Body)> = Vec::with_capacity(count.min(1 << 16));
    let mut p = 12;
    for _ in 0..count {
        let start = p;
        let mut c = *buf.get(p).ok_or("truncated pack")?;
        p += 1;
        let code = (c >> 4) & 7;
        let mut size = (c & 15) as usize;
        let mut shift = 4;
        while c & 0x80 != 0 {
            c = *buf.get(p).ok_or("truncated pack")?;
            p += 1;
            if shift > 56 {
                return Err("bad size".into());
            }
            size |= ((c & 0x7f) as usize) << shift;
            shift += 7;
        }
        let kind = match code {
            6 => {
                let mut c = *buf.get(p).ok_or("truncated pack")?;
                p += 1;
                let mut off = (c & 0x7f) as usize;
                while c & 0x80 != 0 {
                    c = *buf.get(p).ok_or("truncated pack")?;
                    p += 1;
                    off = ((off + 1) << 7) | (c & 0x7f) as usize;
                }
                if off == 0 || off > start {
                    return Err("bad ofs-delta base".into());
                }
                Some(start - off)
            }
            7 => None,
            _ => {
                obj_type(code).ok_or("bad object type")?;
                None
            }
        };
        let base_oid: Option<Oid> = if code == 7 {
            let b = buf.get(p..p + 20).ok_or("truncated pack")?.try_into().unwrap();
            p += 20;
            Some(b)
        } else {
            None
        };
        if p > end {
            return Err("truncated pack".into());
        }
        let (data, used) = zlib::inflate(&buf[p..end])?;
        if data.len() != size {
            return Err("object size mismatch".into());
        }
        p += used;
        let body = match (code, kind, base_oid) {
            (6, Some(base), _) => Body::Ofs(base, data),
            (7, _, Some(b)) => Body::Ref(b, data),
            _ => Body::Whole(obj_type(code).unwrap(), data),
        };
        entries.push((start, body));
    }
    if p != end {
        return Err("trailing bytes in pack".into());
    }

    // Resolve: OFS bases come earlier; REF bases may be anywhere, so loop until no progress.
    let n = entries.len();
    let mut done: Vec<Option<PackObject>> = (0..n).map(|_| None).collect();
    let by_offset: HashMap<usize, usize> = entries.iter().enumerate().map(|(i, e)| (e.0, i)).collect();
    let mut by_oid: HashMap<Oid, usize> = HashMap::new();
    let mut left = n;
    loop {
        let before = left;
        for i in 0..n {
            if done[i].is_some() {
                continue;
            }
            let resolved = match &entries[i].1 {
                Body::Whole(k, d) => Some((*k, d.clone())),
                Body::Ofs(off, delta) => {
                    let bi = *by_offset.get(off).ok_or("ofs-delta base is not an object start")?;
                    match &done[bi] {
                        Some(b) => Some((b.kind, apply_delta(&b.data, delta)?)),
                        None => None,
                    }
                }
                Body::Ref(base, delta) => {
                    if let Some(&bi) = by_oid.get(base) {
                        let b = done[bi].as_ref().unwrap();
                        Some((b.kind, apply_delta(&b.data, delta)?))
                    } else if let Some((k, d)) = external(base) {
                        Some((k, apply_delta(&d, delta)?))
                    } else {
                        None
                    }
                }
            };
            if let Some((kind, data)) = resolved {
                let oid = hash_object(kind, &data);
                by_oid.insert(oid, i);
                done[i] = Some(PackObject { oid, kind, data: Arc::from(data) });
                left -= 1;
            }
        }
        if left == 0 {
            break;
        }
        if left == before {
            return Err("unresolvable delta base".into());
        }
    }
    // Free the raw bodies early; entries is dropped here.
    drop(entries);
    Ok(done.into_iter().map(Option::unwrap).collect())
}

fn delta_varint(d: &[u8], p: &mut usize) -> Result<usize, String> {
    let mut v = 0usize;
    let mut shift = 0;
    loop {
        let c = *d.get(*p).ok_or("truncated delta")?;
        *p += 1;
        if shift > 56 {
            return Err("bad delta size".into());
        }
        v |= ((c & 0x7f) as usize) << shift;
        shift += 7;
        if c & 0x80 == 0 {
            return Ok(v);
        }
    }
}

pub fn apply_delta(base: &[u8], d: &[u8]) -> Result<Vec<u8>, String> {
    let mut p = 0;
    if delta_varint(d, &mut p)? != base.len() {
        return Err("delta base size mismatch".into());
    }
    let size = delta_varint(d, &mut p)?;
    let mut out = Vec::with_capacity(size);
    while p < d.len() {
        let op = d[p];
        p += 1;
        if op & 0x80 != 0 {
            let mut off = 0usize;
            let mut len = 0usize;
            for i in 0..4 {
                if op & (1 << i) != 0 {
                    off |= (*d.get(p).ok_or("truncated delta")? as usize) << (8 * i);
                    p += 1;
                }
            }
            for i in 0..3 {
                if op & (0x10 << i) != 0 {
                    len |= (*d.get(p).ok_or("truncated delta")? as usize) << (8 * i);
                    p += 1;
                }
            }
            if len == 0 {
                len = 0x10000;
            }
            out.extend_from_slice(base.get(off..off + len).ok_or("delta copy out of range")?);
        } else if op != 0 {
            out.extend_from_slice(d.get(p..p + op as usize).ok_or("truncated delta")?);
            p += op as usize;
        } else {
            return Err("delta opcode 0".into());
        }
    }
    if out.len() != size {
        return Err("delta result size mismatch".into());
    }
    Ok(out)
}

/// Pack of whole objects: header + (type/size varint + zlib) per object + SHA-1 trailer.
pub fn write_pack(objs: &[(ObjType, Arc<[u8]>)]) -> Vec<u8> {
    let total: usize = objs.iter().map(|o| o.1.len() + 32).sum();
    let mut out = Vec::with_capacity(total + 32);
    out.extend_from_slice(b"PACK");
    out.extend_from_slice(&2u32.to_be_bytes());
    out.extend_from_slice(&(objs.len() as u32).to_be_bytes());
    for (kind, data) in objs {
        let mut size = data.len();
        let mut c = ((*kind as u8) << 4) | (size & 15) as u8;
        size >>= 4;
        while size != 0 {
            out.push(c | 0x80);
            c = (size & 0x7f) as u8;
            size >>= 7;
        }
        out.push(c);
        out.extend_from_slice(&zlib::deflate(data));
    }
    let mut h = Sha1::new();
    h.update(&out);
    out.extend_from_slice(&h.finish());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_deltas() {
        let base: Arc<[u8]> = Arc::from(&b"hello world, this is the base"[..]);
        let big: Arc<[u8]> = Arc::from(vec![7u8; 70000]);
        let pack = write_pack(&[(ObjType::Blob, base.clone()), (ObjType::Blob, big.clone())]);
        let objs = read_pack(&pack, |_| None).unwrap();
        assert_eq!(objs.len(), 2);
        assert_eq!(&*objs[1].data, &*big);
        assert_eq!(objs[0].oid, hash_object(ObjType::Blob, &base));

        // delta: copy "hello world" (0..11) then insert "!"
        let delta = [29u8, 12, 0x80 | 0x10, 11, 1, b'!'];
        assert_eq!(apply_delta(&base, &delta).unwrap(), b"hello world!");

        // Hand-built pack: whole blob, OFS_DELTA on it, REF_DELTA on an external base.
        let mut p = b"PACK\0\0\0\x02\0\0\0\x03".to_vec();
        let off0 = p.len();
        p.push(0x30 | (base.len() & 15) as u8 | 0x80);
        p.push((base.len() >> 4) as u8);
        p.extend_from_slice(&zlib::deflate(&base));
        let off1 = p.len();
        p.push(0x60 | delta.len() as u8);
        p.push((off1 - off0) as u8);
        p.extend_from_slice(&zlib::deflate(&delta));
        let ext = hash_object(ObjType::Blob, &base);
        p.push(0x70 | delta.len() as u8);
        p.extend_from_slice(&ext);
        p.extend_from_slice(&zlib::deflate(&delta));
        let t = sha1(&p);
        p.extend_from_slice(&t);
        let objs = read_pack(&p, |o| (*o == ext).then(|| (ObjType::Blob, base.clone()))).unwrap();
        assert_eq!(&*objs[1].data, b"hello world!");
        // REF_DELTA resolves from the in-pack base first (same oid).
        assert_eq!(&*objs[2].data, b"hello world!");
        assert!(read_pack(&p[..p.len() - 1], |_| None).is_err());
    }
}
