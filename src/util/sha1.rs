//! SHA-1（git のオブジェクト ID 用）。

pub struct Sha1 {
    h: [u32; 5],
    buf: [u8; 64],
    nbuf: usize,
    len: u64,
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha1 {
    pub fn new() -> Self {
        Sha1 { h: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0], buf: [0; 64], nbuf: 0, len: 0 }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.len += data.len() as u64;
        if self.nbuf > 0 {
            let n = (64 - self.nbuf).min(data.len());
            self.buf[self.nbuf..self.nbuf + n].copy_from_slice(&data[..n]);
            self.nbuf += n;
            data = &data[n..];
            if self.nbuf < 64 {
                return;
            }
            let b = self.buf;
            compress(&mut self.h, &b);
            self.nbuf = 0;
        }
        let mut chunks = data.chunks_exact(64);
        for c in &mut chunks {
            compress(&mut self.h, c.try_into().unwrap());
        }
        let rest = chunks.remainder();
        self.buf[..rest.len()].copy_from_slice(rest);
        self.nbuf = rest.len();
    }

    pub fn finish(mut self) -> [u8; 20] {
        let bits = self.len.wrapping_mul(8);
        let pad = if self.nbuf < 56 { 56 - self.nbuf } else { 120 - self.nbuf };
        let mut tail = [0u8; 72];
        tail[0] = 0x80;
        self.update(&tail[..pad]);
        self.update(&bits.to_be_bytes());
        let mut out = [0u8; 20];
        for (i, w) in self.h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
        }
        out
    }
}

fn compress(h: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 80];
    for i in 0..16 {
        w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *h;
    for (i, wi) in w.iter().enumerate() {
        let (f, k) = match i / 20 {
            0 => ((b & c) | (!b & d), 0x5A827999),
            1 => (b ^ c ^ d, 0x6ED9EBA1),
            2 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
            _ => (b ^ c ^ d, 0xCA62C1D6),
        };
        let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = t;
    }
    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
}

pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut s = Sha1::new();
    s.update(data);
    s.finish()
}

pub fn hex(oid: &[u8; 20]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(40);
    for b in oid {
        s.push(D[(b >> 4) as usize] as char);
        s.push(D[(b & 15) as usize] as char);
    }
    s
}

pub fn from_hex(s: &str) -> Option<[u8; 20]> {
    let b = s.as_bytes();
    if b.len() != 40 {
        return None;
    }
    let v = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = [0u8; 20];
    for i in 0..20 {
        out[i] = v(b[i * 2])? << 4 | v(b[i * 2 + 1])?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        let mut s = Sha1::new();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            s.update(&chunk[..7]);
            s.update(&chunk[7..]);
        }
        assert_eq!(hex(&s.finish()), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
        assert_eq!(hex(&sha1(b"blob 0\0")), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    }

    #[test]
    fn hex_roundtrip() {
        let o = sha1(b"abc");
        assert_eq!(from_hex(&hex(&o)), Some(o));
        assert_eq!(from_hex(&hex(&o).to_uppercase()), Some(o));
        assert_eq!(from_hex("zz"), None);
        assert_eq!(from_hex(&"g".repeat(40)), None);
    }
}
