mod converters;
mod winit_windows;

pub use winit_windows::*;

use bevy_app::{App, CoreStage, Events, ManualEventReader, Plugin};
use bevy_ecs::prelude::*;
use bevy_input::{
    keyboard::KeyboardInput,
    mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel},
    touch::TouchInput,
};
use bevy_math::{ivec2, DVec2, Vec2};
use bevy_render::{RenderApp, RenderAppChannel, RenderSystem};
use bevy_utils::tracing::{error, trace, warn};
use bevy_window::{
    CreateWindow, CursorEntered, CursorLeft, CursorMoved, FileDragAndDrop, ReceivedCharacter,
    WindowCloseRequested, WindowCreated, WindowFocused, WindowMoved, WindowResized,
    WindowScaleFactorChanged, Windows,
};
use crossbeam_channel::{Receiver, Sender};
use winit::{
    dpi::PhysicalPosition,
    event::{self, DeviceEvent, Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget},
};

use winit::dpi::LogicalSize;

#[derive(Default)]
pub struct WinitPlugin;

impl Plugin for WinitPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WinitWindows>()
            .set_runner(winit_runner)
            .add_system_to_stage(CoreStage::PostUpdate, update_windows.exclusive_system())
            .add_system_to_stage(
                CoreStage::Last,
                request_window_redraws
                    .exclusive_system()
                    .at_end()
                    .before(RenderSystem::Extract),
            );
        let event_loop = EventLoop::new();
        handle_initial_window_events(&mut app.world, &event_loop);
        app.insert_non_send_resource(event_loop);
    }
}

fn run<F>(event_loop: EventLoop<()>, event_handler: F) -> !
where
    F: 'static + FnMut(Event<'_, ()>, &EventLoopWindowTarget<()>, &mut ControlFlow),
{
    event_loop.run(event_handler)
}

#[derive(Clone)]
pub struct WinitWindowEventChannel {
    pub sender: Sender<WinitWindowEvent>,
    pub receiver: Receiver<WinitWindowEvent>,
}

impl Default for WinitWindowEventChannel {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
    }
}

pub struct WinitWindowEvent {
    event: WindowEvent<'static>,
    id: winit::window::WindowId,
}

#[derive(Clone)]
pub struct WinitDeviceEventChannel {
    pub sender: Sender<WinitDeviceEvent>,
    pub receiver: Receiver<WinitDeviceEvent>,
}

impl Default for WinitDeviceEventChannel {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
    }
}

pub struct WinitDeviceEvent {
    event: DeviceEvent,
    id: winit::event::DeviceId,
}

pub fn winit_runner(mut app: App) {
    let event_loop = app.world.remove_non_send::<EventLoop<()>>().unwrap();
    let window_event_channel = WinitWindowEventChannel::default();
    let device_event_channel = WinitDeviceEventChannel::default();
    app.insert_resource(window_event_channel.clone())
        .insert_resource(device_event_channel.clone())
        .insert_non_send_resource(event_loop.create_proxy())
        // HACK: added as exclusive systems to ensure these run after input event systems
        .add_system_to_stage(
            CoreStage::First,
            handle_window_events.exclusive_system().at_end(),
        )
        .add_system_to_stage(
            CoreStage::First,
            handle_device_events.exclusive_system().at_end(),
        );

    let render_sub_app = app.remove_sub_app(RenderApp).unwrap();
    let render_app_channel = app
        .world
        .get_resource::<RenderAppChannel>()
        .unwrap()
        .clone();
    render_app_channel
        .update_sender
        .send(render_sub_app.app)
        .unwrap();
    std::thread::spawn(move || loop {
        app.update();
    });
    trace!("Entering winit event loop");

    let mut active = true;

    let event_handler = move |event: Event<()>,
                              event_loop: &EventLoopWindowTarget<()>,
                              control_flow: &mut ControlFlow| {
        *control_flow = ControlFlow::Poll;

        // if let Some(app_exit_events) = app.world.get_resource_mut::<Events<AppExit>>() {
        //     if app_exit_event_reader
        //         .iter(&app_exit_events)
        //         .next_back()
        //         .is_some()
        //     {
        //         *control_flow = ControlFlow::Exit;
        //     }
        // }

        match event {
            event::Event::WindowEvent {
                event, window_id, ..
            } => {
                if let Some(event) = event.to_static() {
                    window_event_channel
                        .sender
                        .send(WinitWindowEvent {
                            event,
                            id: window_id,
                        })
                        .unwrap()
                }
            }
            event::Event::DeviceEvent { event, device_id } => device_event_channel
                .sender
                .send(WinitDeviceEvent {
                    event,
                    id: device_id,
                })
                .unwrap(),
            event::Event::Suspended => {
                active = false;
            }
            event::Event::Resumed => {
                active = true;
            }
            event::Event::RedrawRequested(_) => {
                let mut render_app = render_app_channel.renderer_receiver.recv().unwrap();
                (render_sub_app.runner)(&mut render_app);
                render_app_channel.renderer_sender.send(render_app).unwrap();
            }
            event::Event::MainEventsCleared => {
                // handle_create_window_events(
                //     &mut app.world,
                //     event_loop,
                //     &mut create_window_event_reader,
                // );
            }
            _ => (),
        }
    };
    run(event_loop, event_handler);
}

