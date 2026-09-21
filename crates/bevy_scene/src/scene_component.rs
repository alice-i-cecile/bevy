use bevy_asset::{AssetServer, Assets};
use bevy_ecs::{
    component::Component,
    error::{BevyError, Result},
    template::{FromTemplate, Template, TemplateContext},
};
use core::{any::TypeId, fmt, marker::PhantomData};

use crate::{ResolvedSceneRoot, Scene, ScenePatch};

/// Implemented for [`Component`]s that have an associated [`Scene`], which can be constructed
/// with [`Self::Props`].
///
/// In general, developers should not implement this manually. Instead, they should derive it,
/// which also derives [`Component`] and adds additional protections and assurances.
///
/// See the ["Scene Components"](crate#scene-components) section of the module docs to see how this is used in practice.
pub trait SceneComponent: Component + FromTemplate<Template: Default> {
    /// The "properties" passed into [`Self::scene`] to build the final scene.
    type Props: Default;

    /// A function that uses the given `props` to produce a [`Scene`]
    fn scene(props: Self::Props) -> impl Scene;
}

/// Marks that the [`Scene`] of the [`SceneComponent`] `C` has been applied to this entity.
///
/// Every scene component requires this using [`ApplySceneComponent`], so inserting a scene component
/// outside of a scene (ex: with [`World::spawn`](bevy_ecs::world::World::spawn)) applies its scene.
#[derive(Component)]
pub struct SceneApplied<C: SceneComponent>(PhantomData<fn() -> C>);

impl<C: SceneComponent> Default for SceneApplied<C> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<C: SceneComponent> Clone for SceneApplied<C> {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl<C: SceneComponent> fmt::Debug for SceneApplied<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SceneApplied")
            .field(&core::any::type_name::<C>())
            .finish()
    }
}

/// A [`Template`] that applies the [`Scene`] of the [`SceneComponent`] `C` to the entity it is built for,
/// using the default [`SceneComponent::Props`].
///
/// This is the required [`SceneApplied<C>`] of every scene component. When a scene component is spawned
/// as a scene, [`SceneApplied<C>`] is already part of the scene, so this does nothing. When it is inserted
/// like any other component, this applies its scene first. Like other required components, the scene
/// never overwrites the inserted `C` or components the entity already has.
pub struct ApplySceneComponent<C>(PhantomData<fn() -> C>);

impl<C> Default for ApplySceneComponent<C> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<C: SceneComponent> Template for ApplySceneComponent<C> {
    type Output = SceneApplied<C>;

    fn build_template(&self, context: &mut TemplateContext) -> Result<SceneApplied<C>> {
        let mut resolved = {
            let world = context.entity.world();
            let (Some(assets), Some(patches)) = (
                world.get_resource::<AssetServer>(),
                world.get_resource::<Assets<ScenePatch>>(),
            ) else {
                return Err(BevyError::error(
                    "Scene components can only be inserted into worlds with the ScenePlugin",
                ));
            };
            ResolvedSceneRoot::resolve(Box::new(C::scene(C::Props::default())), assets, patches)?
        };
        resolved.scene.remove_template(TypeId::of::<C::Template>());
        resolved.apply_if_new(context.entity, &mut Default::default())?;
        Ok(SceneApplied::default())
    }

    fn clone_template(&self) -> Self {
        Self::default()
    }
}
