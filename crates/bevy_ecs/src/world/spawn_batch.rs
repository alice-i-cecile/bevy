use bevy_ptr::{move_as_ptr, MovingPtr};

use crate::{
    bundle::{Bundle, BundleId, BundleSpawner, NoBundleEffect},
    change_detection::MaybeLocation,
    entity::{AllocEntitiesIterator, Entity, EntitySetIterator},
    world::World,
};
use core::iter::FusedIterator;

/// An iterator that spawns a series of entities and returns the [ID](Entity) of
/// each spawned entity.
///
/// If this iterator is not fully exhausted, any remaining entities will be spawned when this type is dropped.
pub struct SpawnBatchIter<'w, I>
where
    I: Iterator,
    I::Item: Bundle<Effect: NoBundleEffect>,
{
    inner: I,
    mode: SpawnBatchMode<'w>,
    caller: MaybeLocation,
}

enum SpawnBatchMode<'w> {
    /// Every entity is spawned into the same archetype with a single [`BundleSpawner`].
    Batched {
        spawner: BundleSpawner<'w>,
        allocator: AllocEntitiesIterator<'w>,
    },
    /// The bundle has required components built from templates, which need world access,
    /// so each entity is spawned individually.
    RequiredTemplates {
        world: &'w mut World,
        bundle_id: BundleId,
    },
}

impl<'w, I> SpawnBatchIter<'w, I>
where
    I: Iterator,
    I::Item: Bundle<Effect: NoBundleEffect>,
{
    #[inline]
    #[track_caller]
    pub(crate) fn new(world: &'w mut World, iter: I, caller: MaybeLocation) -> Self {
        let bundle_id = world.register_bundle_info::<I::Item>();
        // SAFETY: the bundle was just registered
        if unsafe { world.bundles.get_unchecked(bundle_id) }.has_required_templates {
            return Self {
                inner: iter,
                mode: SpawnBatchMode::RequiredTemplates { world, bundle_id },
                caller,
            };
        }

        let change_tick = world.change_tick();

        let (lower, upper) = iter.size_hint();
        let length = upper.unwrap_or(lower);

        // SAFETY: the bundle was registered above, and has no required templates
        let mut spawner = unsafe { BundleSpawner::new_with_id(world, bundle_id, change_tick) };
        spawner.reserve_storage(length);
        let allocator = spawner.allocator().alloc_many(length as u32);

        Self {
            inner: iter,
            mode: SpawnBatchMode::Batched { spawner, allocator },
            caller,
        }
    }
}

impl<I> Drop for SpawnBatchIter<'_, I>
where
    I: Iterator,
    I::Item: Bundle<Effect: NoBundleEffect>,
{
    fn drop(&mut self) {
        // Spawn the remaining bundles, matching on the mode once rather than once per bundle.
        match &mut self.mode {
            SpawnBatchMode::Batched { spawner, allocator } => {
                for bundle in &mut self.inner {
                    move_as_ptr!(bundle);
                    spawn_batched(spawner, allocator, bundle, self.caller);
                }
                // Free all the over allocated entities.
                for e in allocator.by_ref() {
                    spawner.allocator().free(e);
                }
                // Apply any commands from those operations.
                // SAFETY: `spawner` is not used again, and is dropped with `self`.
                unsafe { spawner.flush_commands() };
            }
            SpawnBatchMode::RequiredTemplates { world, bundle_id } => {
                for bundle in &mut self.inner {
                    move_as_ptr!(bundle);
                    spawn_with_required_templates(world, *bundle_id, bundle, self.caller);
                }
            }
        }
    }
}

#[inline(always)]
fn spawn_batched<B: Bundle<Effect: NoBundleEffect>>(
    spawner: &mut BundleSpawner,
    allocator: &mut AllocEntitiesIterator,
    bundle: MovingPtr<'_, B>,
    caller: MaybeLocation,
) -> Entity {
    if let Some(bulk) = allocator.next() {
        // SAFETY:
        // - bundle matches spawner type and we just allocated it
        // - B::Effect: NoBundleEffect
        unsafe {
            spawner.spawn_at(bulk, bundle, caller);
        }
        bulk
    } else {
        // SAFETY:
        // - bundle matches spawner type
        // - B::Effect: NoBundleEffect
        unsafe { spawner.spawn(bundle, caller) }
    }
}

#[cold]
#[inline(never)]
fn spawn_with_required_templates<B: Bundle>(
    world: &mut World,
    bundle_id: BundleId,
    bundle: MovingPtr<'_, B>,
    caller: MaybeLocation,
) -> Entity {
    let entity = world.entity_allocator.alloc();
    world
        .spawn_at_with_required_templates_or_report(entity, bundle_id, bundle, caller)
        .id()
}

impl<I> Iterator for SpawnBatchIter<'_, I>
where
    I: Iterator,
    I::Item: Bundle<Effect: NoBundleEffect>,
{
    type Item = Entity;

    fn next(&mut self) -> Option<Entity> {
        let bundle = self.inner.next()?;
        move_as_ptr!(bundle);
        Some(match &mut self.mode {
            SpawnBatchMode::Batched { spawner, allocator } => {
                spawn_batched(spawner, allocator, bundle, self.caller)
            }
            SpawnBatchMode::RequiredTemplates { world, bundle_id } => {
                spawn_with_required_templates(world, *bundle_id, bundle, self.caller)
            }
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<I, T> ExactSizeIterator for SpawnBatchIter<'_, I>
where
    I: ExactSizeIterator<Item = T>,
    T: Bundle<Effect: NoBundleEffect>,
{
    fn len(&self) -> usize {
        self.inner.len()
    }
}

impl<I, T> FusedIterator for SpawnBatchIter<'_, I>
where
    I: FusedIterator<Item = T>,
    T: Bundle<Effect: NoBundleEffect>,
{
}

// SAFETY: Newly spawned entities are unique.
unsafe impl<I: Iterator, T> EntitySetIterator for SpawnBatchIter<'_, I>
where
    I: FusedIterator<Item = T>,
    T: Bundle<Effect: NoBundleEffect>,
{
}

#[cfg(test)]
mod tests {
    use bevy_ecs_macros::Component;

    use super::*;

    #[derive(Clone, Copy, Component)]
    struct ComponentA;

    #[test]
    fn spawn_batch_does_not_leak_entities() {
        let mut world = World::new();
        world.spawn_batch((0u32..50).filter(|&i| i & 1 > 0).map(|_| ComponentA));
        let total_allocated = world.entity_allocator().inner.total_entity_indices();
        world.entity_allocator_mut().inner.flush_freed();
        world.entity_allocator_mut().alloc();
        let reused = world.entity_allocator().inner.total_entity_indices() == total_allocated;
        assert!(reused);
    }
}
