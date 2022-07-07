use super::interface::*;

#[derive(Copy, Clone)]
pub enum PadButton {
    A,
    B,
    Select,
    Start,
    Up,
    Down,
    Left,
    Right,
}

trait ControllerIO {
    fn start_frame(&mut self);

    fn press_button(&mut self, button: PadButton);
    fn release_button(&mut self, button: PadButton);

    /// $4016/7 reads
    fn read(&mut self) -> u8;

    /// $4016/7 reads without side effects
    fn peek(&mut self) -> u8;

    /// $4016 writes
    fn write(&mut self, value: u8);

}

#[derive(Clone)]
pub struct StandardControllerState {
    // Considering that the emulation of a frame might be done over a
    // very short period there is an increased chance that it may miss
    // inputs (whereas the controllers can be polled at uniform time
    // intervals on original hardware)
    //
    // Assuming the emulator is synchronized with wall-clock time on
    // per-frame basis we add a per-frame latch for all controller
    // inputs that effectively cause button presses remain latched
    // until the end of the current frame so it's less likely they
    // will be missed.
    //
    // Note: the original plan had been to also reset the latch whenever
    // the controller state was read but since the $4016/7 controller
    // registers are also APU registers we can't differentiate writes
    // aimed at the APU vs the controller so we don't really know
    // when the controller state has been read - thus we only reset
    // the latches at the start of each frame.

    pub poll_mode: bool,
    pub button_presses: u8,
    pub button_press_latches: u8,
    pub controller_shift: u8,
}

fn debug_print_buttons_pressed(buttons: u8) {
    if buttons & 1 != 0 { println!("> A pressed"); }
    if buttons & 2 != 0 { println!("> B pressed"); }
    if buttons & 4 != 0 { println!("> Select pressed"); }
    if buttons & 8 != 0 { println!("> Start pressed"); }
    if buttons & 16 != 0 { println!("> Up pressed"); }
    if buttons & 32 != 0 { println!("> Down pressed"); }
    if buttons & 64 != 0 { println!("> Left pressed"); }
    if buttons & 128 != 0 { println!("> Right pressed"); }
}

impl ControllerIO for StandardControllerState {

    fn start_frame(&mut self) {
        self.button_press_latches = self.button_presses;
    }

    fn press_button(&mut self, button: PadButton) {
        match button {
            PadButton::A => self.button_presses = self.button_presses | 0x01u8,
            PadButton::B => self.button_presses = self.button_presses | 0x02u8,
            PadButton::Select => self.button_presses = self.button_presses | 0x04u8,
            PadButton::Start => self.button_presses = self.button_presses | 0x08u8,
            PadButton::Up => self.button_presses = self.button_presses | 0x10u8,
            PadButton::Down => self.button_presses = self.button_presses | 0x20u8,
            PadButton::Left => self.button_presses = self.button_presses | 0x40u8,
            PadButton::Right => self.button_presses = self.button_presses | 0x80u8,
        }
        self.button_press_latches |= self.button_presses;
        //println!("Press Button:");
        //debug_print_buttons_pressed(self.button_presses);
    }

    fn release_button(&mut self, button: PadButton) {
        match button {
            PadButton::A => self.button_presses = self.button_presses & (!0x01u8),
            PadButton::B => self.button_presses = self.button_presses & (!0x02u8),
            PadButton::Select => self.button_presses = self.button_presses & (!0x04u8),
            PadButton::Start => self.button_presses = self.button_presses & (!0x08u8),
            PadButton::Up => self.button_presses = self.button_presses & (!0x10u8),
            PadButton::Down => self.button_presses = self.button_presses & (!0x20u8),
            PadButton::Left => self.button_presses = self.button_presses & (!0x40u8),
            PadButton::Right => self.button_presses = self.button_presses & (!0x80u8),
        }
    }

    // $4016/7 reads
    fn read(&mut self) -> u8 {
        if self.poll_mode {
            //println!("Read poll mode A button");
            // "While S (strobe) is high, the shift registers in the controllers are continuously reloaded
            // from the button states, and reading $4016/$4017 will keep returning the current state of
            // the first button (A)."
            self.button_press_latches & 1
        } else {
            let value = self.controller_shift & 1;

            self.controller_shift >>= 1;
            // "After 8 bits are read, all subsequent bits will report 1 on a standard NES controller,"
            self.controller_shift |= 0b1000_0000;

            value
        }
    }

    fn peek(&mut self) -> u8 {
        if self.poll_mode {
            self.button_press_latches & 1
        } else {
            self.controller_shift & 1
        }
    }

    // $4016 writes
    fn write(&mut self, value: u8) {
        let prev = self.poll_mode;

        self.poll_mode = value & 1 != 0;

        if self.poll_mode == false && prev == true {
            self.controller_shift = self.button_press_latches;
            //println!("Updated controller shift register:");
            //debug_print_buttons_pressed(self.controller_shift);
        }
    }
}


#[derive(Clone)]
pub enum Controller {
    StandardController(StandardControllerState),
}

#[derive(Clone)]
pub struct Pad {
    controller: Controller,
}

impl Default for Pad {
    fn default() -> Self {
        Self {
            controller: Controller::StandardController(StandardControllerState {
                poll_mode: false,
                button_presses: 0,
                button_press_latches: 0,
                controller_shift: 0
            })
        }
    }
}

impl Pad {
    pub fn write_register(&mut self, value: u8) {
        match &mut self.controller {
            Controller::StandardController(state) => state.write(value),
        }
    }

    pub fn update_button_press_latches(&mut self) {
        match &mut self.controller {
            Controller::StandardController(state) => state.start_frame(),
        }
    }

    pub fn read(&mut self) -> u8 {
        match &mut self.controller {
            Controller::StandardController(state) => state.read(),
        }
    }

    pub fn peek(&mut self) -> u8 {
        match &mut self.controller {
            Controller::StandardController(state) => state.peek(),
        }
    }

    pub fn press_button(&mut self, button: PadButton) {
        match &mut self.controller {
            Controller::StandardController(state) => state.press_button(button),
        }
    }

    pub fn release_button(&mut self, button: PadButton) {
        match &mut self.controller {
            Controller::StandardController(state) => state.release_button(button),
        }
    }

}
