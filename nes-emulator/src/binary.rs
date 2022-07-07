use log::{debug};
use anyhow::anyhow;
use anyhow::Result;

use crate::constants::*;
use crate::prelude::{TVSystem, NameTableMirror};

#[derive(Debug)]
pub enum Type {
    NSF,
    INES,
    Unknown
}

pub struct INesConfig {
    pub mapper_number: u8,
    pub tv_system: TVSystem,
    pub n_prg_rom_pages: usize,
    pub n_prg_ram_pages: usize,
    pub n_chr_rom_pages: usize,
    pub n_chr_ram_pages: usize,
    pub has_chr_ram: bool,
    pub has_battery: bool,
    pub has_trainer: bool,
    pub nametable_mirror: NameTableMirror,
    pub ignore_mirror_control: bool,


    pub trainer_baseaddr: Option<usize>,
    pub prg_rom_baseaddr: usize,
    pub chr_rom_baseaddr: usize,
}

impl INesConfig {
    pub fn prg_rom_bytes(&self) -> usize {
        self.n_prg_rom_pages * PAGE_SIZE_16K
    }
    pub fn chr_rom_bytes(&self) -> usize {
        self.n_chr_rom_pages * PAGE_SIZE_8K
    }
    pub fn chr_ram_bytes(&self) -> usize {
        self.n_chr_ram_pages * PAGE_SIZE_8K
    }
}

#[derive(Debug)]
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
    pub tv_system: TVSystem,
    pub tv_system_byte: u8,
    pub uses_vrc6: bool,
    pub uses_vrc7: bool,
    pub uses_fds: bool,
    pub uses_mmc5: bool,
    pub uses_namco163: bool,
    pub uses_sunsoft5b: bool,
    pub uses_vt02plus: bool,
    pub prg_len: u32
}

pub enum NesBinaryConfig {
    Nsf(NsfConfig),
    INes(INesConfig),
}

pub fn check_type(binary: &[u8]) -> Type {
    const INES_HEADER: [u8; 4] = ['N' as u8, 'E' as u8, 'S' as u8, 0x1a /* character break */];
    const NSF_HEADER: [u8; 5] = ['N' as u8, 'E' as u8, 'S' as u8, 'M' as u8, 0x1a /* character break */];

    if binary.len() > NSF_HEADER.len() && binary[0..5] == NSF_HEADER {
        Type::NSF
    } else if binary.len() > INES_HEADER.len() && binary[0..4] == INES_HEADER {
        Type::INES
    } else {
        Type::Unknown
    }
}

fn cstr_len(cstr_slice: &[u8]) -> usize {
    for i in 0..cstr_slice.len() {
        if cstr_slice[i] == 0 {
            return i;
        }
    }
    return 0;
}

fn nsf_string_from_cstr_slice(cstr_slice: &[u8]) -> String {
    let len = cstr_len(cstr_slice);
    let slice = &cstr_slice[0..len];

    match std::str::from_utf8(slice) { Ok(s) => s.to_string(), Err(_err) => format!("Unknown") }
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
    let title = nsf_string_from_cstr_slice(&nsf[14..(14+32)]);
    let artist = nsf_string_from_cstr_slice(&nsf[46..(46+32)]);
    let copyright = nsf_string_from_cstr_slice(&nsf[78..(78+32)]);
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
        0 => TVSystem::Ntsc,
        1 => TVSystem::Pal,
        2 => TVSystem::Dual,
        _ => TVSystem::Unknown,
    };
    let uses = nsf[123];
    let uses_vrc6 =      (uses & 0b0000_0001) != 0;
    let uses_vrc7 =      (uses & 0b0000_0010) != 0;
    let uses_fds =       (uses & 0b0000_0100) != 0;
    let uses_mmc5 =      (uses & 0b0000_1000) != 0;
    let uses_namco163 =  (uses & 0b0001_0000) != 0;
    let uses_sunsoft5b = (uses & 0b0010_0000) != 0;
    let uses_vt02plus =  (uses & 0b0100_0000) != 0;
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

    let mut has_chr_ram = false;
    let n_prg_rom_pages = usize::from(ines[4]); // * 16KBしてあげる
    debug!("iNes: {} PRG ROM pages", n_prg_rom_pages);
    let n_chr_rom_pages = usize::from(ines[5]); // * 8KBしてあげる
    debug!("iNes: {} CHR ROM pages", n_chr_rom_pages);

    let mut n_chr_ram_pages = 0;
    if n_chr_rom_pages == 0 {
        has_chr_ram = true;
        n_chr_ram_pages = 1;
    }
    let n_prg_ram_pages = 2; // Need iNes 2.0 to configure properly

    let flags6 = ines[6];
    let has_battery = (flags6 & 0x02) == 0x02; // 0x6000 - 0x7fffのRAMを使わせる
    debug!("iNes: Has Battery {}", has_battery);
    let has_trainer = (flags6 & 0x04) == 0x04; // 512byte trainer at 0x7000-0x71ff in ines file
    debug!("iNes: Has Trainer {}", has_trainer);

    let is_mirroring_vertical = (flags6 & 0x01) == 0x01;

    let nametable_mirror = if is_mirroring_vertical {
        NameTableMirror::Vertical
    } else {
        NameTableMirror::Horizontal
    };
    debug!("iNes: Mirroring {:?}", nametable_mirror);

    let flags7 = ines[7];
    let _flags8 = ines[8];
    let _flags9 = ines[9];
    let flags10 = ines[10];
    let tv_system = match flags10 & 0b11 {
        0 => TVSystem::Ntsc,
        2 => TVSystem::Pal,
        1 | 3 => TVSystem::Dual,

        _ => { unreachable!() } // Rust compiler should know this is unreachable :/
    };
    debug!("iNes: TV System {:?}", tv_system);
    // 11~15 unused_padding
    debug_assert!(n_prg_rom_pages > 0);

    let header_bytes = 16;
    let trainer_bytes = if has_trainer { 512 } else { 0 };
    let prg_rom_bytes = n_prg_rom_pages * PAGE_SIZE_16K;
    let chr_rom_bytes = n_chr_rom_pages * PAGE_SIZE_8K;

    let trainer_baseaddr = if has_trainer { Some(header_bytes) } else { None };
    let prg_rom_baseaddr = header_bytes + trainer_bytes;
    let chr_rom_baseaddr = header_bytes + trainer_bytes + prg_rom_bytes;

    let mut mapper_number: u8 = 0;
    let low_nibble = (flags6 & 0b11110000) >> 4;
    mapper_number |= low_nibble;
    let high_nibble = flags7 & 0xF0;
    mapper_number |= high_nibble;

    Ok(INesConfig {
        mapper_number,
        tv_system,
        n_prg_rom_pages,
        n_prg_ram_pages,
        n_chr_rom_pages,
        n_chr_ram_pages,
        nametable_mirror,
        ignore_mirror_control: false, // FIXME
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
        Type::Unknown => Err(anyhow!("Unknown binary type"))
    }
}