use crate::{
    component::{Component, ComponentId, ComponentsRegistrator},
    relationship::RelationshipHookMode,
    world::EntityWorldMut,
};
use alloc::vec::Vec;
use bevy_ptr::PtrMut;
use bumpalo::Bump;

/// A reusable bump allocator for batching entity component insertions across scenes.
///
/// Use [`BundleWriter::begin_entity`] to obtain an [`EntityScratch`] for each entity.
/// After calling [`EntityScratch::write`], call [`BundleWriter::reset_alloc`] to reclaim
/// the bump memory for the next entity.
///
/// The key property: [`EntityScratch`] holds `&self.alloc`, so the borrow checker prevents
/// calling [`BundleWriter::reset_alloc`] while any scratch is still live.
#[derive(Default)]
pub struct BundleWriter {
    // Safety: this cannot be exposed, otherwise `alloc.reset()` could be called in arbitrary
    // places, which could invalidate the data stored in any live EntityScratch.
    alloc: Bump,
}

// SAFETY: all data written to EntityScratch via push_component is `Component: Send`
unsafe impl Send for BundleWriter {}

impl BundleWriter {
    /// Returns an [`EntityScratch`] that borrows from this writer's bump allocator.
    ///
    /// The borrow checker enforces the usage contract: the returned [`EntityScratch`]
    /// holds `&self.alloc`, which conflicts with the `&mut self` needed by
    /// [`BundleWriter::reset_alloc`]. This means `reset_alloc` can only be called
    /// after the scratch has been consumed via [`EntityScratch::write`].
    pub fn begin_entity(&mut self) -> EntityScratch<'_> {
        EntityScratch {
            alloc: &self.alloc,
            component_ids: Vec::new(),
            component_ptrs: Vec::new(),
        }
    }

    /// Resets the bump allocator, reclaiming all allocated memory for reuse.
    ///
    /// The borrow checker enforces that this is only callable once all [`EntityScratch`]
    /// instances created from this writer have been consumed via [`EntityScratch::write`].
    pub fn reset_alloc(&mut self) {
        self.alloc.reset();
    }
}

/// A short-lived scratch buffer for accumulating components to write to a single entity in one batch.
///
/// Created via [`BundleWriter::begin_entity`]. Consuming via [`EntityScratch::write`] releases
/// the borrow on the bump allocator, allowing [`BundleWriter::reset_alloc`] to reclaim memory
/// for the next entity.
///
/// Drop safety: unlike raw-pointer scratch buffers, this type cannot outlive the allocator
/// it borrows from, and the borrow checker prevents the allocator from being reset while any
/// staged component data is still live.
pub struct EntityScratch<'a> {
    alloc: &'a Bump,
    component_ids: Vec<ComponentId>,
    component_ptrs: Vec<PtrMut<'a>>,
}

impl<'a> EntityScratch<'a> {
    /// Stages `component` for batch insertion. Registers the component if not already registered.
    ///
    /// # Safety
    ///
    /// `components` must be from the same world as the entity that
    /// [`EntityScratch::write`] will be called with.
    pub unsafe fn push_component<C: Component>(
        &mut self,
        components: &mut ComponentsRegistrator,
        component: C,
    ) {
        let id = components.register_component::<C>();
        let component_ref = self.alloc.alloc(component);
        self.component_ids.push(id);
        self.component_ptrs.push(PtrMut::from(component_ref));
    }

    /// Inserts all staged components into `entity` in a single batch, then consumes this
    /// scratch — releasing the borrow on the bump allocator.
    ///
    /// # Safety
    ///
    /// All staged components must have been registered via a [`ComponentsRegistrator`]
    /// from the same world as `entity`.
    pub unsafe fn write(self, entity: &mut EntityWorldMut, mode: RelationshipHookMode) {
        unsafe {
            entity.insert_by_ids_internal(
                &self.component_ids,
                self.component_ptrs.into_iter().map(|p| p.promote()),
                mode,
            );
        }
    }
}
