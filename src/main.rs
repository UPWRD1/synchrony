// main.rs
use cpal::traits::{DeviceTrait, HostTrait};
use druid::widget::{Flex, Slider};
use druid::{
    AppDelegate, AppLauncher, Command, DelegateCtx, Env, ExtEventSink, Lens, LocalizedString,
    Selector, Target, Widget, WidgetExt, WindowDesc,
};

#[derive(Clone, Default, Lens)]
struct AppState {
    channels: Vec<Channel>,
}

#[derive(Clone, Default, Lens)]
struct Channel {
    volume: f32,
}

impl Channel {
    fn new() -> Self {
        Channel { volume: 1.0 }
    }
}

const CHANNEL_VOLUME_CHANGED: Selector<f32> = Selector::new("channel-volume-changed");

fn main() {
    // Initialize audio
    let host = cpal::default_host();
    let default_output_device = host
        .default_output_device()
        .expect("No output device available");

    let output_format = default_output_device.default_output_format().unwrap();
    let event_sink = ExtEventSink::new();

    // Initialize GUI
    let main_window = WindowDesc::new(build_ui)
        .title(LocalizedString::new("Rust DAW"))
        .window_size((400.0, 200.0));

    let initial_state = AppState {
        channels: vec![Channel::new(), Channel::new()],
    };

    AppLauncher::with_window(main_window)
        .delegate(AppDelegate::default())
        .use_simple_logger()
        .launch(initial_state)
        .expect("Failed to launch application");
}

fn build_ui() -> impl Widget<AppState> {
    let mut flex = Flex::column();

    for i in 0..2 {
        let slider = Slider::new()
            .with_range(0.0, 1.0)
            .controller(ChannelVolumeController)
            .lens(Channel::volume);

        let channel_ui = Flex::row()
            .with_child(druid::widget::Label::new(format!("Channel {}", i + 1)))
            .with_spacer(10.0)
            .with_child(slider);

        flex.add_child(channel_ui);
    }

    flex.center()
}

struct ChannelVolumeController;

impl druid::controller::Controller<AppState, Slider<f32>, f32> for ChannelVolumeController {
    fn event(
        &mut self,
        child: &mut Slider<f32>,
        ctx: &mut druid::EventCtx,
        event: &druid::Event,
        data: &mut f32,
        env: &druid::Env,
    ) {
        match event {
            druid::Event::Command(cmd) if cmd.is(CHANNEL_VOLUME_CHANGED) => {
                child.set_value(ctx, *data, env);
                ctx.request_paint();
                ctx.request_layout();
            }
            _ => child.event(ctx, event, data, env),
        }
    }
}

impl AppDelegate<AppState> for AppDelegateImpl {
    fn command(
        &mut self,
        ctx: &mut DelegateCtx,
        _target: Target,
        cmd: &Command,
        data: &mut AppState,
        _env: &Env,
    ) {
        if cmd.is(CHANNEL_VOLUME_CHANGED) {
            // Handle volume change event
        }
    }
}

struct AppDelegateImpl;

impl Default for AppDelegateImpl {
    fn default() -> Self {
        Self {}
    }
}

impl druid::AppDelegate<AppState> for AppDelegateImpl {
    fn command(
        &mut self,
        _ctx: &mut druid::DelegateCtx,
        _target: druid::Target,
        cmd: &druid::Command,
        data: &mut AppState,
        _env: &druid::Env,
    ) {
        if let Some(new_volume) = cmd.get(CHANNEL_VOLUME_CHANGED) {
            // Handle volume change event
            println!("Channel volume changed: {}", new_volume);
        }
    }
}
