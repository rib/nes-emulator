use crate::system::Model;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ControllerButton {
    A = 0,
    B = 1,
    Select = 2,
    Start = 3,
    Up = 4,
    Down = 5,
    Left = 6,
    Right = 7,
}
impl TryFrom<u8> for ControllerButton {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ControllerButton::A),
            1 => Ok(ControllerButton::B),
            2 => Ok(ControllerButton::Select),
            3 => Ok(ControllerButton::Start),
            4 => Ok(ControllerButton::Up),
            5 => Ok(ControllerButton::Down),
            6 => Ok(ControllerButton::Left),
            7 => Ok(ControllerButton::Right),
            _ => Err(()),
        }
    }
}

#[derive(PartialEq, Eq)]
pub enum ControllerKind {
    StandardController,
    Zapper,
}

trait ControllerIO {
    fn kind(&mut self) -> ControllerKind;

    fn power_cycle(&mut self, model: Model);

    /// Called at the end of each frame - can be used to latch
    /// button presses over a frame
    fn finish_frame(&mut self) {}

    /// Update internal state based on cpu clock progress - can be used to
    /// decay internal state (such as pull-up capacitors)
    fn progress(&mut self, cpu_clock: u64) {}

    /// $4016/7 reads
    fn read(&mut self) -> u8;

    /// $4016/7 reads without side effects
    fn peek(&mut self) -> u8;

    /// $4016 writes
    fn write(&mut self, value: u8);
}


#[derive(Clone, Default)]
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


#[allow(dead_code)]
fn debug_print_buttons_pressed(buttons: u8) {
    if buttons & 1 != 0 {
        println!("> A pressed");
    }
    if buttons & 2 != 0 {
        println!("> B pressed");
    }
    if buttons & 4 != 0 {
        println!("> Select pressed");
    }
    if buttons & 8 != 0 {
        println!("> Start pressed");
    }
    if buttons & 16 != 0 {
        println!("> Up pressed");
    }
    if buttons & 32 != 0 {
        println!("> Down pressed");
    }
    if buttons & 64 != 0 {
        println!("> Left pressed");
    }
    if buttons & 128 != 0 {
        println!("> Right pressed");
    }
}

impl ControllerIO for StandardControllerState {
    fn kind(&mut self) -> ControllerKind {
        ControllerKind::StandardController
    }

    fn power_cycle(&mut self, model: Model) {
        *self = Default::default();
    }

    fn finish_frame(&mut self) {
        self.button_press_latches = self.button_presses;
    }

    fn progress(&mut self, cpu_clock: u64) {
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

        if !self.poll_mode && prev {
            self.controller_shift = self.button_press_latches;
            //println!("Updated controller shift register:");
            //debug_print_buttons_pressed(self.controller_shift);
        }
    }
}

impl StandardControllerState {


    fn press_button(&mut self, button: ControllerButton) {
        match button {
            ControllerButton::A => self.button_presses |= 0x01u8,
            ControllerButton::B => self.button_presses |= 0x02u8,
            ControllerButton::Select => self.button_presses |= 0x04u8,
            ControllerButton::Start => self.button_presses |= 0x08u8,
            ControllerButton::Up => self.button_presses |= 0x10u8,
            ControllerButton::Down => self.button_presses |= 0x20u8,
            ControllerButton::Left => self.button_presses |= 0x40u8,
            ControllerButton::Right => self.button_presses |= 0x80u8,
        }
        self.button_press_latches |= self.button_presses;
        //println!("Press Button:");
        //debug_print_buttons_pressed(self.button_presses);
    }

    fn release_button(&mut self, button: ControllerButton) {
        match button {
            ControllerButton::A => self.button_presses &= !0x01u8,
            ControllerButton::B => self.button_presses &= !0x02u8,
            ControllerButton::Select => self.button_presses &= !0x04u8,
            ControllerButton::Start => self.button_presses &= !0x08u8,
            ControllerButton::Up => self.button_presses &= !0x10u8,
            ControllerButton::Down => self.button_presses &= !0x20u8,
            ControllerButton::Left => self.button_presses &= !0x40u8,
            ControllerButton::Right => self.button_presses &= !0x80u8,
        }
    }

