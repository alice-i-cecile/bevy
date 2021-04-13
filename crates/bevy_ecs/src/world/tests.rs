use crate::component::StorageType;
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
fn table_query() {
    struct ChildOf;

    let mut world = World::new();

    let parent1 = world.spawn().id();
    let child1 = world.spawn().insert_relation(ChildOf, parent1).id();
    let parent2 = world.spawn().id();
    let child2 = world.spawn().insert_relation(ChildOf, parent2).id();

    let mut query = world.query::<(Entity, &Relation<ChildOf>)>();
    let mut iter = query.iter_mut(&mut world);
    assert!(iter.next().unwrap().0 == child1);
    assert!(iter.next().unwrap().0 == child2);
    assert!(matches!(iter.next(), None));

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new().add_target_filter::<ChildOf, _>(parent1),
    );
    let mut iter = query.iter_mut(&mut world);
    assert!(iter.next().unwrap().0 == child1);
    assert!(matches!(iter.next(), None));

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new().add_target_filter::<ChildOf, _>(parent2),
    );
    let mut iter = query.iter_mut(&mut world);
    assert!(iter.next().unwrap().0 == child2);
    assert!(matches!(iter.next(), None));

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new()
            .add_target_filter::<ChildOf, _>(parent1)
            .add_target_filter::<ChildOf, _>(parent2),
    );
    let mut iter = query.iter_mut(&mut world);
    assert!(matches!(iter.next(), None));
}

#[test]
fn sparse_query() {
    struct ChildOf;

    let mut world = World::new();

    world
        .register_component::<ChildOf>(StorageType::SparseSet)
        .unwrap();

    let parent1 = world.spawn().id();
    let child1 = world.spawn().insert_relation(ChildOf, parent1).id();
    let parent2 = world.spawn().id();
    let child2 = world.spawn().insert_relation(ChildOf, parent2).id();

    let mut query = world.query::<(Entity, &Relation<ChildOf>)>();
    let mut iter = query.iter_mut(&mut world);
    assert!(iter.next().unwrap().0 == child1);
    assert!(iter.next().unwrap().0 == child2);
    assert!(matches!(iter.next(), None));

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new().add_target_filter::<ChildOf, _>(parent1),
    );
    let mut iter = query.iter_mut(&mut world);
    assert!(iter.next().unwrap().0 == child1);
    assert!(matches!(iter.next(), None));

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new().add_target_filter::<ChildOf, _>(parent2),
    );
    let mut iter = query.iter_mut(&mut world);
    assert!(iter.next().unwrap().0 == child2);
    assert!(matches!(iter.next(), None));

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new()
            .add_target_filter::<ChildOf, _>(parent1)
            .add_target_filter::<ChildOf, _>(parent2),
    );
    let mut iter = query.iter_mut(&mut world);
    assert!(matches!(iter.next(), None));
}

#[test]
fn table_relation_access() {
    #[derive(Debug, PartialEq, Eq)]
    struct ChildOf {
        despawn_recursive: bool,
    }
    let mut world = World::new();

    let random_parent = world.spawn().id();
    let parent1 = world.spawn().id();
    let parent2 = world.spawn().id();
    let child1 = world
        .spawn()
        .insert_relation(
            ChildOf {
                despawn_recursive: true,
            },
            parent1,
        )
        .insert_relation(
            ChildOf {
                despawn_recursive: false,
            },
            random_parent,
        )
        .id();
    let child2 = world
        .spawn()
        .insert_relation(
            ChildOf {
                despawn_recursive: false,
            },
            parent2,
        )
        .insert_relation(
            ChildOf {
                despawn_recursive: true,
            },
            random_parent,
        )
        .id();

    let mut query = world.query::<(Entity, &Relation<ChildOf>)>();

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new().add_target_filter::<ChildOf, _>(parent1),
    );
    let mut iter = query.iter(&world);
    let (child, mut accessor) = iter.next().unwrap();
    assert!(child == child1);
    assert!(
        accessor.next().unwrap()
            == (
                // FIXME(Relationships) honestly having Option<Entity> is really annoying
                // i should just make a statically knowable entity to represent None...
                Some(parent1),
                &ChildOf {
                    despawn_recursive: true
                }
            )
    );
    assert!(matches!(accessor.next(), None));
    assert!(matches!(iter.next(), None));

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new().add_target_filter::<ChildOf, _>(parent2),
    );
    let mut iter = query.iter(&world);
    let (child, mut accessor) = iter.next().unwrap();
    assert!(child == child2);
    assert_eq!(
        accessor.next().unwrap(),
        (
            // FIXME(Relationships) honestly having Option<Entity> is really annoying
            // i should just make a statically knowable entity to represent None...
            Some(parent2),
            &ChildOf {
                despawn_recursive: false
            }
        )
    );
    assert!(matches!(accessor.next(), None));
    assert!(matches!(iter.next(), None));

    query.set_relation_filter(&world, QueryRelationFilter::new());
    let mut iter = query.iter(&world);
    //
    let (child, mut accessor) = iter.next().unwrap();
    assert!(child == child1);
    assert_eq!(
        accessor.next().unwrap(),
        (
            Some(random_parent),
            &ChildOf {
                despawn_recursive: false
            }
        )
    );
    assert_eq!(
        accessor.next().unwrap(),
        (
            Some(parent1),
            &ChildOf {
                despawn_recursive: true
            }
        )
    );
    assert!(matches!(accessor.next(), None));
    //
    let (child, accessor) = iter.next().unwrap();
    assert!(child == child2);
    let foo = accessor.collect::<Vec<_>>();
    assert_eq!(
        foo[0],
        (
            Some(parent2),
            &ChildOf {
                despawn_recursive: false
            }
        )
    );
    assert_eq!(
        foo[1],
        (
            Some(random_parent),
            &ChildOf {
                despawn_recursive: true
            }
        )
    );
    assert!(foo.len() == 2);
    assert!(matches!(iter.next(), None));
}

