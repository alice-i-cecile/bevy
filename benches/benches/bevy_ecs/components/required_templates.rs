use bevy_ecs::{
    lifecycle::HookContext,
    prelude::*,
    template::{template, TemplateContext},
    world::DeferredWorld,
};
use criterion::{BatchSize, Criterion};

#[derive(Resource)]
struct Frame(u32);

#[derive(Component, Default)]
struct StartFrame(u32);

fn start_frame(context: &mut TemplateContext) -> Result<StartFrame> {
    Ok(StartFrame(context.resource::<Frame>().0))
}

#[derive(Component)]
#[require(~{template(start_frame)})]
struct WithTemplate;

#[derive(Component)]
#[require(StartFrame)]
#[component(on_add = overwrite_start_frame)]
struct WithHook;

fn overwrite_start_frame(mut world: DeferredWorld, context: HookContext) {
    let frame = world.resource::<Frame>().0;
    world.get_mut::<StartFrame>(context.entity).unwrap().0 = frame;
}

#[derive(Component)]
struct Explicit;

const ENTITIES: usize = 1_000;

fn world() -> World {
    let mut world = World::new();
    world.insert_resource(Frame(5));
    world
}

pub fn required_template(c: &mut Criterion) {
    let mut group = c.benchmark_group("required_template");
    group.warm_up_time(core::time::Duration::from_millis(500));
    group.measurement_time(core::time::Duration::from_secs(4));
    group.bench_function("spawn_template", |b| {
        b.iter_batched_ref(
            world,
            |world| {
                for _ in 0..ENTITIES {
                    world.spawn(WithTemplate);
                }
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("spawn_hook_workaround", |b| {
        b.iter_batched_ref(
            world,
            |world| {
                for _ in 0..ENTITIES {
                    world.spawn(WithHook);
                }
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("spawn_explicit", |b| {
        b.iter_batched_ref(
            world,
            |world| {
                for _ in 0..ENTITIES {
                    let frame = world.resource::<Frame>().0;
                    world.spawn((Explicit, StartFrame(frame)));
                }
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("insert_template", |b| {
        b.iter_batched_ref(
            || {
                let mut world = world();
                let entities = (0..ENTITIES)
                    .map(|_| world.spawn_empty().id())
                    .collect::<Vec<_>>();
                (world, entities)
            },
            |(world, entities)| {
                for &entity in entities.iter() {
                    world.entity_mut(entity).insert(WithTemplate);
                }
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("insert_hook_workaround", |b| {
        b.iter_batched_ref(
            || {
                let mut world = world();
                let entities = (0..ENTITIES)
                    .map(|_| world.spawn_empty().id())
                    .collect::<Vec<_>>();
                (world, entities)
            },
            |(world, entities)| {
                for &entity in entities.iter() {
                    world.entity_mut(entity).insert(WithHook);
                }
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("spawn_batch_template", |b| {
        b.iter_batched_ref(
            world,
            |world| {
                world.spawn_batch((0..ENTITIES).map(|_| WithTemplate));
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("spawn_batch_hook_workaround", |b| {
        b.iter_batched_ref(
            world,
            |world| {
                world.spawn_batch((0..ENTITIES).map(|_| WithHook));
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("spawn_batch_explicit", |b| {
        b.iter_batched_ref(
            world,
            |world| {
                let frame = world.resource::<Frame>().0;
                world.spawn_batch((0..ENTITIES).map(move |_| (Explicit, StartFrame(frame))));
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("insert_batch_template", |b| {
        b.iter_batched_ref(
            || {
                let mut world = world();
                let entities = (0..ENTITIES)
                    .map(|_| world.spawn_empty().id())
                    .collect::<Vec<_>>();
                (world, entities)
            },
            |(world, entities)| {
                world.insert_batch(entities.iter().map(|&entity| (entity, WithTemplate)));
            },
            BatchSize::LargeInput,
        );
    });
    group.bench_function("insert_batch_hook_workaround", |b| {
        b.iter_batched_ref(
            || {
                let mut world = world();
                let entities = (0..ENTITIES)
                    .map(|_| world.spawn_empty().id())
                    .collect::<Vec<_>>();
                (world, entities)
            },
            |(world, entities)| {
                world.insert_batch(entities.iter().map(|&entity| (entity, WithHook)));
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}
