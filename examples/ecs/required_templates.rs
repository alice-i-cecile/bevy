//! Shows how `#[require]` accepts `bsn!` syntax, so required components can be built from templates that
//! load assets and read from the [`World`]. They are built before the requiring component is inserted, so
//! hooks and observers always see their final values.

use bevy::{
    ecs::template::{template, TemplateContext},
    prelude::*,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_observer(announce_ring)
        .add_systems(Startup, setup)
        .add_systems(Update, spin)
        .run();
}

#[derive(Component)]
#[require(
    // Loads the mesh with the `AssetServer`, just like `Mesh3d("...")` does in `bsn!`.
    Mesh3d("models/torus/torus.gltf#Mesh0/Primitive0"),
    // Reuses a material handle stored in a resource.
    ~{template(ring_material)},
    // Reads the `Ring` that is being inserted.
    ~{template(ring_name)},
)]
struct Ring {
    points: u32,
}

#[derive(Resource)]
struct RingMaterial(Handle<StandardMaterial>);

fn ring_material(context: &mut TemplateContext) -> Result<MeshMaterial3d<StandardMaterial>> {
    Ok(MeshMaterial3d(context.resource::<RingMaterial>().0.clone()))
}

fn ring_name(context: &mut TemplateContext) -> Result<Name> {
    let points = context.inserting::<Ring>().map_or(0, |ring| ring.points);
    Ok(Name::new(format!("Ring worth {points} points")))
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(RingMaterial(materials.add(Color::srgb(1.0, 0.8, 0.2))));

    // No mesh, material or name is specified here: they are all required by `Ring`.
    for i in 0..5 {
        commands.spawn((
            Ring {
                points: (i + 1) * 10,
            },
            Transform::from_xyz(i as f32 * 3.0 - 6.0, 0.0, 0.0),
        ));
    }

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 4.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(3.0, 8.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn announce_ring(add: On<Add<Ring>>, rings: Query<(&Name, &Mesh3d)>) {
    let (name, mesh) = rings.get(add.entity).unwrap();
    info!("{name} was added with mesh {:?}", mesh.0.path());
}

fn spin(time: Res<Time>, mut rings: Query<&mut Transform, With<Ring>>) {
    for mut transform in &mut rings {
        transform.rotate_x(time.delta_secs());
    }
}
