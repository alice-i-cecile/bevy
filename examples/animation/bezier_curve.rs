//! Demonstrates how to work with beizer curves.
//!

use bevy::{math::cubic_splines::CubicCurve, prelude::*};

#[derive(Component)]
pub struct CubeCurve(CubicCurve<Vec3>);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, animate_cube)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Define your control points
    // These point will build a curve looking like
    //
    // y
    // │     .``.
    // │    .-``-.
    // │ .-`      `-.
    // ●─────────────── x
    //
    // (Do not comment my ascii art graph skills)
    let control_point1 = Vec3::new(-6., 2., 0.);
    let control_point2 = Vec3::new(12., 8., 0.);

    let control_point3 = Vec3::new(-12., 8., 0.);
    let control_point4 = Vec3::new(6., 2., 0.);

    let points = [[
        control_point1,
        control_point2,
        control_point3,
        control_point4,
    ]];

    // Make a CubicCurve
    let bezier = Bezier::new(points).to_curve();

    // Spawning a cube to experiment on
    commands.spawn((
        PbrBundle {
            mesh: meshes.add(shape::Cube::default().into()),
            material: materials.add(Color::ORANGE.into()),
            transform: Transform::from_translation(control_point1),
            ..default()
        },
        CubeCurve(bezier),
    ));

    // Some light to see something
    commands.spawn(PointLightBundle {
        point_light: PointLight {
            intensity: 9000.,
            range: 100.,
            shadows_enabled: true,
            ..default()
        },
        transform: Transform::from_xyz(8., 16., 8.),
        ..default()
    });

    // ground plane
    commands.spawn(PbrBundle {
        mesh: meshes.add(shape::Plane::from_size(50.).into()),
        material: materials.add(Color::SILVER.into()),
        ..default()
    });

    // The camera
    commands.spawn(Camera3dBundle {
        transform: Transform::from_xyz(0., 6., 12.).looking_at(Vec3::new(0., 3., 0.), Vec3::Y),
        ..default()
    });
}

pub fn animate_cube(time: Res<Time>, mut query: Query<(&mut Transform, &CubeCurve)>) {
    let step = (time.elapsed_seconds().sin() + 1.) / 2.;

    for (mut transform, cube_curve) in &mut query {
        // position takes a point from the curve where 0 is the initial point
        // and 1 is the last point
        transform.translation = cube_curve.0.position(step);
    }
}
