use crate::prelude::*;

#[test]
fn bar() {
    let mut world = World::new();
    let mut entity = world.spawn();
    entity.insert(10);
}

#[test]
fn foo() {
    let mut world = World::new();

    struct ChildOf;

    let parent = world.spawn().id();
    let not_parent = world.spawn().id();

    let mut child = world.spawn();
    let child = child.insert_relation(ChildOf, parent);

    assert!(child.contains_relation::<ChildOf>(parent));
    assert!(child.contains_relation::<ChildOf>(not_parent) == false);
    assert!(child.contains_relation::<u32>(parent) == false);

    assert!(child.remove_relation::<ChildOf>(parent).is_some());
    assert!(child.remove_relation::<ChildOf>(parent).is_none());
    assert!(child.remove_relation::<u32>(parent).is_none());
    assert!(child.remove_relation::<ChildOf>(not_parent).is_none());
}

#[test]
fn query() {
    struct ChildOf;

    let mut world = World::new();

    let parent1 = world.spawn().id();
    let child1 = world.spawn().insert_relation(ChildOf, parent1).id();
    let parent2 = world.spawn().id();
    let child2 = world.spawn().insert_relation(ChildOf, parent2).id();

    let mut query = world.query::<(Entity, &Relation<ChildOf>)>();
    let mut iter = query.iter_mut(&mut world);
    assert!(iter.next() == Some((child1, ())));
    assert!(iter.next() == Some((child2, ())));
    assert!(iter.next() == None);

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new().add_target_filter::<ChildOf, _>(parent1),
    );
    let mut iter = query.iter_mut(&mut world);
    assert!(iter.next() == Some((child1, ())));
    assert!(iter.next() == None);

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new().add_target_filter::<ChildOf, _>(parent2),
    );
    let mut iter = query.iter_mut(&mut world);
    assert!(iter.next() == Some((child2, ())));
    assert!(iter.next() == None);
}
