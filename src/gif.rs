use color_quant::NeuQuant;
use weezl::{encode::Encoder as LzwEncoder, BitOrder};

pub struct GifEncoder;

impl GifEncoder {
    pub fn write_screen_desc(buf: &mut Vec<u8>, width: u16, height: u16, flags: Option<u8>) {
        buf.extend_from_slice(b"GIF89a");
        buf.extend_from_slice(&width.to_le_bytes());
        buf.extend_from_slice(&height.to_le_bytes());
        buf.extend_from_slice(&[flags.unwrap_or(0), 0, 0]); // flags, bgcolor, aspect
    }

    pub fn global_palette_flags(palette: &[u8]) -> u8 {
        let mut flags = 0;
        flags |= 1 << 7; // global color table
        let num_colors = palette.len() / 3;

        flags |= flag_size(num_colors);
        flags |= flag_size(num_colors) << 4;

        flags
    }

    pub fn write_color_table(buf: &mut Vec<u8>, table: &[u8]) {
        let num_colors = table.len() / 3;

        buf.extend_from_slice(&table[..num_colors * 3]);

        let size = flag_size(num_colors);

        // Pad with black
        for _ in 0..(2usize << size).saturating_sub(num_colors) {
            buf.extend_from_slice(&[0, 0, 0]);
        }
    }

    pub fn write_repeat(buf: &mut Vec<u8>, repeat: u16) {
        Self::write_extension(buf, ExtensionData::Repetitions(repeat))
    }

    pub fn write_loop(buf: &mut Vec<u8>) {
        Self::write_extension(buf, ExtensionData::InfiniteRepetitions)
    }

    pub fn write_extension(buf: &mut Vec<u8>, extension: ExtensionData) {
        use ExtensionData::*;

        // Don't write empty extensions
        if let Repetitions(0) = extension {
            return;
        }

        buf.push(0x21);

        match extension {
            Control {
                flags,
                delay,
                transparency_idx: trns,
            } => {
                buf.push(0xF9);
                buf.push(4);
                buf.push(flags);
                buf.extend_from_slice(&delay.to_le_bytes());
                buf.push(trns);
            }
            InfiniteRepetitions => {
                buf.push(0xFF);
                buf.push(11);
                buf.extend_from_slice(b"NETSCAPE2.0");
                buf.push(3);
                buf.push(1);
                buf.extend_from_slice(&0u16.to_le_bytes())
            }
            Repetitions(repeat) => {
                buf.push(0xFF);
                buf.push(11);
                buf.extend_from_slice(b"NETSCAPE2.0");
                buf.push(3);
                buf.push(1);

                buf.extend_from_slice(&repeat.to_le_bytes())
            }
        }

        buf.push(0);
    }

    pub fn write_frame_header(
        buf: &mut Vec<u8>,
        frame: &Frame,
        delay: u16,
        interlaced: bool,
        dispose: DisposalMethod,
    ) {
        let t = frame.transparent.unwrap_or(0);
        Self::write_extension(
            buf,
            ExtensionData::Control {
                flags: dispose as u8 | 1 << 3,
                delay,
                transparency_idx: t,
            },
        );

        buf.push(0x2C);
        buf.extend_from_slice(&0u16.to_le_bytes()); // top
        buf.extend_from_slice(&0u16.to_le_bytes()); // left
        buf.extend_from_slice(&frame.width.to_le_bytes());
        buf.extend_from_slice(&frame.height.to_le_bytes());

        let mut flags = 0;
        if interlaced {
            flags |= 1 << 6;
        }

        if let Some(palette) = &frame.palette {
            flags |= 1 << 7; // local color table
            flags |= flag_size(palette.len() / 3);
            buf.push(flags);
            Self::write_color_table(buf, &palette);
        } else {
            buf.push(flags);
        }
    }

    pub fn write_frame(
        buf: &mut Vec<u8>,
        frame: &Frame,
        delay: u16,
        interlaced: bool,
        dispose: DisposalMethod,
    ) {
        Self::write_frame_header(buf, frame, delay, interlaced, dispose);
        Self::write_image_block(buf, &frame.buffer);
    }

    pub fn write_image_block(buf: &mut Vec<u8>, data: &[u8]) {
        let mut lzw = Vec::new();
        lzw_encode(&mut lzw, data);
        Self::write_encoded_image_block(buf, &lzw);
    }

