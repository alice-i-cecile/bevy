//! Demonstrates how to make turn-based games in Bevy,
//! using a tic-tac-toe game as the base.
//!
//! The thing to pay attention to here is the use of an exclusive system that runs a schedule.

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_startup_system(ui::setup)
        // Runs all of our logic in a sub-schedule
        // which is scheduled at the end of CoreStage::Update by default
        .add_system(logic::run_schedule.exclusive_system())
        .run();
}

mod logic {
    #[derive(StageLabel)]
    enum TurnBasedStage {
        Picking,
        Checking,
    }

    use bevy::prelude::*;

    /// Resource that stores all of our game logic
    #[derive(Deref, DerefMut)]
    struct TurnBasedSchedule(Schedule);

    impl Default for TurnBasedSchedule {
        fn default() -> Self {
            let update_stage = SystemStage::parallel();
            // Add systems here to update_stage

            let mut schedule = Schedule::default();
            schedule.add_stage(TurnBasedStage::Picking, update_stage);

            TurnBasedSchedule(schedule)
        }
    }

    pub fn run_schedule(world: &mut World) {
        // Split apart the SimSchedule and the rest of the world
        world.resource_scope(|world, mut sim_schedule: Mut<TurnBasedSchedule>| {
            sim_schedule.run_once(world);
        });
    }
}

mod ui {
    use bevy::prelude::*;

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
    pub fn setup(mut commands: Commands) {
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
}
