use super::{FetchState, WorldQuery};
use std::{
    hash::{Hash, Hasher},
    marker::PhantomData,
};

// FIXME(Relationships) this is not remotely correct, we want an assoc type on `Fetch`
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
