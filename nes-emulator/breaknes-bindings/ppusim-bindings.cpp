#include "ppusim-bindings.h"

PPUSim::PPU *ppu_sim_new(PPUSim::Revision revision) {
  return new PPUSim::PPU(revision, false /* hle mode */, false /* video gen */);
}

void ppu_sim_drop(PPUSim::PPU *ppu) {
    delete ppu;
}
