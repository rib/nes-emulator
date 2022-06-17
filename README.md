# rust-nes-emulator-embedded

[![Build](https://github.com/kamiyaowl/rust-nes-emulator-embedded/workflows/Build/badge.svg)](https://github.com/kamiyaowl/rust-nes-emulator-embedded/actions?query=workflow%3ABuild)

[original - github.com/kamiyaowl/rust-nes-emulator](https://github.com/kamiyaowl/rust-nes-emulator)

## `<project-root>/minimal`

In order to profile our implementation in STM32, we used to call Rust from C to implement it.
For rendering, we use [raylib](https://github.com/raysan5/raylib).

Through this project, we are analyzing and improving performance issues.

![minimal](https://user-images.githubusercontent.com/4300987/98466687-7c000b00-2214-11eb-8031-6e986602f14f.png)

## `<project-root>/stm32f7`

*TODO:*

An improved NES Emulator written in Rust on STM32 application.
It is expected to have better performance than the [original project](https://github.com/kamiyaowl/rust-nes-emulator#embedded-for-stm32f769) ...



# Links:

https://www.nesdev.com/NESDoc.pdf
https://github.com/starrhorne/nes-rust
https://github.com/AndreaOrru/LaiNES
https://github.com/fogleman/nes
https://wiki.nesdev.org/w/index.php/Emulator_tests

http://www.ffd2.com/fridge/docs/6502-NMOS.extra.opcodes (seems more reliable than undocumented_opcodes.txt)
https://www.nesdev.com/undocumented_opcodes.txt

https://github.com/bokuweb/rustynes

https://github.com/christopherpow/nes-test-roms