#[test]
fn sparse_relation_access() {
    #[derive(Debug, PartialEq, Eq)]
    struct ChildOf {
        despawn_recursive: bool,
    }
    let mut world = World::new();

    world
        .register_component::<ChildOf>(StorageType::SparseSet)
        .unwrap();

    let random_parent = world.spawn().id();
    let parent1 = world.spawn().id();
    let parent2 = world.spawn().id();
    let child1 = world
        .spawn()
        .insert_relation(
            ChildOf {
                despawn_recursive: true,
            },
            parent1,
        )
        .insert_relation(
            ChildOf {
                despawn_recursive: false,
            },
            random_parent,
        )
        .id();
    let child2 = world
        .spawn()
        .insert_relation(
            ChildOf {
                despawn_recursive: false,
            },
            parent2,
        )
        .insert_relation(
            ChildOf {
                despawn_recursive: true,
            },
            random_parent,
        )
        .id();

    let mut query = world.query::<(Entity, &Relation<ChildOf>)>();

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new().add_target_filter::<ChildOf, _>(parent1),
    );
    let mut iter = query.iter(&world);
    let (child, mut accessor) = iter.next().unwrap();
    assert!(child == child1);
    assert!(
        accessor.next().unwrap()
            == (
                // FIXME(Relationships) honestly having Option<Entity> is really annoying
                // i should just make a statically knowable entity to represent None...
                Some(parent1),
                &ChildOf {
                    despawn_recursive: true
                }
            )
    );
    assert!(matches!(accessor.next(), None));
    assert!(matches!(iter.next(), None));

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new().add_target_filter::<ChildOf, _>(parent2),
    );
    let mut iter = query.iter(&world);
    let (child, mut accessor) = iter.next().unwrap();
    assert!(child == child2);
    assert_eq!(
        accessor.next().unwrap(),
        (
            // FIXME(Relationships) honestly having Option<Entity> is really annoying
            // i should just make a statically knowable entity to represent None...
            Some(parent2),
            &ChildOf {
                despawn_recursive: false
            }
        )
    );
    assert!(matches!(accessor.next(), None));
    assert!(matches!(iter.next(), None));

    query.set_relation_filter(&world, QueryRelationFilter::new());
    let mut iter = query.iter(&world);
    //
    let (child, mut accessor) = iter.next().unwrap();
    assert!(child == child1);
    assert_eq!(
        accessor.next().unwrap(),
        (
            Some(parent1),
            &ChildOf {
                despawn_recursive: true
            }
        )
    );
    assert_eq!(
        accessor.next().unwrap(),
        (
            Some(random_parent),
            &ChildOf {
                despawn_recursive: false
            }
        )
    );
    assert!(matches!(accessor.next(), None));
    //
    let (child, mut accessor) = iter.next().unwrap();
    assert!(child == child2);
    assert_eq!(
        accessor.next().unwrap(),
        (
            Some(random_parent),
            &ChildOf {
                despawn_recursive: true
            }
        )
    );
    assert_eq!(
        accessor.next().unwrap(),
        (
            Some(parent2),
            &ChildOf {
                despawn_recursive: false
            }
        )
    );
    assert!(matches!(accessor.next(), None));
    assert!(matches!(iter.next(), None));
}

#[test]
fn compiles() {
    let mut world = World::new();

    let mut query = world.query::<&u32>();

    let borrows = query.iter(&world).collect::<Vec<_>>();
    query.set_relation_filter(&world, QueryRelationFilter::new());
    let _borrows2 = query.iter(&world).collect::<Vec<_>>();
    dbg!(borrows);
}

#[test]
fn compile_fail() {
    let mut world = World::new();

    let mut query = world.query::<&Relation<u32>>();

    let _borrows = query.iter(&world).collect::<Vec<_>>();
    query.set_relation_filter(&world, QueryRelationFilter::new());
    let _borrows2 = query.iter(&world).collect::<Vec<_>>();
    // FIXME(Relationships) sort out a proper compile_fail test here
    // drop(_borrows);
}
