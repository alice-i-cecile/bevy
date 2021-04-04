use super::World;

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
