//! This example shows off reconciliation using the `bevy_feathers` widgets.
//!
//! It also serves as a test suite to see how the widget states behave when the scene is reconciled.
//!
//! Run this with subsecond hot patching to enable bsn!-macro hot reloading:
//! `BEVY_ASSET_ROOT="." dx serve --hot-patch --example bsn_reconcile --features=hotpatching`
//!

use bevy::{
    color::palettes,
    core_widgets::{
        callback, Activate, CoreRadio, CoreRadioGroup, CoreWidgetsPlugins, SliderPrecision,
        SliderStep, SliderValue, ValueChange,
    },
    feathers::{
        controls::{
            button, checkbox, color_slider, color_swatch, radio, slider, toggle_switch,
            ButtonProps, CheckboxProps, ColorChannel, ColorSliderProps, SliderProps,
            ToggleSwitchProps,
        },
        dark_theme::create_dark_theme,
        theme::{ThemeBackgroundColor, ThemedText, UiTheme},
        tokens, FeathersPlugin,
    },
    input_focus::{
        tab_navigation::{TabGroup, TabNavigationPlugin},
        InputDispatchPlugin,
    },
    prelude::*,
    scene2::prelude::{Scene, *},
    ui::Checked,
};

/// A struct to hold the state of various widgets shown in the demo.
#[derive(Resource)]
struct DemoWidgetStates {
    controlled_slider_value: f32,
    hsl_color: Hsla,
}

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            CoreWidgetsPlugins,
            InputDispatchPlugin,
            TabNavigationPlugin,
            FeathersPlugin,
        ))
        .insert_resource(UiTheme(create_dark_theme()))
        .insert_resource(DemoWidgetStates {
            controlled_slider_value: 20.0,
            hsl_color: palettes::tailwind::AMBER_800.into(),
        })
        .add_systems(Startup, setup)
        .add_systems(Update, reconcile_ui)
        .run();
}

#[derive(Component)]
struct UiRoot;

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn(UiRoot);
}

fn reconcile_ui(
    mut commands: Commands,
    ui_root_query: Single<Entity, With<UiRoot>>,
    state: Res<DemoWidgetStates>,
) {
    // Reconcile the UI on every frame
    commands
        .entity(ui_root_query.entity())
        .reconcile_scene(demo_root(&state));
}

fn demo_root(state: &DemoWidgetStates) -> impl Scene {
    let DemoWidgetStates {
        controlled_slider_value,
        hsl_color,
    } = *state;

    bsn! {
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Start,
            justify_content: JustifyContent::Start,
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(10.0),
        }
        TabGroup
        ThemeBackgroundColor(tokens::WINDOW_BG)
        [
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                justify_content: JustifyContent::Start,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(8.0),
                width: Val::Percent(30.),
                min_width: Val::Px(200.),
            } [
                (
                    :button(ButtonProps {
                        on_click: callback(|_: In<Activate>| {
                            info!("Button clicked!");
                        }),
                        ..default()
                    }) [(Text("Click me!") ThemedText)]
                ),
                (
                    :checkbox(CheckboxProps::default())
                    [(Text("Checkbox") ThemedText)]
                ),
                (
                    Node {
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                    }
                    CoreRadioGroup {
                        // Update radio button states based on notification from radio group.
                        on_change: callback(
                            |ent: In<Activate>, q_radio: Query<Entity, With<CoreRadio>>, mut commands: Commands| {
                                for radio in q_radio.iter() {
                                    if radio == ent.0.0 {
                                        commands.entity(radio).insert(Checked);
                                    } else {
                                        commands.entity(radio).remove::<Checked>();
                                    }
                                }
                            },
                        ),
                    }
                    [
                        :radio [(Text("One") ThemedText)],
                        :radio [(Text("Two") ThemedText)],
                    ]
                ),
                :toggle_switch(ToggleSwitchProps::default()),
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                } [
                    // Uncontrolled slider (the slider widget owns the state)
                    (
                        :slider(SliderProps {
                            max: 1.0,
                            ..default()
                        })
                        SliderStep(0.1)
                        SliderPrecision(3)
                    ),
                    // Controlled slider (the caller owns the state)
                    (
                        :slider(SliderProps {
                            max: 100.0,
                            on_change: callback(|change: In<ValueChange<f32>>, mut state: ResMut<DemoWidgetStates>| {
                                state.controlled_slider_value = change.value;
                            }),
                            ..default()
                        })
                        SliderValue(controlled_slider_value)
                        SliderStep(10.)
                        SliderPrecision(2)
                    ),
                    (
                        Node {
                            justify_content: JustifyContent::SpaceBetween,
                        } [
                            Text("Hsl"),
                            (color_swatch() BackgroundColor(hsl_color)),
                        ]
                    ),
                    // Controlled color slider
                    (
                        :color_slider(
                            ColorSliderProps {
                                on_change: callback(
                                    |change: In<ValueChange<f32>>, mut color: ResMut<DemoWidgetStates>| {
                                        color.hsl_color.hue = change.value;
                                    },
                                ),
                                channel: ColorChannel::HslHue
                            }
                        )
                        SliderValue({hsl_color.hue})
                    ),
                ]
            ],
        ]
    }
}
