//! zlib / gzip: inflate（stored・固定・動的ハフマン）と deflate（stored ブロック）。

const FAST: u32 = 10;

struct Huff {
    count: [u16; 16],
    sym: Vec<u16>,
    // (sym << 4) | len を FAST ビットで引く。0 は表外（長い符号）
    fast: Vec<u16>,
}

impl Huff {
    fn new(lens: &[u8]) -> Result<Huff, String> {
        let mut count = [0u16; 16];
        for &l in lens {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut left: i32 = 1;
        for len in 1..16 {
            left = (left << 1) - count[len] as i32;
            if left < 0 {
                return Err("over-subscribed huffman code".into());
            }
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut sym = vec![0u16; lens.len()];
        for (s, &l) in lens.iter().enumerate() {
            if l != 0 {
                sym[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        let mut fast = vec![0u16; 1 << FAST];
        let (mut code, mut idx) = (0u32, 0usize);
        for len in 1..=FAST {
            for _ in 0..count[len as usize] {
                let rev = code.reverse_bits() >> (32 - len);
                let e = (sym[idx] << 4) | len as u16;
                let mut j = rev as usize;
                while j < fast.len() {
                    fast[j] = e;
                    j += 1 << len;
                }
                code += 1;
                idx += 1;
            }
            code <<= 1;
        }
        Ok(Huff { count, sym, fast })
    }
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    buf: u64,
    n: u32,
}

impl Bits<'_> {
    #[inline]
    fn refill(&mut self) {
        if self.pos + 8 <= self.d.len() {
            let w = u64::from_le_bytes(self.d[self.pos..self.pos + 8].try_into().unwrap());
            self.buf |= w << self.n;
            let k = (63 - self.n) / 8;
            self.pos += k as usize;
            self.n += k * 8;
        } else {
            while self.n <= 56 && self.pos < self.d.len() {
                self.buf |= (self.d[self.pos] as u64) << self.n;
                self.pos += 1;
                self.n += 8;
            }
        }
    }
    #[inline]
    fn need(&mut self, k: u32) -> Result<(), String> {
        if self.n < k {
            self.refill();
            if self.n < k {
                return Err("unexpected end of deflate stream".into());
            }
        }
        Ok(())
    }
    #[inline]
    fn get(&mut self, k: u32) -> Result<u32, String> {
        if k == 0 {
            return Ok(0);
        }
        self.need(k)?;
        let v = (self.buf & ((1u64 << k) - 1)) as u32;
        self.buf >>= k;
        self.n -= k;
        Ok(v)
    }
    #[inline]
    fn decode(&mut self, h: &Huff) -> Result<u16, String> {
        if self.n < 15 {
            self.refill();
        }
        let e = h.fast[(self.buf & ((1 << FAST) - 1)) as usize];
        let l = (e & 15) as u32;
        if e != 0 && l <= self.n {
            self.buf >>= l;
            self.n -= l;
            return Ok(e >> 4);
        }
        // 長い符号: 1ビットずつ正準復号
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= self.get(1)? as i32;
            let c = h.count[len] as i32;
            if code - c < first {
                return Ok(h.sym[(index + code - first) as usize]);
            }
            index += c;
            first = (first + c) << 1;
            code <<= 1;
        }
        Err("invalid huffman code".into())
    }
    // 端数ビットを捨てて、まだ使っていない入力の位置を返す
    fn align(&mut self) -> usize {
        let r = self.n % 8;
        self.buf >>= r;
        self.n -= r;
        self.pos - (self.n / 8) as usize
    }
}

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// 生の deflate を展開し、（出力, 消費バイト数）を返す。
fn inflate_raw(d: &[u8], out: &mut Vec<u8>) -> Result<usize, String> {
    let mut b = Bits { d, pos: 0, buf: 0, n: 0 };
    let mut fixed: Option<(Huff, Huff)> = None;
    loop {
        let last = b.get(1)?;
        let ty = b.get(2)?;
        match ty {
            0 => {
                let p = b.align();
                if p + 4 > d.len() {
                    return Err("truncated stored block".into());
                }
                let len = u16::from_le_bytes([d[p], d[p + 1]]) as usize;
                let nlen = u16::from_le_bytes([d[p + 2], d[p + 3]]) as usize;
                if len != !nlen & 0xffff || p + 4 + len > d.len() {
                    return Err("bad stored block".into());
                }
                out.extend_from_slice(&d[p + 4..p + 4 + len]);
                b = Bits { d, pos: p + 4 + len, buf: 0, n: 0 };
            }
            1 => {
                if fixed.is_none() {
                    let mut l = [8u8; 288];
                    l[144..256].fill(9);
                    l[256..280].fill(7);
                    fixed = Some((Huff::new(&l)?, Huff::new(&[5u8; 30])?));
                }
                let (lh, dh) = fixed.as_ref().unwrap();
                codes(&mut b, out, lh, dh)?;
            }
            2 => {
                let nlen = b.get(5)? as usize + 257;
                let ndist = b.get(5)? as usize + 1;
                let ncode = b.get(4)? as usize + 4;
                if nlen > 286 || ndist > 30 {
                    return Err("bad dynamic block counts".into());
                }
                const ORD: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                let mut cl = [0u8; 19];
                for &i in &ORD[..ncode] {
                    cl[i] = b.get(3)? as u8;
                }
                let ch = Huff::new(&cl)?;
                let mut lens = [0u8; 316];
                let mut i = 0;
                while i < nlen + ndist {
                    let s = b.decode(&ch)?;
                    let (v, rep) = match s {
                        0..=15 => (s as u8, 1),
                        16 => {
                            if i == 0 {
                                return Err("repeat with no previous length".into());
                            }
                            (lens[i - 1], 3 + b.get(2)? as usize)
                        }
                        17 => (0, 3 + b.get(3)? as usize),
                        _ => (0, 11 + b.get(7)? as usize),
                    };
                    if i + rep > nlen + ndist {
                        return Err("too many code lengths".into());
                    }
                    lens[i..i + rep].fill(v);
                    i += rep;
                }
                if lens[256] == 0 {
                    return Err("missing end-of-block code".into());
                }
                codes(&mut b, out, &Huff::new(&lens[..nlen])?, &Huff::new(&lens[nlen..nlen + ndist])?)?;
            }
            _ => return Err("invalid block type".into()),
        }
        if last == 1 {
            return Ok(b.align());
        }
    }
}

fn codes(b: &mut Bits, out: &mut Vec<u8>, lh: &Huff, dh: &Huff) -> Result<(), String> {
    loop {
        let s = b.decode(lh)? as usize;
        if s < 256 {
            out.push(s as u8);
            continue;
        }
        if s == 256 {
            return Ok(());
        }
        let s = s - 257;
        if s >= 29 {
            return Err("bad length symbol".into());
        }
        let len = LBASE[s] as usize + b.get(LEXT[s] as u32)? as usize;
        let ds = b.decode(dh)? as usize;
        if ds >= 30 {
            return Err("bad distance symbol".into());
        }
        let dist = DBASE[ds] as usize + b.get(DEXT[ds] as u32)? as usize;
        if dist > out.len() {
            return Err("distance too far back".into());
        }
        let start = out.len() - dist;
        if dist >= len {
            out.extend_from_within(start..start + len);
        } else {
            out.reserve(len);
            for k in 0..len {
                let c = out[start + k];
                out.push(c);
            }
        }
    }
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for c in data.chunks(5552) {
        for &x in c {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// zlib ストリームを展開し、（出力, adler32 を含む消費バイト数）を返す。
pub fn inflate(input: &[u8]) -> Result<(Vec<u8>, usize), String> {
    if input.len() < 2 || input[0] & 0x0f != 8 || ((input[0] as u16) << 8 | input[1] as u16) % 31 != 0 {
        return Err("bad zlib header".into());
    }
    if input[1] & 0x20 != 0 {
        return Err("zlib preset dictionary not supported".into());
    }
    let mut out = Vec::with_capacity(input.len() * 3);
    let end = 2 + inflate_raw(&input[2..], &mut out)?;
    let t = input.get(end..end + 4).ok_or("missing adler32")?;
    if u32::from_be_bytes(t.try_into().unwrap()) != adler32(&out) {
        return Err("adler32 mismatch".into());
    }
    Ok((out, end + 4))
}

/// gzip（単一メンバ）を展開する。
// shortcut: CRC32 は検証せず ISIZE だけ見る。HTTP ボディ用で TCP が誤りを防ぐため。
pub fn gunzip(input: &[u8]) -> Result<Vec<u8>, String> {
    if input.len() < 18 || input[0] != 0x1f || input[1] != 0x8b || input[2] != 8 {
        return Err("bad gzip header".into());
    }
    let flg = input[3];
    let mut p = 10;
    if flg & 4 != 0 {
        let x = input.get(p..p + 2).ok_or("truncated gzip")?;
        p += 2 + u16::from_le_bytes([x[0], x[1]]) as usize;
    }
    for bit in [8u8, 16] {
        if flg & bit != 0 {
            p += input.get(p..).ok_or("truncated gzip")?.iter().position(|&c| c == 0).ok_or("truncated gzip")? + 1;
        }
    }
    if flg & 2 != 0 {
        p += 2;
    }
    let mut out = Vec::new();
    let end = p + inflate_raw(input.get(p..).ok_or("truncated gzip")?, &mut out)?;
    let t = input.get(end + 4..end + 8).ok_or("missing gzip trailer")?;
    if u32::from_le_bytes(t.try_into().unwrap()) != out.len() as u32 {
        return Err("gzip size mismatch".into());
    }
    Ok(out)
}

/// 有効な zlib ストリームを出す（stored ブロックのみ。速度優先）。
pub fn deflate(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() + input.len() / 65535 * 5 + 11);
    out.extend_from_slice(&[0x78, 0x01]);
    let mut chunks = input.chunks(65535).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(c) = chunks.next() {
        out.push(chunks.peek().is_none() as u8);
        let n = c.len() as u16;
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(!n).to_le_bytes());
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&adler32(input).to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize) -> Vec<u8> {
        let mut x = 0x12345678u32;
        (0..n).map(|_| { x ^= x << 13; x ^= x >> 17; x ^= x << 5; x as u8 }).collect()
    }

    #[test]
    fn roundtrip() {
        for d in [Vec::new(), b"hello".to_vec(), noise(1 << 20)] {
            let z = deflate(&d);
            let (o, n) = inflate(&z).unwrap();
            assert_eq!(o, d);
            assert_eq!(n, z.len());
        }
    }

    #[test]
    fn consumed_with_trailing_garbage() {
        let mut z = deflate(b"abc");
        let len = z.len();
        z.extend_from_slice(b"GARBAGE-after-stream-0123456789");
        assert_eq!(inflate(&z).unwrap(), (b"abc".to_vec(), len));
    }

    #[test]
    fn fixed_huffman() {
        let z = [120, 218, 203, 72, 205, 201, 201, 87, 200, 64, 39, 1, 104, 3, 8, 177];
        let mut zz = z.to_vec();
        zz.extend_from_slice(&[0xff; 9]);
        assert_eq!(inflate(&zz).unwrap(), (b"hello hello hello hello".to_vec(), z.len()));
    }

    #[test]
    fn dynamic_huffman() {
        let z: [u8; 73] = [120, 218, 237, 204, 193, 17, 192, 32, 8, 0, 176, 89, 105, 69, 225, 20, 196, 67, 97, 253, 190, 187, 131, 25, 32, 64, 213, 195, 30, 80, 215, 132, 185, 87, 193, 83, 64, 38, 153, 190, 13, 152, 227, 13, 162, 68, 24, 130, 131, 119, 90, 70, 87, 94, 214, 122, 53, 107, 194, 126, 196, 23, 54, 184, 201, 77, 254, 201, 7, 6, 96, 253, 10];
        let want: Vec<u8> = (0..600u32).map(|i| ((i * i * 7 + i / 3) % 23 + 97) as u8).collect();
        let mut zz = z.to_vec();
        zz.push(0x78);
        assert_eq!(inflate(&zz).unwrap(), (want, z.len()));
        assert!(inflate(&z[..60]).is_err());
    }

    #[test]
    fn gzip() {
        let g = [31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 203, 72, 205, 201, 201, 87, 72, 175, 202, 44, 80, 40, 207, 47, 202, 73, 1, 0, 107, 125, 232, 183, 16, 0, 0, 0];
        assert_eq!(gunzip(&g).unwrap(), b"hello gzip world");
    }
}