    fn peek_button(&self, button: ControllerButton) -> bool {
        match button {
            ControllerButton::A => self.button_presses & 0x01u8 != 0,
            ControllerButton::B => self.button_presses & 0x02u8 != 0,
            ControllerButton::Select => self.button_presses & 0x04u8 != 0,
            ControllerButton::Start => self.button_presses & 0x08u8 != 0,
            ControllerButton::Up => self.button_presses & 0x10u8 != 0,
            ControllerButton::Down => self.button_presses & 0x20u8 != 0,
            ControllerButton::Left => self.button_presses & 0x40u8 != 0,
            ControllerButton::Right => self.button_presses & 0x80u8 != 0,
        }
    }
}

#[derive(Clone, Default)]
pub struct ZapperState {

    pub poll_mode: bool,
    pub pos: Option<[u8; 2]>,
    pub half_triggered: bool,

    /// The light sensor is active whenever the "voltage" is above zero
    ///
    /// We model the sensor as having a capacitance that will decay
    /// the voltage exponentially each scan line.
    pub sensor_line_voltage: u8,

    // Vs. System has shift register like standard controller
    // pub controller_shift: u8,
}

impl ControllerIO for ZapperState {
    fn kind(&mut self) -> ControllerKind {
        ControllerKind::Zapper
    }

    fn power_cycle(&mut self, model: Model) {
        *self = Default::default();
    }

    fn finish_frame(&mut self) {
    }

    fn progress(&mut self, cpu_clock: u64) {
    }

    // $4016/7 reads
    fn read(&mut self) -> u8 {
        let mut value: u8 = 0;
        if self.half_triggered { value |= 0b0001_0000; }
        if self.sensor_line_voltage > 0 { value |= 0b0000_1000; }
        value
    }

    fn peek(&mut self) -> u8 {
        self.read()
    }

    // $4016 writes
    fn write(&mut self, value: u8) {

    }

}

impl ZapperState {

    /// Set to `true` when the Zapper trigger is half pressed and `false`
    /// when the trigger is released or fully pressed.
    fn set_half_trigger(&mut self, half_triggered: bool) {
        self.half_triggered = half_triggered;
    }

    /// Set to 0xff for white. Will decay exponentially and
    fn set_luminance_level(&mut self, level: u8) {
        // Convert the luminance level into a number of scanlines to decay the level over
        // based on the observation on the nesvdev wiki:
        //
        // "Tests in the Zap Ruder test ROM show that the photodiode stays on for
        //  about 26 scanlines with pure white, 24 scanlines with light gray, or
        //  19 lines with dark gray"
        //
        // Function courtesy of GPT-3
        self.sensor_line_voltage = (26.0 * (1.0 - (-0.02 * level as f32).exp())).round() as u8;
    }

    // exponentially decay the light level
    fn step_scanline(&mut self) {
        // nesdev:
        // "For an emulator developer, one useful model of the light sensor's
        // behavior is that luminance is collected as voltage into a capacitor,
        // whose voltage drains out exponentially over the course of several
        // scanlines, and the light bit is active while the voltage is above a
        // threshold"
        if self.sensor_line_voltage > 0 {
            self.sensor_line_voltage -= 1;
        }
    }

    /// Maintains the current screen position that the Zapper is pointed
    /// at, or `None` if pointing offscreen
    fn set_pos(&mut self, pos: Option<[u8; 2]>) {
        self.pos = pos;
    }

    /// Queries the current screen position that the Zapper is pointed at,
    /// or `None` if pointing offscreen
    fn get_pos(&self) -> Option<[u8; 2]> {
        self.pos
    }
}

#[derive(Clone)]
pub enum Controller {
    StandardController(StandardControllerState),
    Zapper(ZapperState)
}

impl Controller {
    fn as_io_mut(&mut self) -> &mut dyn ControllerIO {
        match self {
            Controller::StandardController(state) => state as &mut dyn ControllerIO,
            Controller::Zapper(state) => state as &mut dyn ControllerIO,
        }
    }
}

