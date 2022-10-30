//use clap::Parser;

use nes_emulator_shell as nes_shell;
use nes_shell::{Args, dispatch_main};

use ::winit::platform::android::activity::AndroidApp;

#[cfg(target_os="android")]
#[no_mangle]
fn android_main(app: AndroidApp) {
    use ::winit::platform::android::EventLoopBuilderExtAndroid;

    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("nes-emulator")
            .with_min_level(log::Level::Debug)
            .with_filter(android_logger::FilterBuilder::new().parse("debug,naga=warn,wgpu=warn").build()
        )
    );
    //android_logger::init_once(android_logger::Config::default().with_min_level(log::Level::Info));

    let args = Args::default();

    let event_loop = if !args.headless {
        let event_loop: ::winit::event_loop::EventLoop<nes_shell::ui::winit::Event> =
            ::winit::event_loop::EventLoopBuilder::with_user_event()
                .with_android_app(app)
                .build();
        Some(event_loop)
    } else {
        None
    };

    dispatch_main(args, event_loop);
}