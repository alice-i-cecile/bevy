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

    fn apply_velocity(mut query: Query<(&mut Transform, &Velocity)>, time: Res<Time>) {
        for (mut transform, velocity) in query.iter_mut() {
            transform.translation += velocity.0 * time.delta_seconds();
        }
    }

    fn jump(mut query: Query<&mut Velocity, With<Player>>, keyboard_input: Res<Input<KeyCode>>) {
        if keyboard_input.just_pressed(KeyCode::Space) {
            let mut player_velocity = query.single_mut();
            player_velocity.0.y += 10.0;
        }
    }

    fn gravity(mut query: Query<(&mut Velocity, &Transform)>, time: Res<Time>) {
        for (mut velocity, transform) in query.iter_mut() {
            if transform.translation.y >= 0.0 {
                velocity.0.y -= 1.0 * time.delta_seconds();
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

/// This plugin runs constantly in our tests,
/// and verifies that none of our internal rules have been broken.
///
/// We can also add it to our game during development in order to proactively catch issues,
/// at the risk of sudden crashes and a small performance overhead.
///
/// We could also handle failure more gracefully, by returning a `Result` from our systems
/// and using system chaining to log and then respond to the violation.
struct InvariantsPlugin;

impl Plugin for InvariantsPlugin {
    fn build(&self, app: &mut App) {
        // Generally, assertions about invariants should be checked
        // at the end or beginning of the frame, where we are "guaranteed" to have a clean state.
        app.add_system_to_stage(CoreStage::Last, assert_player_does_not_fall_through_floor);
    }
}

fn assert_player_does_not_fall_through_floor(query: Query<&Transform, With<Player>>) {
    // Note that query.single() also enforces an invariant: there is always exactly one Player
    let player_transform = query.single();
    assert!(player_transform.translation.y >= 0.0);
}

/// A convenience method to reduce code duplication in tests
fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugin(PhysicsPlugin)
        .add_plugin(InputPlugin)
        // By adding this invariant-checking plugin to our test setup,
        // we can automatically check for common or complex failure modes,
        // without having to predict exactly when they might occur
        .add_plugin(InvariantsPlugin);
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
    let player_transform = player_query.iter(&app.world).next().unwrap();

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
    use bevy::input::keyboard::KeyboardInput;
    use bevy::input::ElementState;

    let mut app = test_app();

    // Spawn everything in
    app.update();

    // Send a fake keyboard press

    // WARNING: inputs sent via pressing / releasing an Input<T> resource
    // are never just-pressed or just-released.
    // Track this bug at: https://github.com/bevyengine/bevy/issues/3847
    app.world.events().send(KeyboardInput {
        scan_code: 44,
        key_code: Some(KeyCode::Space),
        state: ElementState::Pressed,
    });

    // Process the keyboard press
    app.update();

    // Verify that the input is pressed
    let keyboard_input: &Input<KeyCode> = app.world.get_resource().unwrap();
    assert!(keyboard_input.pressed(KeyCode::Space));
    assert!(keyboard_input.just_pressed(KeyCode::Space));

    // Check that the player has upwards velocity due to jumping
    let mut player_query = app
        .world
        .query_filtered::<(&Velocity, &Transform), With<Player>>();
    let (player_velocity, player_transform) = player_query.iter(&app.world).next().unwrap();

    // Check that the player has moved upwards due to jumping
    assert!(player_velocity.0.y > 0.0);
    assert!(player_transform.translation.y > 0.0);
}