fn handle_device_events(
    winit_device_event_channel: Res<WinitDeviceEventChannel>,
    mut mouse_motion_events: ResMut<Events<MouseMotion>>,
) {
    while let Ok(event) = winit_device_event_channel.receiver.try_recv() {
        match event.event {
            DeviceEvent::MouseMotion { delta } => {
                mouse_motion_events.send(MouseMotion {
                    delta: Vec2::new(delta.0 as f32, delta.1 as f32),
                });
            }
            _ => (),
        }
    }
}

fn handle_window_events(
    winit_windows: ResMut<WinitWindows>,
    mut windows: ResMut<Windows>,
    winit_window_event_channel: Res<WinitWindowEventChannel>,
    mut window_resized_events: ResMut<Events<WindowResized>>,
    mut window_close_requested_events: ResMut<Events<WindowCloseRequested>>,
    mut keyboard_input_events: ResMut<Events<KeyboardInput>>,
    mut cursor_moved_events: ResMut<Events<CursorMoved>>,
    mut cursor_entered_events: ResMut<Events<CursorEntered>>,
    mut cursor_left_events: ResMut<Events<CursorLeft>>,
    mut mouse_wheel_input_events: ResMut<Events<MouseWheel>>,
    mut mouse_button_input_events: ResMut<Events<MouseButtonInput>>,
    mut touch_input_events: ResMut<Events<TouchInput>>,
    mut received_character_events: ResMut<Events<ReceivedCharacter>>,
    mut window_focused_events: ResMut<Events<WindowFocused>>,
    mut window_moved_events: ResMut<Events<WindowMoved>>,
    mut file_drag_and_drop_events: ResMut<Events<FileDragAndDrop>>,
) {
    while let Ok(event) = winit_window_event_channel.receiver.try_recv() {
        let window_id = if let Some(window_id) = winit_windows.get_window_id(event.id) {
            window_id
        } else {
            warn!("Skipped event for unknown winit Window Id {:?}", event.id);
            return;
        };

        let window = if let Some(window) = windows.get_mut(window_id) {
            window
        } else {
            warn!("Skipped event for unknown Window Id {:?}", event.id);
            return;
        };
        match event.event {
            WindowEvent::Resized(size) => {
                window.update_actual_size_from_backend(size.width, size.height);
                window_resized_events.send(WindowResized {
                    id: window_id,
                    width: window.width(),
                    height: window.height(),
                });
            }
            WindowEvent::CloseRequested => {
                window_close_requested_events.send(WindowCloseRequested { id: window_id });
            }
            WindowEvent::KeyboardInput { ref input, .. } => {
                keyboard_input_events.send(converters::convert_keyboard_input(input));
            }
            WindowEvent::CursorMoved { position, .. } => {
                let winit_window = winit_windows.get_window(window_id).unwrap();
                let position = position.to_logical(winit_window.scale_factor());
                let inner_size = winit_window.inner_size();

                // move origin to bottom left
                let y_position = inner_size.height as f64 - position.y;
                let physical_position = DVec2::new(position.x, y_position);
                window.update_cursor_physical_position_from_backend(Some(physical_position));

                cursor_moved_events.send(CursorMoved {
                    id: window_id,
                    position: (physical_position / window.scale_factor()).as_vec2(),
                });
            }
            WindowEvent::CursorEntered { .. } => {
                cursor_entered_events.send(CursorEntered { id: window_id });
            }
            WindowEvent::CursorLeft { .. } => {
                window.update_cursor_physical_position_from_backend(None);
                cursor_left_events.send(CursorLeft { id: window_id });
            }
            WindowEvent::MouseInput { state, button, .. } => {
                mouse_button_input_events.send(MouseButtonInput {
                    button: converters::convert_mouse_button(button),
                    state: converters::convert_element_state(state),
                });
            }
            WindowEvent::MouseWheel { delta, .. } => match delta {
                event::MouseScrollDelta::LineDelta(x, y) => {
                    mouse_wheel_input_events.send(MouseWheel {
                        unit: MouseScrollUnit::Line,
                        x,
                        y,
                    });
                }
                event::MouseScrollDelta::PixelDelta(p) => {
                    mouse_wheel_input_events.send(MouseWheel {
                        unit: MouseScrollUnit::Pixel,
                        x: p.x as f32,
                        y: p.y as f32,
                    });
                }
            },
            WindowEvent::Touch(touch) => {
                let winit_window = winit_windows.get_window(window_id).unwrap();
                let mut location = touch.location.to_logical(winit_window.scale_factor());

                // On a mobile window, the start is from the top while on PC/Linux/OSX from
                // bottom
                if cfg!(target_os = "android") || cfg!(target_os = "ios") {
                    let window_height = windows.get_primary().unwrap().height();
                    location.y = window_height - location.y;
                }
                touch_input_events.send(converters::convert_touch_input(touch, location));
            }
            WindowEvent::ReceivedCharacter(c) => {
                received_character_events.send(ReceivedCharacter {
                    id: window_id,
                    char: c,
                })
            }
            // WindowEvent::ScaleFactorChanged {
            //     scale_factor,
            //     new_inner_size,
            // } => {
            //     let mut backend_scale_factor_change_events = world
            //         .get_resource_mut::<Events<WindowBackendScaleFactorChanged>>()
            //         .unwrap();
            //     backend_scale_factor_change_events.send(WindowBackendScaleFactorChanged {
            //         id: window_id,
            //         scale_factor,
            //     });
            //     let prior_factor = window.scale_factor();
            //     window.update_scale_factor_from_backend(scale_factor);
            //     let new_factor = window.scale_factor();
            //     if let Some(forced_factor) = window.scale_factor_override() {
            //         // If there is a scale factor override, then force that to be used
            //         // Otherwise, use the OS suggested size
            //         // We have already told the OS about our resize constraints, so
            //         // the new_inner_size should take those into account
            //         *new_inner_size = winit::dpi::LogicalSize::new(
            //             window.requested_width(),
            //             window.requested_height(),
            //         )
            //         .to_physical::<u32>(forced_factor);
            //     } else if approx::relative_ne!(new_factor, prior_factor) {
            //         let mut scale_factor_change_events = world
            //             .get_resource_mut::<Events<WindowScaleFactorChanged>>()
            //             .unwrap();

            //         scale_factor_change_events.send(WindowScaleFactorChanged {
            //             id: window_id,
            //             scale_factor,
            //         });
            //     }

            //     let new_logical_width = new_inner_size.width as f64 / new_factor;
            //     let new_logical_height = new_inner_size.height as f64 / new_factor;
            //     if approx::relative_ne!(window.width() as f64, new_logical_width)
            //         || approx::relative_ne!(window.height() as f64, new_logical_height)
            //     {
            //         let mut resize_events =
            //             world.get_resource_mut::<Events<WindowResized>>().unwrap();
            //         resize_events.send(WindowResized {
            //             id: window_id,
            //             width: new_logical_width as f32,
            //             height: new_logical_height as f32,
            //         });
            //     }
            //     window.update_actual_size_from_backend(
            //         new_inner_size.width,
            //         new_inner_size.height,
            //     );
            // }
            WindowEvent::Focused(focused) => {
                window.update_focused_status_from_backend(focused);
                window_focused_events.send(WindowFocused {
                    id: window_id,
                    focused,
                });
            }
            WindowEvent::DroppedFile(path_buf) => {
                file_drag_and_drop_events.send(FileDragAndDrop::DroppedFile {
                    id: window_id,
                    path_buf,
                });
            }
            WindowEvent::HoveredFile(path_buf) => {
                file_drag_and_drop_events.send(FileDragAndDrop::HoveredFile {
                    id: window_id,
                    path_buf,
                });
            }
            WindowEvent::HoveredFileCancelled => {
                file_drag_and_drop_events
                    .send(FileDragAndDrop::HoveredFileCancelled { id: window_id });
            }
            WindowEvent::Moved(position) => {
                let position = ivec2(position.x, position.y);
                window.update_actual_position_from_backend(position);
                window_moved_events.send(WindowMoved {
                    id: window_id,
                    position,
                });
            }
            _ => {}
        }
    }
}

