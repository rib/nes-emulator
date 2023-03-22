//use clap::Parser;

use nes_emulator_shell as nes_shell;

use ::winit::platform::android::activity::AndroidApp;

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: AndroidApp) {
    use ::winit::platform::android::EventLoopBuilderExtAndroid;

    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("nes-emulator")
            .with_min_level(log::Level::Debug)
            .with_filter(
                android_logger::FilterBuilder::new()
                    .parse("debug,naga=warn,wgpu=warn")
                    .build(),
            ),
    );
    //android_logger::init_once(android_logger::Config::default().with_min_level(log::Level::Info));

    log::debug!("NES Emulator: main()");
    let args = nes_shell::Args::default();

    let options = eframe::NativeOptions {
        event_loop_builder: Some(Box::new(move |builder| {
            builder.with_android_app(app);
        })),
        ..eframe::NativeOptions::default()
    };

    nes_shell::ui::eframe::native_ui_main(args, options).expect("Failure running UI main");
}
