#include "pch.h"
#include <ppu.h>

#pragma once

extern "C" {

PPUSim::PPU *ppu_sim_new(PPUSim::Revision revision);
void ppu_sim_drop(PPUSim::PPU *ppu);

}
