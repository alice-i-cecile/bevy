//! Demonstrates how to make turn-based games in Bevy,
//! using an extremely simple tic-tac-toe game

use std::cell::Cell;

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_startup_system(setup)
        .run();
}

#[derive(Component)]
enum CellContents {
    Empty,
    X,
    O,
}

#[derive(Component)]
struct Position {
    x: u8,
    y: u8,
}

/// Draw the grid as UI elements
fn setup(mut commands: Commands) {
    // UI camera
    commands.spawn_bundle(Camera2dBundle::default());

    // Create a root UI node
    let root = commands
        .spawn_bundle(NodeBundle {
            style: Style {
                size: Size::new(Val::Percent(100.0), Val::Percent(100.0)),
                ..default()
            },
            ..default()
        })
        .id();

    for x in 0..3 {
        // Create a node for each column
        let column = commands
            .spawn_bundle(NodeBundle {
                style: Style {
                    flex_direction: FlexDirection::Column,
                    size: Size::new(Val::Percent(100.0), Val::Percent(100.0)),
                    ..default()
                },
                ..default()
            })
            .id();

        commands.entity(root).add_child(column);

        for y in 0..3 {
            // Create three cells in each column
            let cell = commands
                .spawn_bundle(NodeBundle {
                    color: Color::WHITE.into(),
                    style: Style {
                        size: Size::new(Val::Percent(100.0), Val::Percent(100.0)),
                        ..default()
                    },
                    ..default()
                })
                // Track where each cell is spawned
                .insert(Position { x, y })
                // Store what's in each cell
                .insert(CellContents::Empty)
                .id();

            commands.entity(column).add_child(cell);
        }
    }
}