fn request_window_redraws(winit_windows: Res<WinitWindows>) {
    for window in winit_windows.windows.values() {
        window.request_redraw();
    }
}

fn handle_create_window_events(
    world: &mut World,
    event_loop: &EventLoopWindowTarget<()>,
    create_window_event_reader: &mut ManualEventReader<CreateWindow>,
) {
    let world = world.cell();
    let mut winit_windows = world.get_resource_mut::<WinitWindows>().unwrap();
    let mut windows = world.get_resource_mut::<Windows>().unwrap();
    let create_window_events = world.get_resource::<Events<CreateWindow>>().unwrap();
    let mut window_created_events = world.get_resource_mut::<Events<WindowCreated>>().unwrap();
    for create_window_event in create_window_event_reader.iter(&create_window_events) {
        let window = winit_windows.create_window(
            event_loop,
            create_window_event.id,
            &create_window_event.descriptor,
        );
        windows.add(window);
        window_created_events.send(WindowCreated {
            id: create_window_event.id,
        });
    }
}

fn handle_initial_window_events(world: &mut World, event_loop: &EventLoop<()>) {
    let world = world.cell();
    let mut winit_windows = world.get_resource_mut::<WinitWindows>().unwrap();
    let mut windows = world.get_resource_mut::<Windows>().unwrap();
    let mut create_window_events = world.get_resource_mut::<Events<CreateWindow>>().unwrap();
    let mut window_created_events = world.get_resource_mut::<Events<WindowCreated>>().unwrap();
    for create_window_event in create_window_events.drain() {
        let window = winit_windows.create_window(
            event_loop,
            create_window_event.id,
            &create_window_event.descriptor,
        );
        windows.add(window);
        window_created_events.send(WindowCreated {
            id: create_window_event.id,
        });
    }
}

