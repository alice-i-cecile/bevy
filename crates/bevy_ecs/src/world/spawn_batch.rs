use bevy_ptr::{move_as_ptr, MovingPtr};

use crate::{
    bundle::{Bundle, BundleSpawner, NoBundleEffect},
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
    RequiredTemplates(&'w mut World),
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
                mode: SpawnBatchMode::RequiredTemplates(world),
                caller,
            };
        }

        let change_tick = world.change_tick();

        let (lower, upper) = iter.size_hint();
        let length = upper.unwrap_or(lower);

        // SAFETY: the bundle was registered above
        let Ok(mut spawner) =
            (unsafe { BundleSpawner::new_with_id(world, bundle_id, change_tick) })
        else {
            unreachable!("the bundle has no required templates");
        };
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
        // Iterate through self in order to spawn remaining bundles.
        for _ in &mut *self {}
        if let SpawnBatchMode::Batched { spawner, allocator } = &mut self.mode {
            // Free all the over allocated entities.
            for e in allocator.by_ref() {
                spawner.allocator().free(e);
            }
            // Apply any commands from those operations.
            // SAFETY: `spawner` is not used again, and is dropped with `self`.
            unsafe { spawner.flush_commands() };
        }
    }
}

impl<I> SpawnBatchIter<'_, I>
where
    I: Iterator,
    I::Item: Bundle<Effect: NoBundleEffect>,
{
    #[cold]
    #[inline(never)]
    fn spawn_with_required_templates(&mut self, bundle: MovingPtr<'_, I::Item>) -> Entity {
        let SpawnBatchMode::RequiredTemplates(world) = &mut self.mode else {
            unreachable!();
        };
        world.spawn_with_caller(bundle, self.caller).id()
    }
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
        let SpawnBatchMode::Batched { spawner, allocator } = &mut self.mode else {
            return Some(self.spawn_with_required_templates(bundle));
        };
        Some(if let Some(bulk) = allocator.next() {
            // SAFETY:
            // - bundle matches spawner type and we just allocated it
            // - I::Item::Effect: NoBundleEffect
            unsafe {
                spawner.spawn_at(bulk, bundle, self.caller);
            }
            bulk
        } else {
            // SAFETY:
            // - bundle matches spawner type
            // - I::Item::Effect: NoBundleEffect
            unsafe { spawner.spawn(bundle, self.caller) }
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
