//! This example shows off reconciliation using the `bevy_feathers` widgets.
//!
//! It also serves as a test suite to see how the widget states behave when the scene is reconciled.
//!
//! Run this with subsecond hot patching to enable bsn!-macro hot reloading:
//! `BEVY_ASSET_ROOT="." dx serve --hot-patch --example bsn_reconcile --features=hotpatching`
//!

use bevy::{
    color::palettes::tailwind,
    feathers::{
        controls::{
            button, checkbox, color_slider, color_swatch, radio, slider, toggle_switch,
            ButtonProps, ColorChannel, ColorSliderProps, SliderProps,
        },
        dark_theme::create_dark_theme,
        theme::{ThemeBackgroundColor, ThemedText, UiTheme},
        tokens, FeathersPlugins,
    },
    input_focus::tab_navigation::TabGroup,
    prelude::*,
    scene2::prelude::{Scene, *},
    ui::Checked,
    ui_widgets::{
        checkbox_self_update, slider_self_update, Activate, RadioButton, RadioGroup,
        SliderPrecision, SliderStep, SliderValue, ValueChange,
    },
};

/// A struct to hold the state of various widgets shown in the demo.
#[derive(Resource)]
struct DemoWidgetStates {
    controlled_slider_value: f32,
    hsl_color: Hsla,
}

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, FeathersPlugins))
        .insert_resource(UiTheme(create_dark_theme()))
        .insert_resource(DemoWidgetStates {
            controlled_slider_value: 20.0,
            hsl_color: tailwind::AMBER_800.into(),
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
                    button(ButtonProps::default())
                    on(|_: On<Activate>| {
                        info!("Button clicked!");
                    })
                    [(Text("Click me!") ThemedText)]
                ),
                (
                    checkbox()
                    on(checkbox_self_update)
                    [(Text("Checkbox") ThemedText)]
                ),
                (
                    Node {
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                    }
                    RadioGroup
                    // Update radio button states based on notification from radio group.
                    on(
                        |value_change: On<ValueChange<Entity>>,
                         q_radio: Query<Entity, With<RadioButton>>,
                         mut commands: Commands| {
                            for radio in q_radio.iter() {
                                if radio == value_change.value {
                                    commands.entity(radio).insert(Checked);
                                } else {
                                    commands.entity(radio).remove::<Checked>();
                                }
                            }
                        }
                    )
                    [
                        radio() [ (Text("One") ThemedText) ],
                        radio() [ (Text("Two") ThemedText) ],
                    ]
                ),
                (toggle_switch() on(checkbox_self_update)),
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                } [
                    // Uncontrolled/self-updating slider (the slider widget owns the state)
                    (
                        slider(SliderProps {
                            max: 1.0,
                            ..default()
                        })
                        on(slider_self_update)
                        SliderStep(0.1)
                        SliderPrecision(3)
                    ),
                    // Controlled slider (the caller owns the state)
                    (
                        slider(SliderProps {
                            max: 100.0,
                            ..default()
                        })
                        on(|change: On<ValueChange<f32>>, mut state: ResMut<DemoWidgetStates>| {
                            state.controlled_slider_value = change.value;
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
                        color_slider(
                            ColorSliderProps {
                                channel: ColorChannel::HslHue,
                                ..default()
                            }
                        )
                        on(|change: On<ValueChange<f32>>, mut color: ResMut<DemoWidgetStates>| {
                            color.hsl_color.hue = change.value;
                        })
                        SliderValue({hsl_color.hue})
                    ),
                ]
            ],

            :todos
        ]
    }
}

fn todos() -> impl Scene {
    bsn! {
        Node::default() [
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(8.0)),
            } [
                Text("Today"),
                #A :todo_item("Write BSN"),
                #B :todo_item("Hot reload it!") DependsOn(#A),
                #C :todo_item("Add checkboxes"),
                #D :todo_item("Try some styling"),
                #E :todo_item("Move things around"),
            ],

            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(8.0)),
            } [
                Text("Tomorrow"),
            ]
        ]
    }
}

#[derive(Component, GetTemplate)]
struct DependsOn(pub Entity);

fn todo_item(title: &'static str) -> impl Scene {
    bsn! {
        :checkbox
        on(checkbox_self_update)
        Node {
            column_gap: Val::Px(10.0),
            padding: UiRect::all(Val::Px(6.0)),
            border: UiRect::all(Val::Px(1.0)),
            align_items: AlignItems::Center,
        }
        BorderColor::all(tailwind::NEUTRAL_700)
        BorderRadius::all(Val::Px(5.0))
        BackgroundColor(tailwind::NEUTRAL_800)
        [
            Text(title)
            TextColor(tailwind::NEUTRAL_100) TextFont { font_size: 16.0 }
        ]
        on(move |add: On<Insert, DependsOn>, query: Query<&DependsOn>, mut previous: Local<Option<Entity>>| {
            if let Ok(depends_on) = query.get(add.entity)
                && (previous.is_none() || previous.unwrap() != depends_on.0)
            {
                *previous = Some(depends_on.0);
                info!("'{title}' ({:?}) depends on {:?}", add.entity, depends_on.0);
            }
        })
    }
}
