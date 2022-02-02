//! Integration testing Bevy apps is surprisingly easy,
//! and is a great tool for ironing out tricky bugs or enabling refactors.
//!
//! Create new files in your root `tests` directory, and then call `cargo test` to ensure that they pass.
//!
//! You can easily reuse functionality between your tests and game by organizing your logic with plugins,
//! and then use direct methods on `App` / `World` to set up test scenarios.
//!
//! There are many helpful assertion methods on [`App`] that correspond to methods on [`World`];
//! browse the docs to discover more!

use bevy::{input::InputPlugin, prelude::*};
use game::{HighestJump, PhysicsPlugin, Player, Velocity};

// This module represents the code defined in your `src` folder, and exported from your project
mod game {
    use bevy::prelude::*;

    pub struct PhysicsPlugin;

    #[derive(SystemLabel, Clone, Debug, PartialEq, Eq, Hash)]
    enum PhysicsLabels {
        PlayerControl,
        Gravity,
        Velocity,
    }

    impl Plugin for PhysicsPlugin {
        fn build(&self, app: &mut App) {
            use PhysicsLabels::*;

            app.add_startup_system(spawn_player)
                .init_resource::<HighestJump>()
                .add_system(jump.label(PlayerControl))
                .add_system(gravity.label(Gravity).after(PlayerControl))
                .add_system(apply_velocity.label(Velocity).after(Gravity))
                .add_system_to_stage(CoreStage::PostUpdate, clamp_position)
                .add_system_to_stage(CoreStage::PreUpdate, update_highest_jump);
        }
    }

    #[derive(Debug, PartialEq, Default)]
    pub struct HighestJump(pub f32);

    #[derive(Component)]
    pub struct Player;

    #[derive(Component, Default)]
    pub struct Velocity(pub Vec3);

    // These systems don't need to be `pub`, as they're hidden within your plugin
    fn spawn_player(mut commands: Commands) {
        commands
            .spawn()
            .insert(Player)
            .insert(Transform::default())
            .insert(Velocity::default());
    }

    fn apply_velocity(mut query: Query<(&mut Transform, &Velocity)>) {
        for (mut transform, velocity) in query.iter_mut() {
            transform.translation += velocity.0;
        }
    }

    fn jump(mut query: Query<&mut Velocity, With<Player>>, keyboard_input: Res<Input<KeyCode>>) {
        if keyboard_input.just_pressed(KeyCode::Space) {
            let mut player_velocity = query.single_mut();
            player_velocity.0.y += 10.0;
        }
    }

    fn gravity(mut query: Query<(&mut Velocity, &Transform)>) {
        for (mut velocity, transform) in query.iter_mut() {
            if transform.translation.y >= 0.0 {
                velocity.0.y -= 1.0;
            }
        }
    }

    /// Players should not fall through the floor
    fn clamp_position(mut query: Query<(&mut Velocity, &mut Transform)>) {
        for (mut velocity, mut transform) in query.iter_mut() {
            if transform.translation.y <= 0.0 {
                velocity.0.y = 0.0;
                transform.translation.y = 0.0;
            }
        }
    }

    fn update_highest_jump(
        query: Query<&Transform, With<Player>>,
        mut highest_jump: ResMut<HighestJump>,
    ) {
        let player_transform = query.single();
        if player_transform.translation.y > highest_jump.0 {
            highest_jump.0 = player_transform.translation.y;
        }
    }
}

/// A convenience method to reduce code duplication in tests
fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugin(PhysicsPlugin)
        .add_plugin(InputPlugin);
    // It is generally unwise to run the initial update in convenience methods like this
    // as startup systems added by later plugins will be missed
    app
}

#[test]
fn player_falls() {
    let mut app = test_app();

    // Allowing the game to initialize,
    // running all systems in the schedule once
    app.update();

    // Moving the player up
    let mut player_query = app.world.query_filtered::<&mut Transform, With<Player>>();
    let mut player_transform = player_query.iter_mut(&mut app.world).next().unwrap();
    player_transform.translation.y = 3.0;

    // Running the app again
    // This should cause gravity to take effect and make the player fall
    app.update();

    let mut player_query = app.world.query_filtered::<&Transform, With<Player>>();
    let player_transform = player_query.iter(&mut app.world).next().unwrap();

    // When possible, try to make assertions about behavior, rather than detailed outcomes
    // This will help make your tests robust to irrelevant changes
    assert!(player_transform.translation.y < 3.0);
    assert_eq!(app.world.get_resource(), Some(&HighestJump(3.0)));
}

#[test]
fn player_does_not_fall_through_floor() {
    // From the `player_falls` test, we know that gravity is working
    let mut app = test_app();

    // The player should start on the floor
    app.update();
    app.assert_component_eq::<Transform, With<Player>>(&Transform::from_xyz(0.0, 0.0, 0.0));

    // Even after some time, the player should not fall through the floor
    for _ in 0..3 {
        app.update();
    }

    app.assert_component_eq::<Transform, With<Player>>(&Transform::from_xyz(0.0, 0.0, 0.0));

    // If we drop the player from a height, they should eventually come to rest on the floor
    let mut player_query = app.world.query_filtered::<&mut Transform, With<Player>>();
    let mut player_transform = player_query.iter_mut(&mut app.world).next().unwrap();
    player_transform.translation.y = 10.0;

    // A while later...
    for _ in 0..10 {
        app.update();
    }

    // The player should have landed by now
    app.assert_component_eq::<Transform, With<Player>>(&Transform::from_xyz(0.0, 0.0, 0.0));
}

#[test]
fn jumping_moves_player_upwards() {
    let mut app = test_app();

    // Spawn everything in
    app.update();

    // Send a fake keyboard press
    let mut keyboard_input: Mut<Input<KeyCode>> = app.world.get_resource_mut().unwrap();
    keyboard_input.press(KeyCode::Space);

    // Process the keyboard press
    app.update();

    // Check that the player has upwards velocity due to jumping
    let mut player_query = app
        .world
        .query_filtered::<(&Velocity, &Transform), With<Player>>();
    let (player_velocity, player_transform) = player_query.iter(&app.world).next().unwrap();

    // Check that the player has moved upwards due to jumping
    assert!(player_velocity.0.y > 0.0);
    assert!(player_transform.translation.y > 0.0);
}
