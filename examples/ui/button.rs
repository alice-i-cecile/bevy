//! This example illustrates how to create a button that changes color and text based on its
//! interaction state.

use bevy::{color::palettes::basic::*, prelude::*, winit::WinitSettings};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // Only run the app when there is user input. This will significantly reduce CPU/GPU use.
        .insert_resource(WinitSettings::desktop_app())
        .add_systems(Startup, setup)
        .add_systems(Update, button_system)
        .run();
}

const NORMAL_BUTTON: Color = Color::srgb(0.15, 0.15, 0.15);
const HOVERED_BUTTON: Color = Color::srgb(0.25, 0.25, 0.25);
const PRESSED_BUTTON: Color = Color::srgb(0.35, 0.75, 0.35);

fn button_system(
    mut button_query: Query<
        (&Button, &mut BackgroundColor, &mut BorderColor, &Children),
        Changed<Button>,
    >,
    mut text_query: Query<&mut Text>,
) {
    for (button, mut color, mut border_color, children) in &mut button_query {
        let new_str = match (button.pressed, button.hovered) {
            (true, _) => {
                *color = PRESSED_BUTTON.into();
                border_color.0 = RED.into();
                "Press"
            }
            (false, true) => {
                *color = HOVERED_BUTTON.into();
                "Hover"
            }
            (false, false) => {
                *color = NORMAL_BUTTON.into();
                "Button"
            }
        };

        if let Ok(mut text) = text_query.get_mut(children[0]) {
            text.sections[0].value = new_str.to_string();
        }
    }
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    // ui camera
    commands.spawn(Camera2dBundle::default());
    commands
        .spawn(NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            ..default()
        })
        .with_children(|parent| {
            parent
                .spawn(ButtonBundle {
                    style: Style {
                        width: Val::Px(150.0),
                        height: Val::Px(65.0),
                        border: UiRect::all(Val::Px(5.0)),
                        // horizontally center child text
                        justify_content: JustifyContent::Center,
                        // vertically center child text
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    border_color: BorderColor(Color::BLACK),
                    border_radius: BorderRadius::MAX,
                    background_color: NORMAL_BUTTON.into(),
                    ..default()
                })
                .with_child(TextBundle::from_section(
                    "Button",
                    TextStyle {
                        font: asset_server.load("fonts/FiraSans-Bold.ttf"),
                        font_size: 33.0,
                        color: Color::srgb(0.9, 0.9, 0.9),
                    },
                ));
        });
}
