use crate::util;
use bevy_asset::{Asset, Assets, Handle, HandleId};
use bevy_ecs::world::World;
use bevy_math::*;
use bevy_reflect::Reflect;
use bevy_transform::prelude::Transform;
use bevy_utils::FloatOrd;

/// An individual input for [`Animatable::blend`].
pub struct BlendInput<T> {
    /// The individual item's weight. This may not be bound to the range `[0.0, 1.0]`.
    pub weight: f32,
    /// The input value to be blended.
    pub value: T,
    /// Whether or not to additively blend this input into the final result.
    pub additive: bool,
}

/// An animatable value type.
pub trait Animatable: Reflect + Sized + Send + Sync + 'static {
    /// Interpolates between `a` and `b` with  a interpolation factor of `time`.
    ///
    /// The `time` parameter here may not be clamped to the range `[0.0, 1.0]`.
    fn interpolate(a: &Self, b: &Self, time: f32) -> Self;

    /// Blends one or more values together.
    ///
    /// Implementors should return a default value when no inputs are provided here.
    fn blend(inputs: impl Iterator<Item = BlendInput<Self>>) -> Self;

    /// Post-processes the value using resources in the [`World`].
    /// Most animatable types do not need to implement this.
    fn post_process(&mut self, _world: &World) {}
}

macro_rules! impl_float_animatable {
    ($ty: ty, $base: ty) => {
        impl Animatable for $ty {
            #[inline(always)]
            fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
                let t = <$base>::from(t);
                (*a) * (1.0 - t) + (*b) * t
            }

            #[inline]
            fn blend(inputs: impl Iterator<Item = BlendInput<Self>>) -> Self {
                let mut value = Default::default();
                for input in inputs {
                    if input.additive {
                        value += <$base>::from(input.weight) * input.value;
                    } else {
                        value = Self::interpolate(&value, &input.value, input.weight);
                    }
                }
                value
            }
        }
    };
}

impl_float_animatable!(f32, f32);
impl_float_animatable!(Vec2, f32);
impl_float_animatable!(Vec3A, f32);
impl_float_animatable!(Vec4, f32);

impl_float_animatable!(f64, f64);
impl_float_animatable!(DVec2, f64);
impl_float_animatable!(DVec3, f64);
impl_float_animatable!(DVec4, f64);

// Vec3 is special cased to use Vec3A internally for blending
impl Animatable for Vec3 {
    #[inline]
    fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        (*a) * (1.0 - t) + (*b) * t
    }

    #[inline]
    fn blend(inputs: impl Iterator<Item = BlendInput<Self>>) -> Self {
        let mut value = Vec3A::ZERO;
        for input in inputs {
            if input.additive {
                value += input.weight * Vec3A::from(input.value);
            } else {
                value = Vec3A::interpolate(&value, &Vec3A::from(input.value), input.weight);
            }
        }
        Self::from(value)
    }
}

impl Animatable for bool {
    #[inline]
    fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        util::step_unclamped(*a, *b, t)
    }

    #[inline]
    fn blend(inputs: impl Iterator<Item = BlendInput<Self>>) -> Self {
        inputs
            .max_by(|a, b| FloatOrd(a.weight).cmp(&FloatOrd(b.weight)))
            .map(|input| input.value)
            .unwrap_or(false)
    }
}

impl Animatable for HandleId {
    #[inline]
    fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        util::step_unclamped(*a, *b, t)
    }

    #[inline]
    fn blend(inputs: impl Iterator<Item = BlendInput<Self>>) -> Self {
        inputs
            .max_by(|a, b| FloatOrd(a.weight).cmp(&FloatOrd(b.weight)))
            .map(|input| input.value)
            .expect("Attempted to blend HandleId with zero inputs.")
    }
}

impl<T: Asset> Animatable for Handle<T> {
    #[inline]
    fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        util::step_unclamped(a.clone_weak(), b.clone_weak(), t)
    }

    #[inline]
    fn blend(inputs: impl Iterator<Item = BlendInput<Self>>) -> Self {
        inputs
            .max_by(|a, b| FloatOrd(a.weight).cmp(&FloatOrd(b.weight)))
            .map(|input| input.value)
            .expect("Attempted to blend Handle with zero inputs.")
    }

    fn post_process(&mut self, world: &World) {
        // Upgrade weak handles into strong ones.
        if self.is_strong() {
            return;
        }
        *self = world
            .get_resource::<Assets<T>>()
            .expect(
                "Attempted to animate a Handle<T> without the corresponding Assets<T> resource.",
            )
            .get_handle(self.id());
    }
}

impl Animatable for Transform {
    fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        Self {
            translation: Vec3::interpolate(&a.translation, &b.translation, t),
            rotation: Quat::interpolate(&a.rotation, &b.rotation, t),
            scale: Vec3::interpolate(&a.scale, &b.scale, t),
        }
    }

    fn blend(inputs: impl Iterator<Item = BlendInput<Self>>) -> Self {
        let mut translation = Vec3A::ZERO;
        let mut scale = Vec3A::ZERO;
        let mut rotation = Quat::IDENTITY;

        for input in inputs {
            if input.additive {
                translation += input.weight * Vec3A::from(input.value.translation);
                scale += input.weight * Vec3A::from(input.value.scale);
                rotation = (input.value.rotation * input.weight) * rotation;
            } else {
                translation = Vec3A::interpolate(
                    &translation,
                    &Vec3A::from(input.value.translation),
                    input.weight,
                );
                scale = Vec3A::interpolate(&scale, &Vec3A::from(input.value.scale), input.weight);
                rotation = Quat::interpolate(&rotation, &input.value.rotation, input.weight);
            }
        }

        Self {
            translation: Vec3::from(translation),
            rotation,
            scale: Vec3::from(scale),
        }
    }
}

impl Animatable for Quat {
    /// Performs an nlerp, because it's cheaper and easier to combine with other animations,
    /// reference: <http://number-none.com/product/Understanding%20Slerp,%20Then%20Not%20Using%20It/>
    #[inline]
    fn interpolate(a: &Self, b: &Self, t: f32) -> Self {
        // Make sure is always the short path, look at this: https://github.com/mgeier/quaternion-nursery
        let b = if a.dot(*b) < 0.0 { -*b } else { *b };

        let a: Vec4 = (*a).into();
        let b: Vec4 = b.into();
        let rot = Vec4::interpolate(&a, &b, t);
        let inv_mag = bevy_math::approx_rsqrt(rot.dot(rot));
        Quat::from_vec4(rot * inv_mag)
    }

    #[inline]
    fn blend(inputs: impl Iterator<Item = BlendInput<Self>>) -> Self {
        let mut value = Self::IDENTITY;
        for input in inputs {
            value = Self::interpolate(&value, &input.value, input.weight);
        }
        value
    }
}
