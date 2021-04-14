use crate::{component::Component, prelude::Entity};

use super::{FetchState, Relation, WorldQuery};
use std::{
    hash::{Hash, Hasher},
    marker::PhantomData,
};

pub struct QueryRelationFilter<Q: WorldQuery, F: WorldQuery>(
    pub <Q::State as FetchState>::RelationFilter,
    pub <F::State as FetchState>::RelationFilter,
    PhantomData<fn() -> (Q, F)>,
);

macro_rules! impl_trait {
    ($trait:ident, $($body:tt)*) => {
        impl<Q: WorldQuery, F: WorldQuery> $trait for QueryRelationFilter<Q, F>
            where
                <Q::State as FetchState>::RelationFilter: $trait,
                <F::State as FetchState>::RelationFilter: $trait {
            $($body)*
        }
    };
}

impl_trait!(
    Clone,
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone(), PhantomData)
    }
);

impl_trait!(
    Default,
    fn default() -> Self {
        Self(Default::default(), Default::default(), PhantomData)
    }
);

impl_trait!(
    PartialEq,
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1 == other.1
    }
);

impl_trait!(Eq,);

impl_trait!(
    Hash,
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
        self.1.hash(state);
    }
);

impl<Q: WorldQuery, F: WorldQuery> QueryRelationFilter<Q, F> {
    pub fn new() -> Self {
        Self::default()
    }

    // FIXME(Relationships) should the behaviour for this be target AND other_target AND other_other_target
    // or should it be target OR other_target OR other_other_target
    pub fn add_target_filter<T: Component, Path>(mut self, target: Entity) -> Self
    where
        Self: SpecifiesRelation<T, Path, RelationFilter = Self>,
    {
        Self::__add_target_filter(target, &mut self);
        self
    }
}

// NOTE: This is ~~hilarious~~ elegant type system hacking- thanks to @TheRawMeatball for coming up with this :)

pub trait SpecifiesRelation<Kind: Component, Path> {
    type RelationFilter;
    fn __add_target_filter(entity: Entity, relation_filter: &mut Self::RelationFilter);
}

pub struct Intrinsic;
pub struct InData<EndItem, Inner>(PhantomData<(EndItem, Inner)>);
pub struct InFilter<EndItem, Inner>(PhantomData<(EndItem, Inner)>);
pub struct InTuple<EndItem, Inner, const I: usize>(PhantomData<(EndItem, Inner)>);

impl<Kind: Component> SpecifiesRelation<Kind, Intrinsic> for &Relation<Kind> {
    type RelationFilter = <<Self as WorldQuery>::State as FetchState>::RelationFilter;
    fn __add_target_filter(entity: Entity, relation_filter: &mut smallvec::SmallVec<[Entity; 4]>) {
        relation_filter.push(entity);
    }
}
impl<Kind: Component> SpecifiesRelation<Kind, Intrinsic> for &mut Relation<Kind> {
    type RelationFilter = <<Self as WorldQuery>::State as FetchState>::RelationFilter;
    fn __add_target_filter(entity: Entity, relation_filter: &mut smallvec::SmallVec<[Entity; 4]>) {
        relation_filter.push(entity);
    }
}

impl<Kind: Component, Path, Q: WorldQuery, F: WorldQuery>
    SpecifiesRelation<Kind, InData<Kind, Path>> for QueryRelationFilter<Q, F>
where
    Q: SpecifiesRelation<
        Kind,
        Path,
        RelationFilter = <<Q as WorldQuery>::State as FetchState>::RelationFilter,
    >,
{
    type RelationFilter = Self;
    fn __add_target_filter(entity: Entity, relation_filter: &mut Self::RelationFilter) {
        Q::__add_target_filter(entity, &mut relation_filter.0);
    }
}
impl<Kind: Component, Path, Q: WorldQuery, F: WorldQuery>
    SpecifiesRelation<Kind, InFilter<Kind, Path>> for QueryRelationFilter<Q, F>
