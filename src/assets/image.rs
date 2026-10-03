//! The game's `.BMP` files: either a genuine Windows bitmap or (almost always) a compact
//! header + palette + LZ-compressed rows.

/// Straight RGBA, top row first.
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// LZSS variant used for image rows. Control bytes are consumed MSB first: a clear bit
/// copies a literal, a set bit copies a back-reference. The stream ends when the input
/// runs out or at a back-reference with a distance of zero; `None` means corrupt data.
fn decompress(src: &[u8], out: &mut Vec<u8>) -> Option<()> {
    decompress_inner(src, out).or(Some(())).filter(|_| !src.is_empty())
}

/// Returns `None` when the input is exhausted.
fn decompress_inner(src: &[u8], out: &mut Vec<u8>) -> Option<()> {
    let start = out.len();
    let mut at = 0;
    let byte = |at: &mut usize| {
        *at += 1;
        src.get(*at - 1).copied()
    };
    out.push(byte(&mut at)?);
    loop {
        let control = byte(&mut at)?;
        for bit in 0..8 {
            if control & (0x80 >> bit) == 0 {
                out.push(byte(&mut at)?);
                continue;
            }
            let token = byte(&mut at)?;
            let back = ((token as usize & 0xf0) << 4) | byte(&mut at)? as usize;
            let count = match token & 0xf {
                0 => byte(&mut at)? as usize + 18,
                n => 18 - n as usize,
            };
            if back == 0 {
                return Some(());
            }
            let Some(from) = out.len().checked_sub(back).filter(|&f| f >= start) else {
                return Some(());
            };
            for i in 0..count {
                out.push(out[from + i]);
            }
        }
    }
}

/// Decodes an image; pixels matching `color_key` (RGB) become transparent.
pub fn decode_bmp(d: &[u8], color_key: Option<[u8; 3]>) -> Option<Pixels> {
    if d.get(..2)? == b"BM" {
        return None; // Plain Windows bitmaps don't appear in the retail archive.
    }
    let bpp = (d[0] & 0x3c) as usize;
    let width = u16::from_le_bytes([d[2], d[3]]) as usize;
    let height = u16::from_le_bytes([d[4], d[5]]) as usize;
    let palette_len = if bpp > 8 || d[0] & 0x80 != 0 { 0 } else { d[1] as usize + 1 };
    let palette = d.get(6..6 + 3 * palette_len)?;

    // Chunks of (raw size, stored size, bytes); stored < raw means compressed.
    let mut raw = Vec::new();
    let mut at = 6 + 3 * palette_len;
    let row_bytes = if bpp == 4 { (width * 4 + 4) >> 3 } else { (bpp * width) >> 3 };
    while raw.len() < row_bytes * height {
        let head = d.get(at..at + 4)?;
        let size = u16::from_le_bytes([head[0], head[1]]) as usize;
        let stored = u16::from_le_bytes([head[2], head[3]]) as usize;
        at += 4;
        if stored < size {
            let before = raw.len();
            decompress(d.get(at..at + stored)?, &mut raw)?;
            raw.resize(before + size, 0);
        } else {
            raw.extend_from_slice(d.get(at..at + size)?);
        }
        at += stored;
    }

    let mut rgba = Vec::with_capacity(width * height * 4);
    let mut push = |b: u8, g: u8, r: u8, a: u8| {
        let keyed = color_key == Some([r, g, b]);
        rgba.extend_from_slice(&[r, g, b, if keyed { 0 } else { a }]);
    };
    for row in raw.chunks(row_bytes).take(height) {
        for x in 0..width {
            match bpp {
                4 | 8 => {
                    let index = if bpp == 8 {
                        row[x]
                    } else if x % 2 == 0 {
                        row[x / 2] >> 4
                    } else {
                        row[x / 2] & 0xf
                    } as usize;
                    let c = palette.get(index * 3..index * 3 + 3).unwrap_or(&[0, 0, 0]);
                    push(c[0], c[1], c[2], 255);
                }
                24 => push(row[x * 3], row[x * 3 + 1], row[x * 3 + 2], 255),
                32 => push(row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3]),
                _ => return None,
            }
        }
    }
    Some(Pixels { width: width as u32, height: height as u32, rgba })
}

/// Decodes an uncompressed true-colour Targa, the only kind the archive holds. These
/// carry their own transparency.
pub fn decode_tga(d: &[u8]) -> Option<Pixels> {
    let (id, kind, bpp) = (*d.first()? as usize, *d.get(2)?, *d.get(16)? as usize);
    if kind != 2 || (bpp != 24 && bpp != 32) {
        return None;
    }
    let width = u16::from_le_bytes([d[12], d[13]]) as usize;
    let height = u16::from_le_bytes([d[14], d[15]]) as usize;
    let top_down = d[17] & 0x20 != 0;
    let step = bpp / 8;
    let data = d.get(18 + id..18 + id + width * height * step)?;
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let row = if top_down { y } else { height - 1 - y };
        for p in data[row * width * step..(row + 1) * width * step].chunks(step) {
            rgba.extend_from_slice(&[p[2], p[1], p[0], if step == 4 { p[3] } else { 255 }]);
        }
    }
    Some(Pixels { width: width as u32, height: height as u32, rgba })
}
