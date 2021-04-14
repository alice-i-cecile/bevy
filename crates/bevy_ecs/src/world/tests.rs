use crate::component::StorageType;
use crate::prelude::*;

#[test]
fn relation_spawn() {
    relation_spawn_raw(StorageType::Table);
    relation_spawn_raw(StorageType::SparseSet);
}
fn relation_spawn_raw(storage_type: StorageType) {
    let mut world = World::new();

    world.register_component::<ChildOf>(storage_type).unwrap();

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
fn relation_query() {
    relation_query_raw(StorageType::Table);
    relation_query_raw(StorageType::SparseSet);
}
fn relation_query_raw(storage_type: StorageType) {
    struct ChildOf;

    let mut world = World::new();

    world.register_component::<ChildOf>(storage_type).unwrap();

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
fn relation_access() {
    relation_access_raw(StorageType::Table);
    relation_access_raw(StorageType::SparseSet);
}
fn relation_access_raw(storage_type: StorageType) {
    #[derive(Debug, PartialEq, Eq)]
    struct ChildOf {
        despawn_recursive: bool,
    }
    let mut world = World::new();

    world.register_component::<ChildOf>(storage_type).unwrap();

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
    assert_eq!(
        accessor.next().unwrap(),
        (
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
fn relation_query_mut() {
    relation_query_mut_raw(StorageType::Table);
    relation_query_mut_raw(StorageType::SparseSet);
}

fn relation_query_mut_raw(storage_type: StorageType) {
    #[derive(Eq, PartialEq, Debug, Copy, Clone)]
    struct MyRelation(bool, u32);

    struct Fragment<const N: usize>;

    let mut world = World::new();
    world
        .register_component::<MyRelation>(storage_type)
        .unwrap();

    let target1 = world.spawn().insert(Fragment::<1>).id();
    let target2 = world.spawn().insert(Fragment::<1>).id();
    let target3 = world.spawn().id();

    let targeter1 = world
        .spawn()
        .insert(Fragment::<0>)
        .insert("targeter1")
        .insert_relation(MyRelation(true, 10), target1)
        .insert_relation(MyRelation(false, 48), target2)
        .insert_relation(MyRelation(false, 14), target3)
        .id();
    let targeter2 = world
        .spawn()
        .insert("targeter2")
        .insert_relation(MyRelation(false, 75), target1)
        .insert_relation(MyRelation(true, 22), target2)
        .id();
    let targeter3 = world
        .spawn()
        .insert(Fragment::<0>)
        .insert("targeter3")
        .insert_relation(MyRelation(true, 839), target2)
        .insert_relation(MyRelation(true, 3), target3)
        .id();

    let mut query = world.query::<(Entity, &mut Relation<MyRelation>, &&str)>();

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new().add_target_filter::<MyRelation, _>(target2),
    );
    for (_, mut accessor, _) in query.iter_mut(&mut world) {
        let (_, mut rel) = accessor.single();
        rel.0 = !rel.0;
        rel.1 += 10;
    }

    query.set_relation_filter(
        &world,
        QueryRelationFilter::new()
            .add_target_filter::<MyRelation, _>(target1)
            .add_target_filter::<MyRelation, _>(target2),
    );
    let mut was_targeter1 = false;
    let mut was_targeter2 = false;
    for (targeter, accessor, name) in query.iter_mut(&mut world) {
        match () {
            _ if targeter == targeter1 => {
                was_targeter1 = true;
                assert_eq!(*name, "targeter1");
                let targets = accessor.map(|(t, rel)| (t, *rel)).collect::<Vec<_>>();
                assert_eq!(&targets[0], &(Some(target1), MyRelation(true, 10)));
                assert_eq!(&targets[1], &(Some(target2), MyRelation(true, 58)));
                assert_eq!(targets.len(), 2);
            }
            _ if targeter == targeter2 => {
                was_targeter2 = true;
                assert_eq!(*name, "targeter2");
                let targets = accessor.map(|(t, rel)| (t, *rel)).collect::<Vec<_>>();
                assert_eq!(&targets[0], &(Some(target1), MyRelation(false, 75)));
                assert_eq!(&targets[1], &(Some(target2), MyRelation(false, 32)));
                assert_eq!(targets.len(), 2);
            }
            _ => panic!(),
        }
    }
    assert!(was_targeter1 && was_targeter2);

    query.set_relation_filter(&world, QueryRelationFilter::new());
    for (_, accessor, _) in query.iter_mut(&mut world) {
        for (_, mut rel) in accessor {
            rel.0 = !rel.0;
            rel.1 *= 2;
        }
    }

    let mut was_targeter1 = false;
    let mut was_targeter2 = false;
    let mut was_targeter3 = false;
    for (targeter, accessor, name) in query.iter_mut(&mut world) {
        match () {
            _ if targeter == targeter1 => {
                was_targeter1 = true;
                assert_eq!(*name, "targeter1");
                let targets = accessor.map(|(t, rel)| (t, *rel)).collect::<Vec<_>>();
                assert_eq!(&targets[0], &(Some(target1), MyRelation(false, 20)));
                assert_eq!(&targets[1], &(Some(target2), MyRelation(false, 116)));
                assert_eq!(&targets[2], &(Some(target3), MyRelation(true, 28)));
                assert_eq!(targets.len(), 3);
            }
            _ if targeter == targeter2 => {
                was_targeter2 = true;
                assert_eq!(*name, "targeter2");
                let targets = accessor.map(|(t, rel)| (t, *rel)).collect::<Vec<_>>();
                assert_eq!(&targets[0], &(Some(target1), MyRelation(true, 150)));
                assert_eq!(&targets[1], &(Some(target2), MyRelation(true, 64)));
                assert_eq!(targets.len(), 2);
            }
            _ if targeter == targeter3 => {
                was_targeter3 = true;
                assert_eq!(*name, "targeter3");
                let targets = accessor.map(|(t, rel)| (t, *rel)).collect::<Vec<_>>();
                assert_eq!(&targets[0], &(Some(target2), MyRelation(true, 849 * 2)));
                assert_eq!(&targets[1], &(Some(target3), MyRelation(false, 6)));
                assert_eq!(targets.len(), 2);
            }
            _ => panic!(),
        }
    }
    assert!(was_targeter1 && was_targeter2 && was_targeter3);
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