where
    F: SpecifiesRelation<
        Kind,
        Path,
        RelationFilter = <<F as WorldQuery>::State as FetchState>::RelationFilter,
    >,
{
    type RelationFilter = Self;
    fn __add_target_filter(entity: Entity, relation_filter: &mut Self::RelationFilter) {
        F::__add_target_filter(entity, &mut relation_filter.1);
    }
}

macro_rules! replace_expr {
    ($_t:tt $sub:expr) => {
        $sub
    };
}

macro_rules! count_tts {
    ($($tts:tt)*) => {0usize $(+ replace_expr!($tts 1usize))*};
}

macro_rules! impl_tuple_inner {
    ([$($head: ident),*], [$($tail: ident),*]) => {
        impl<Kind: Component, Inner, Selected, $($head: WorldQuery,)* $($tail: WorldQuery,)*>
            SpecifiesRelation<Kind, InTuple<Kind, Inner, { count_tts!($($head)*) }>>
            for
            ($($head,)* Selected, $($tail,)*)
        where
            Selected: WorldQuery +
                SpecifiesRelation<
                    Kind,
                    Inner,
                    RelationFilter = <<Selected as WorldQuery>::State as FetchState>::RelationFilter,
                >,
        {
            type RelationFilter = (
                $(<<$head as WorldQuery>::State as FetchState>::RelationFilter,)*
                <Selected::State as FetchState>::RelationFilter,
                $(<<$tail as WorldQuery>::State as FetchState>::RelationFilter,)*
            );

            #[allow(non_snake_case, unused)]
            fn __add_target_filter(entity: Entity, relation_filter: &mut Self::RelationFilter) {
                let (
                    $($head,)*
                    my_thing,
                    $($tail,)*
                ) = relation_filter;
                Selected::__add_target_filter(entity, my_thing);
            }
        }
    };
}

macro_rules! impl_tuple {
    ($($idents: ident),*) => {
        impl_tuple!([], [$($idents),*]);
    };
    ([$($head: ident),*], []) => {
        impl_tuple_inner!([$($head),*], []);
    };
    ([$($head: ident),*], [$last: ident]) => {
        impl_tuple_inner!([$($head),*], [$last]);
        impl_tuple!([$($head,)* $last], []);
    };
    ([$($head: ident),*], [$transfer: ident, $($tail: ident),*]) => {
        impl_tuple_inner!([$($head),*], [$($tail,)* $transfer]);
        impl_tuple!([$($head,)* $transfer], [$($tail),*]);
    };
}

impl_tuple!();
impl_tuple!(A);
impl_tuple!(A, B);
impl_tuple!(A, B, C);
impl_tuple!(A, B, C, D);
impl_tuple!(A, B, C, D, E);
impl_tuple!(A, B, C, D, E, F);
impl_tuple!(A, B, C, D, E, F, G);
impl_tuple!(A, B, C, D, E, F, G, H);
impl_tuple!(A, B, C, D, E, F, G, H, I);
impl_tuple!(A, B, C, D, E, F, G, H, I, J);
impl_tuple!(A, B, C, D, E, F, G, H, I, J, K);

#[cfg(test)]
#[test]
fn target_filter_tests() {
    fn assert_impl<Kind: Component, Path, T: SpecifiesRelation<Kind, Path> + ?Sized>() {}
    assert_impl::<u64, _, QueryRelationFilter<(&Relation<u32>, &Relation<u64>), ()>>();
    assert_impl::<u32, _, QueryRelationFilter<(&Relation<u32>, &Relation<u64>), ()>>();

    let foo: QueryRelationFilter<&Relation<u32>, ()> = Default::default();
    let foo = foo.add_target_filter::<u32, _>(Entity::new(1));
    dbg!(&foo.0);

    let foo: QueryRelationFilter<(&Relation<u32>, &Relation<u64>), ()> = Default::default();
    let foo = foo
        .add_target_filter::<u32, _>(Entity::new(1))
        .add_target_filter::<u64, _>(Entity::new(12));
    dbg!(&foo.0);
}