fn update_windows(world: &mut World) {
    let world = world.cell();
    let winit_windows = world.get_resource::<WinitWindows>().unwrap();
    let mut windows = world.get_resource_mut::<Windows>().unwrap();

    for bevy_window in windows.iter_mut() {
        let id = bevy_window.id();
        for command in bevy_window.drain_commands() {
            match command {
                bevy_window::WindowCommand::SetWindowMode {
                    mode,
                    resolution: (width, height),
                } => {
                    let window = winit_windows.get_window(id).unwrap();
                    match mode {
                        bevy_window::WindowMode::BorderlessFullscreen => {
                            window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)))
                        }
                        bevy_window::WindowMode::Fullscreen => {
                            window.set_fullscreen(Some(winit::window::Fullscreen::Exclusive(
                                get_best_videomode(&window.current_monitor().unwrap()),
                            )))
                        }
                        bevy_window::WindowMode::SizedFullscreen => window.set_fullscreen(Some(
                            winit::window::Fullscreen::Exclusive(get_fitting_videomode(
                                &window.current_monitor().unwrap(),
                                width,
                                height,
                            )),
                        )),
                        bevy_window::WindowMode::Windowed => window.set_fullscreen(None),
                    }
                }
                bevy_window::WindowCommand::SetTitle { title } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_title(&title);
                }
                bevy_window::WindowCommand::SetScaleFactor { scale_factor } => {
                    let mut window_dpi_changed_events = world
                        .get_resource_mut::<Events<WindowScaleFactorChanged>>()
                        .unwrap();
                    window_dpi_changed_events.send(WindowScaleFactorChanged { id, scale_factor });
                }
                bevy_window::WindowCommand::SetResolution {
                    logical_resolution: (width, height),
                    scale_factor,
                } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_inner_size(
                        winit::dpi::LogicalSize::new(width, height)
                            .to_physical::<f64>(scale_factor),
                    );
                }
                bevy_window::WindowCommand::SetVsync { .. } => (),
                bevy_window::WindowCommand::SetResizable { resizable } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_resizable(resizable);
                }
                bevy_window::WindowCommand::SetDecorations { decorations } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_decorations(decorations);
                }
                bevy_window::WindowCommand::SetCursorLockMode { locked } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window
                        .set_cursor_grab(locked)
                        .unwrap_or_else(|e| error!("Unable to un/grab cursor: {}", e));
                }
                bevy_window::WindowCommand::SetCursorVisibility { visible } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_cursor_visible(visible);
                }
                bevy_window::WindowCommand::SetCursorPosition { position } => {
                    let window = winit_windows.get_window(id).unwrap();
                    let inner_size = window.inner_size().to_logical::<f32>(window.scale_factor());
                    window
                        .set_cursor_position(winit::dpi::LogicalPosition::new(
                            position.x,
                            inner_size.height - position.y,
                        ))
                        .unwrap_or_else(|e| error!("Unable to set cursor position: {}", e));
                }
                bevy_window::WindowCommand::SetMaximized { maximized } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_maximized(maximized)
                }
                bevy_window::WindowCommand::SetMinimized { minimized } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_minimized(minimized)
                }
                bevy_window::WindowCommand::SetPosition { position } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_outer_position(PhysicalPosition {
                        x: position[0],
                        y: position[1],
                    });
                }
                bevy_window::WindowCommand::SetResizeConstraints { resize_constraints } => {
                    let window = winit_windows.get_window(id).unwrap();
                    let constraints = resize_constraints.check_constraints();
                    let min_inner_size = LogicalSize {
                        width: constraints.min_width,
                        height: constraints.min_height,
                    };
                    let max_inner_size = LogicalSize {
                        width: constraints.max_width,
                        height: constraints.max_height,
                    };

                    window.set_min_inner_size(Some(min_inner_size));
                    if constraints.max_width.is_finite() && constraints.max_height.is_finite() {
                        window.set_max_inner_size(Some(max_inner_size));
                    }
                }
            }
        }
    }
}
