use anyhow::anyhow;
use anyhow::Result;
#[allow(unused_imports)]
use log::{debug, warn};

use crate::cartridge::{NameTableMirror, TVSystemCompatibility};
use crate::constants::*;

#[derive(Debug)]
pub enum Type {
    NSF,
    INES,
    Unknown,
}

/// iNES file header
/// See: https://www.nesdev.org/wiki/INES
#[derive(Clone, Debug, Default)]
pub struct INesConfig {
    /// Format version
    /// Version 1: https://www.nesdev.org/wiki/INES
    /// Version 2: https://www.nesdev.org/wiki/NES_2.0
    pub version: u8,

    /// iNES allocated mapper number
    /// See: https://www.nesdev.org/wiki/Mapper
    pub mapper_number: u8,

    /// NTSC or PAL
    pub tv_system: TVSystemCompatibility,

    /// Number of 16K pages of program ROM
    pub n_prg_rom_pages: usize,

    /// Number of 16K pages of program ROM
    pub n_prg_ram_pages: usize,

    /// Number of 8K pages of CHR ROM
    pub n_chr_rom_pages: usize,

    /// Number of 8K pages of CHR RAM
    pub n_chr_ram_pages: usize,

    /// Does this mapper have writable CHR RAM?
    pub has_chr_ram: bool,

    /// Does this mapper have persistent WRAM
    pub has_battery: bool,

    /// Does the program ROM include additional 'trainer' code
    pub has_trainer: bool,

    /// The VRAM mirroring mode for nametables
    pub nametable_mirror: NameTableMirror,

    /// Override the `nametable_mirror` mode and provide four screens of VRAM
    pub four_screen_vram: bool,

    /// The optional file offset for trainer code
    pub trainer_baseaddr: Option<usize>,

    /// The file offset for the program ROM
    pub prg_rom_baseaddr: usize,

    /// The file offset for the CHR ROM
    pub chr_rom_baseaddr: usize,
}

impl INesConfig {
    /// Returns [`Self::n_prg_rom_pages`] converted into bytes (n_pages * 16K)
    pub fn prg_rom_bytes(&self) -> usize {
        self.n_prg_rom_pages * PAGE_SIZE_16K
    }

    /// Returns [`Self::n_chr_rom_pages`] converted into bytes (n_pages * 8K)
    pub fn chr_rom_bytes(&self) -> usize {
        self.n_chr_rom_pages * PAGE_SIZE_8K
    }

    /// Returns [`Self::n_chr_ram_pages`] converted into bytes (n_pages * 8K)
    pub fn chr_ram_bytes(&self) -> usize {
        self.n_chr_ram_pages * PAGE_SIZE_8K
    }
}

#[derive(Debug, Clone)]
pub struct NsfConfig {
    pub version: u8,
    pub n_songs: u8,
    pub first_song: u8, // base 1
    pub load_address: u16,
    pub init_address: u16,
    pub play_address: u16,
    pub title: String,
    pub artist: String,
    pub copyright: String,
    pub ntsc_play_speed: u16,
    pub pal_play_speed: u16,
    pub banks: [u8; 8],
    pub is_bank_switched: bool,
    pub tv_system: TVSystemCompatibility,
    pub tv_system_byte: u8,
    pub uses_vrc6: bool,
    pub uses_vrc7: bool,
    pub uses_fds: bool,
    pub uses_mmc5: bool,
    pub uses_namco163: bool,
    pub uses_sunsoft5b: bool,
    pub uses_vt02plus: bool,
    pub prg_len: u32,
}

#[derive(Clone, Debug)]
pub enum NesBinaryConfig {
    Nsf(NsfConfig),
    INes(INesConfig),
    None,
}

pub fn check_type(binary: &[u8]) -> Type {
    const INES_HEADER: [u8; 4] = [b'N', b'E', b'S', 0x1a /* character break */];
    const NSF_HEADER: [u8; 5] = [b'N', b'E', b'S', b'M', 0x1a /* character break */];

    if binary.len() > NSF_HEADER.len() && binary[0..5] == NSF_HEADER {
        Type::NSF
    } else if binary.len() > INES_HEADER.len() && binary[0..4] == INES_HEADER {
        Type::INES
    } else {
        Type::Unknown
    }
}

