use bevy_ecs::prelude::*;
use criterion::{BatchSize, Criterion};
use glam::*;

#[derive(Component, Default, Clone)]
#[require(Position, Velocity)]
struct Player;

#[derive(Component, Default, Clone)]
struct Position(Vec3);

#[derive(Component, Default, Clone)]
struct Velocity(Vec3);

const ENTITIES: usize = 1_000;

pub fn required_default(c: &mut Criterion) {
    let mut group = c.benchmark_group("required_default");
    group.warm_up_time(core::time::Duration::from_millis(500));
    group.measurement_time(core::time::Duration::from_secs(4));
    group.bench_function("spawn", |b| {
        b.iter_batched_ref(
            World::new,
            |world| {
                for _ in 0..ENTITIES {
                    world.spawn(Player);
                }
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("insert", |b| {
        b.iter_batched_ref(
            || {
                let mut world = World::new();
                let entities = (0..ENTITIES)
                    .map(|_| world.spawn_empty().id())
                    .collect::<Vec<_>>();
                (world, entities)
            },
            |(world, entities)| {
                for &entity in entities.iter() {
                    world.entity_mut(entity).insert(Player);
                }
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("spawn_batch", |b| {
        b.iter_batched_ref(
            World::new,
            |world| {
                world.spawn_batch((0..ENTITIES).map(|_| Player));
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("insert_batch", |b| {
        b.iter_batched_ref(
            || {
                let mut world = World::new();
                let entities = (0..ENTITIES)
                    .map(|_| world.spawn_empty().id())
                    .collect::<Vec<_>>();
                (world, entities)
            },
            |(world, entities)| {
                world.insert_batch(entities.iter().map(|&entity| (entity, Player)));
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("commands_spawn", |b| {
        b.iter_batched_ref(
            World::new,
            |world| {
                let mut commands = world.commands();
                for _ in 0..ENTITIES {
                    commands.spawn(Player);
                }
                world.flush();
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}
