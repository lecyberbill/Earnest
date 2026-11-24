use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::{Read, Write};

const PNG_SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_buf = Vec::with_capacity(4 + data.len());
    crc_buf.extend_from_slice(kind);
    crc_buf.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_buf).to_be_bytes());
}

/// Encodes raw RGB pixels (`side * side * 3` bytes) into a valid PNG byte stream.
pub fn encode_rgb_png(side: usize, rgb: &[u8]) -> Result<Vec<u8>, String> {
    if side == 0 || rgb.len() != side * side * 3 {
        return Err("Invalid RGB pixel buffer dimensions".to_string());
    }
    let mut raw_scanlines = Vec::with_capacity(side * (1 + side * 3));
    for y in 0..side {
        raw_scanlines.push(0u8); // filter type 0 (None)
        let row = &rgb[y * side * 3..(y + 1) * side * 3];
        raw_scanlines.extend_from_slice(row);
    }

    let mut zlib = ZlibEncoder::new(Vec::new(), Compression::default());
    zlib.write_all(&raw_scanlines).map_err(|e| e.to_string())?;
    let idat = zlib.finish().map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    out.extend_from_slice(PNG_SIG);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(side as u32).to_be_bytes());
    ihdr.extend_from_slice(&(side as u32).to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(2); // color type: RGB
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace
    write_chunk(&mut out, b"IHDR", &ihdr);
    write_chunk(&mut out, b"IDAT", &idat);
    write_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// Decodes a PNG written by `encode_rgb_png` back into `(side, rgb_bytes)`.
pub fn decode_rgb_png(bytes: &[u8]) -> Result<(usize, Vec<u8>), String> {
    if bytes.len() < 8 || &bytes[..8] != PNG_SIG {
        return Err("Invalid PNG signature".to_string());
    }
    let mut pos = 8usize;
    let mut side: Option<usize> = None;
    let mut idat_all = Vec::new();

    while pos + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = bytes[pos + 4..pos + 8].try_into().unwrap();
        pos += 8;
        if pos + len + 4 > bytes.len() {
            return Err("Truncated PNG chunk".to_string());
        }
        let data = &bytes[pos..pos + len];
        let expected_crc = u32::from_be_bytes(bytes[pos + len..pos + len + 4].try_into().unwrap());
        let mut crc_buf = Vec::with_capacity(4 + len);
        crc_buf.extend_from_slice(&kind);
        crc_buf.extend_from_slice(data);
        if crc32(&crc_buf) != expected_crc {
            return Err("Corrupted PNG chunk CRC".to_string());
        }
        pos += len + 4;

        match &kind {
            b"IHDR" => {
                if data.len() != 13 {
                    return Err("Invalid IHDR".to_string());
                }
                let w = u32::from_be_bytes(data[0..4].try_into().unwrap()) as usize;
                let h = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
                if w != h || w == 0 || data[8] != 8 || data[9] != 2 {
                    return Err("Unsupported PNG format (expected square 8-bit RGB)".to_string());
                }
                side = Some(w);
            }
            b"IDAT" => idat_all.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
    }

    let side = side.ok_or_else(|| "Missing IHDR chunk".to_string())?;
    let mut dec = ZlibDecoder::new(&idat_all[..]);
    let mut raw = Vec::new();
    dec.read_to_end(&mut raw).map_err(|e| format!("PNG zlib error: {}", e))?;

    let row_bytes = 1 + side * 3;
    if raw.len() != side * row_bytes {
        return Err("Unexpected PNG decompressed size".to_string());
    }
    let mut rgb = Vec::with_capacity(side * side * 3);
    for y in 0..side {
        let row = &raw[y * row_bytes..(y + 1) * row_bytes];
        if row[0] != 0 {
            return Err("Unsupported PNG scanline filter".to_string());
        }
        rgb.extend_from_slice(&row[1..]);
    }
    Ok((side, rgb))
}