fn cstr_len(cstr_slice: &[u8]) -> usize {
    for (i, c) in cstr_slice.iter().enumerate() {
        if *c == 0 {
            return i;
        }
    }
    0
}

fn nsf_string_from_cstr_slice(cstr_slice: &[u8]) -> String {
    let len = cstr_len(cstr_slice);
    let slice = &cstr_slice[0..len];

    match std::str::from_utf8(slice) {
        Ok(s) => s.to_string(),
        Err(_err) => "Unknown".to_string(),
    }
}

pub fn parse_nsf_header(nsf: &[u8]) -> Result<NsfConfig> {
    debug!("Parsing NSF header...");

    if !matches!(check_type(nsf), Type::NSF) {
        return Err(anyhow!("Missing NSF file marker"));
    }
    if nsf.len() < 128 {
        return Err(anyhow!("To small to be a valid NSF file"));
    }

    let version = nsf[5];
    let n_songs = nsf[6];
    let first_song = nsf[7];
    let load_address = nsf[8] as u16 | (nsf[9] as u16) << 8;
    let init_address = nsf[10] as u16 | (nsf[11] as u16) << 8;
    let play_address = nsf[12] as u16 | (nsf[13] as u16) << 8;
    let title = nsf_string_from_cstr_slice(&nsf[14..(14 + 32)]);
    let artist = nsf_string_from_cstr_slice(&nsf[46..(46 + 32)]);
    let copyright = nsf_string_from_cstr_slice(&nsf[78..(78 + 32)]);
    let ntsc_play_speed = nsf[110] as u16 | (nsf[111] as u16) << 8;
    let pal_play_speed = nsf[120] as u16 | (nsf[121] as u16) << 8;
    let mut banks: [u8; 8] = [0u8; 8];
    let mut is_bank_switched = false;
    for i in 0..8 {
        banks[i] = nsf[112 + i];
        if banks[i] != 0 {
            is_bank_switched = true;
        }
    }
    let tv_system_byte = nsf[112];
    let tv_system = match tv_system_byte {
        0 => TVSystemCompatibility::Ntsc,
        1 => TVSystemCompatibility::Pal,
        2 => TVSystemCompatibility::Dual,
        _ => TVSystemCompatibility::Unknown,
    };
    let uses = nsf[123];
    let uses_vrc6 = (uses & 0b0000_0001) != 0;
    let uses_vrc7 = (uses & 0b0000_0010) != 0;
    let uses_fds = (uses & 0b0000_0100) != 0;
    let uses_mmc5 = (uses & 0b0000_1000) != 0;
    let uses_namco163 = (uses & 0b0001_0000) != 0;
    let uses_sunsoft5b = (uses & 0b0010_0000) != 0;
    let uses_vt02plus = (uses & 0b0100_0000) != 0;
    let mut prg_len = nsf[125] as u32 | (nsf[126] as u32) << 8 | (nsf[127] as u32) << 16;
    if prg_len == 0 {
        prg_len = (nsf.len() - 128) as u32;
    }

    Ok(NsfConfig {
        version,
        n_songs,
        first_song,
        load_address,
        init_address,
        play_address,
        title,
        artist,
        copyright,
        ntsc_play_speed,
        pal_play_speed,
        banks,
        is_bank_switched,
        tv_system,
        tv_system_byte,
        uses_vrc6,
        uses_vrc7,
        uses_fds,
        uses_mmc5,
        uses_namco163,
        uses_sunsoft5b,
        uses_vt02plus,
        prg_len,
    })
}

