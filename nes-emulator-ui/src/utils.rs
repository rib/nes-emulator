use std::{path::{Path, PathBuf}, time::Instant, str::FromStr};

use anyhow::Result;

use nes_emulator::{cartridge::Cartridge, nes::Nes, system::Model};

pub fn epoch_timestamp() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => panic!("SystemTime before UNIX EPOCH!"),
    }
}

pub fn create_nes_from_binary(path: impl AsRef<Path>, audio_sample_rate: u32, start_timestamp: Instant) -> Result<Nes> {
    let rom = std::fs::read(path)?;
    let cartridge = Cartridge::from_binary(&rom)?;
    let model = match cartridge.tv_system() {
        nes_emulator::cartridge::TVSystemCompatibility::Pal => Model::Pal,
        _ => Model::Ntsc
    };
    let mut nes = Nes::new(model, audio_sample_rate, start_timestamp);
    nes.insert_cartridge(Some(cartridge))?;
    nes.power_cycle(start_timestamp);
    Ok(nes)
}

pub fn canonicalize_rom_dirs(rom_dirs: &Vec<String>) -> Vec<PathBuf> {
    rom_dirs.iter()
        .map(|s| PathBuf::from_str(s).unwrap())
        .filter_map(|dir| {
            if !dir.exists() { return None; }

            match dir.canonicalize() {
                Ok(dir) => Some(dir),
                Err(_) => None
            }
        }).collect()
}

/// Finds the shortest path for the given rom, relative to the given rom directories
///
/// Assumes rom_dirs have already been filtered with [`utils::canonicalize_rom_dirs`]
pub fn find_shortest_rom_path(rom: &PathBuf, rom_dirs: &Vec<PathBuf>) -> Result<PathBuf> {
    let rom = rom.canonicalize()?;
    let mut best = rom.clone();
    let mut best_len = best.to_string_lossy().len();

    for dir in rom_dirs.iter() {
        if rom.starts_with(dir) {
            let stripped = rom.strip_prefix(dir).unwrap();
            let len = stripped.to_string_lossy().len();
            if len < best_len {
                best_len = len;
                best = stripped.to_path_buf();
            }
        }
    }

    Ok(best)
}

pub fn find_rom<P: AsRef<std::path::Path>>(path: P, rom_dirs: &Vec<PathBuf>) -> Option<PathBuf> {
    let path = path.as_ref();
    if path.exists() {
        return Some(path.into());
    } else {
        for parent in rom_dirs.iter() {
            let abs = parent.join(&path);
            if abs.exists() {
                return Some(abs);
            }
        }
    }

    None
}