impl Default for Controller {
    fn default() -> Self {
        Controller::StandardController(StandardControllerState::default())
    }
}

#[derive(Clone)]
pub struct Port {
    model: Model,
    controller: Controller,
}

impl Port {
    pub(crate) fn new(model: Model) -> Self {
        Self {
            model,
            controller: Controller::StandardController(StandardControllerState {
                poll_mode: false,
                button_presses: 0,
                button_press_latches: 0,
                controller_shift: 0,
            }),
        }
    }

    pub fn kind(&mut self) -> ControllerKind {
        self.controller.as_io_mut().kind()
    }

    pub fn power_cycle(&mut self) {
        self.controller.as_io_mut().power_cycle(self.model);
    }

    /// Plug a controller peripheral into this port
    pub fn plug(&mut self, controller: Controller) {
        self.controller = controller;
        self.controller.as_io_mut().power_cycle(self.model);
    }

    /// Called at the end of each frame - can be used to latch
    /// button presses over a frame
    pub(crate) fn finish_frame(&mut self) {
        self.controller.as_io_mut().finish_frame();
    }

    /// Update internal state based on cpu clock progress - can be used to
    /// decay internal state (such as pull-up capacitors)
    pub(crate) fn progress(&mut self, cpu_clock: u64)
    {
        self.controller.as_io_mut().progress(cpu_clock);
    }

    /// $4016/7 reads
    pub fn read(&mut self) -> u8 {
        self.controller.as_io_mut().read()
    }

    /// $4016/7 reads without side effects
    pub fn peek(&mut self) -> u8 {
        self.controller.as_io_mut().peek()
    }

    // $4016 writes
    pub fn write_register(&mut self, value: u8) {
        self.controller.as_io_mut().write(value);
    }

    pub fn press_button(&mut self, button: ControllerButton) {
        match &mut self.controller {
            Controller::StandardController(state) => state.press_button(button),
            Controller::Zapper(_) => { /* ignore */ }
        }
    }

    pub fn release_button(&mut self, button: ControllerButton) {
        match &mut self.controller {
            Controller::StandardController(state) => state.release_button(button),
            Controller::Zapper(_) => { /* ignore */ }
        }
    }

    pub fn peek_button(&self, button: ControllerButton) -> bool {
        match &self.controller {
            Controller::StandardController(state) => state.peek_button(button),
            Controller::Zapper(_) => { /* ignore */ false }
        }
    }

    /// Explicitly set the Zapper trigger on or off
    /// The trigger can be fired by setting and then immediately clearing and the
    /// trigger will internally stay active until the emulated pull-up capacitor
    /// has drained.
    ///
    /// NOP for standard controllers
    pub fn set_trigger(&mut self, trigger: bool) {
        match &mut self.controller {
            Controller::StandardController(_) => { /* ignore */ }
            Controller::Zapper(state) => state.set_half_trigger(trigger)
        }
    }

    /// Updates the luminance level for the Zapper
    ///
    /// NOP for standard controllers
    pub(crate) fn set_luminance_level(&mut self, luminance: u8) {
        match &mut self.controller {
            Controller::StandardController(_) => { /* ignore */ }
            Controller::Zapper(state) => state.set_luminance_level(luminance)
        }
    }

    /// Updates where a Zapper is currently pointing - NOP for standard controllers
    /// `None` indicates that the zapper is pointing offscreen
    pub fn set_pos(&mut self, pos: Option<[u8; 2]>) {
        match &mut self.controller {
            Controller::StandardController(_) => { /* ignore */ }
            Controller::Zapper(state) => state.set_pos(pos)
        }
    }

    /// Queries where a Zapper is currently pointing - returns `None` for standard controllers
    /// `None` indicates that the Zapper is pointing offscreen
    pub fn get_pos(&mut self) -> Option<[u8; 2]> {
        match &mut self.controller {
            Controller::StandardController(_) => { /* ignore */ None }
            Controller::Zapper(state) => state.get_pos()
        }
    }
}