    pub fn write_encoded_image_block(buf: &mut Vec<u8>, data: &[u8]) {
        let (&min_code_size, data) = data.split_first().unwrap_or((&2, &[]));
        buf.push(min_code_size);

        let mut iter = data.chunks_exact(0xFF);
        while let Some(chunk) = iter.next() {
            buf.push(0xFF);
            buf.extend_from_slice(chunk);
        }

        let rem = iter.remainder();
        if !rem.is_empty() {
            buf.push(rem.len() as u8);
            buf.extend_from_slice(rem);
        }

        buf.push(0);
    }

    pub fn write_trailer(buf: &mut Vec<u8>) {
        buf.push(0x3B);
    }
}

pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub transparent: Option<u8>,
    pub palette: Option<Vec<u8>>,
    pub buffer: Vec<u8>,
}

pub fn normalize_alpha(data: &mut [u8]) {
    for pix in data.chunks_exact_mut(4) {
        if pix[3] != 0 {
            pix[3] = 0xFF;
        }
    }
}

impl Frame {
    pub fn from_rgba(w: u16, h: u16, data: &[u8], speed: i32) -> Self {
        let mut transparent = None;

        for pix in data.chunks_exact(4) {
            if pix[3] == 0 {
                transparent = Some(pix);
            }
        }

        let nq = NeuQuant::new(speed, 256, &data);
        let palette = nq.color_map_rgb();

        Self {
            width: w,
            height: h,
            transparent: transparent.map(|t| nq.index_of(t) as u8),
            palette: Some(palette),
            buffer: data
                .chunks_exact(4)
                .map(|pix| nq.index_of(pix) as u8)
                .collect(),
        }
    }

    pub fn with_global_palette_rgba(w: u16, h: u16, data: &[u8], gp: &GlobalPalette) -> Self {
        let mut transparent = None;

        for pix in data.chunks_exact(4) {
            if pix[3] == 0 {
                transparent = Some(pix);
            }
        }

        Self {
            width: w,
            height: h,
            transparent: transparent.map(|t| gp.index_of(t) as u8),
            palette: None,
            buffer: data
                .chunks_exact(4)
                .map(|pix| gp.index_of(pix) as u8)
                .collect(),
        }
    }

    pub fn from_palatte_rgba(w: u16, h: u16, data: &[u8], palette: &[u8]) -> Self {
        Self {
            width: w,
            height: h,
            transparent: None,
            palette: Some(palette.to_vec()),
            buffer: data.to_vec(),
        }
    }

    pub fn from_indexed_rgba(w: u16, h: u16, data: &[u8]) -> Self {
        Self {
            width: w,
            height: h,
            transparent: None,
            palette: None,
            buffer: data.to_vec(),
        }
    }
}

#[derive(Copy, Clone)]
pub enum DisposalMethod {
    Any = 0,
    Keep = 1,
    Background = 2,
    Previous = 3,
}

pub enum ExtensionData {
    Control {
        flags: u8,
        delay: u16,
        transparency_idx: u8,
    },
    Repetitions(u16),
    InfiniteRepetitions,
}

// Color table size converted to flag bits
fn flag_size(size: usize) -> u8 {
    match size {
        0..=2 => 0,
        3..=4 => 1,
        5..=8 => 2,
        9..=16 => 3,
        17..=32 => 4,
        33..=64 => 5,
        65..=128 => 6,
        129..=256 => 7,
        _ => 7,
    }
}

pub struct GlobalPalette {
    nq: NeuQuant,
    palette: Vec<u8>,
}

impl GlobalPalette {
    // colors must be between 1 and 256
    pub fn new(speed: i32, colors: usize, data: &[u8]) -> Self {
        assert!(colors > 0 && colors <= 256);

        let nq = NeuQuant::new(speed, colors, data);
        let palette = nq.color_map_rgb();

        Self { nq, palette }
    }

    pub fn palette(&self) -> &[u8] {
        &self.palette
    }

    pub fn index_of(&self, pix: &[u8]) -> u8 {
        self.nq.index_of(pix) as u8
    }

    pub fn get_indexed_rgba(&self, data: &[u8]) -> Vec<u8> {
        data.chunks_exact(4).map(|pix| self.index_of(pix)).collect()
    }
}

pub fn lzw_encode(buf: &mut Vec<u8>, data: &[u8]) {
    let min_code_size = match flag_size(1 + data.iter().copied().max().unwrap_or(0) as usize) + 1 {
        1 => 2, // As per gif spec: The minimal code size has to be >= 2
        n => n,
    };

    buf.push(min_code_size);

    let mut encoder = LzwEncoder::new(BitOrder::Lsb, min_code_size);
    let len = encoder.into_vec(buf).encode_all(data).consumed_out;

    buf.truncate(len + 1);
}