pub fn parse_ines_header(ines: &[u8]) -> Result<INesConfig> {
    debug!("Parsing iNes header...");

    if !matches!(check_type(ines), Type::INES) {
        return Err(anyhow!("Missing iNES file marker"));
    }

    let version = if ines[7] & 0x0C == 0x08 { 2 } else { 1 };
    debug!("iNes: Version {version}");
    if version == 2 {
        warn!("iNes 2.0 fields aren't supported yet - will be read as a 1.0 format file");
    }
    // TODO: actually support parsing iNES 2.0 fields

    let mut has_chr_ram = false;
    let n_prg_rom_pages = usize::from(ines[4]); // * 16KBしてあげる
    let n_chr_rom_pages = usize::from(ines[5]); // * 8KBしてあげる

    let mut n_chr_ram_pages = 0;
    if n_chr_rom_pages == 0 {
        has_chr_ram = true;
        n_chr_ram_pages = 1;
    }
    let n_prg_ram_pages = 2; // Need iNes 2.0 to configure properly

    let flags6 = ines[6];
    let has_battery = (flags6 & 0x02) == 0x02; // 0x6000 - 0x7fffのRAMを使わせる
    debug!("iNes: Has Battery: {}", has_battery);
    let has_trainer = (flags6 & 0x04) == 0x04; // 512byte trainer at 0x7000-0x71ff in ines file
    debug!("iNes: Has Trainer: {}", has_trainer);

    let is_mirroring_vertical = (flags6 & 0x01) == 0x01;

    let nametable_mirror = if is_mirroring_vertical {
        NameTableMirror::Vertical
    } else {
        NameTableMirror::Horizontal
    };
    debug!("iNes: Mirroring {:?}", nametable_mirror);
    let four_screen_vram = flags6 & 0b1000 != 0;
    debug!(
        "iNes: Four screen VRAM (Mirroring override): {}",
        four_screen_vram
    );

    let flags7 = ines[7];
    let _flags8 = ines[8];
    let _flags9 = ines[9];
    let flags10 = ines[10];
    let tv_system = match flags10 & 0b11 {
        0 => TVSystemCompatibility::Ntsc,
        2 => TVSystemCompatibility::Pal,
        1 | 3 => TVSystemCompatibility::Dual,

        _ => {
            unreachable!()
        } // Rust compiler should know this is unreachable :/
    };
    debug!("iNes: TV System {:?}", tv_system);
    // 11~15 unused_padding
    debug_assert!(n_prg_rom_pages > 0);

    let header_bytes = 16;
    let trainer_bytes = if has_trainer { 512 } else { 0 };
    let prg_rom_bytes = n_prg_rom_pages * PAGE_SIZE_16K;
    debug!("iNes: {n_prg_rom_pages} PRG ROM pages x 16k = {prg_rom_bytes} bytes");
    let chr_rom_bytes = n_chr_rom_pages * PAGE_SIZE_8K;
    debug!("iNes: {n_chr_rom_pages} CHR ROM pages x 8k = {chr_rom_bytes} bytes");

    let trainer_baseaddr = if has_trainer {
        Some(header_bytes)
    } else {
        None
    };
    let prg_rom_baseaddr = header_bytes + trainer_bytes;
    let chr_rom_baseaddr = header_bytes + trainer_bytes + prg_rom_bytes;

    let mut mapper_number: u8 = 0;
    let low_nibble = (flags6 & 0b11110000) >> 4;
    mapper_number |= low_nibble;
    let high_nibble = flags7 & 0xF0;
    mapper_number |= high_nibble;

    Ok(INesConfig {
        version,
        mapper_number,
        tv_system,
        n_prg_rom_pages,
        n_prg_ram_pages,
        n_chr_rom_pages,
        n_chr_ram_pages,
        nametable_mirror,
        four_screen_vram,
        has_battery,
        has_chr_ram,
        has_trainer,

        trainer_baseaddr,
        prg_rom_baseaddr,
        chr_rom_baseaddr,
    })
}

pub fn parse_any_header(binary: &[u8]) -> Result<NesBinaryConfig> {
    match check_type(binary) {
        Type::INES => Ok(NesBinaryConfig::INes(parse_ines_header(binary)?)),
        Type::NSF => Ok(NesBinaryConfig::Nsf(parse_nsf_header(binary)?)),
        Type::Unknown => Err(anyhow!("Unknown binary type")),
    }
}
