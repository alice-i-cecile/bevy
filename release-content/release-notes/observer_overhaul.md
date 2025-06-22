---
title: Observer Overhaul
authors: ["@Jondolf", "@alice-i-cecile", "@hukasu]
pull_requests: [19596, 19663, 19611]
---

## Rename `Trigger` to `On`

In past releases, the observer API looked like this:

```rust
app.add_observer(|trigger: Trigger<OnAdd, Player>| {
    info!("Added player {}", trigger.target());
});
```

In this example, the `Trigger` type contains information about the `OnAdd` event that was triggered
for a `Player`.

**Bevy 0.17** renames the `Trigger` type to `On`, and removes the `On` prefix from lifecycle events
such as `OnAdd` and `OnRemove`:

```rust
app.add_observer(|trigger: On<Add, Player>| {
    info!("Added player {}", trigger.target());
});
```

This significantly improves readability and ergonomics, and is especially valuable in UI contexts
where observers are very high-traffic APIs.

One concern that may come to mind is that `Add` can sometimes conflict with the `core::ops::Add` trait.
However, in practice these scenarios should be rare, and when you do get conflicts, it should be straightforward
to disambiguate by using `ops::Add`, for example.

## Relation-backed observer storage and spawning

Ever wish you could just add an observer watching the entity you spawned as part of a single `spawn` call?
Wish you could nicely nest these into your hierarchy?
Us too!

While each observer was stored as an entity, all of the book-keeping was done in a centralized [`Observers`](https://docs.rs/bevy/0.16.1/bevy/ecs/observer/struct.Observers.html)
data storage.
This meant that we needed specialized APIs like [`EntityCommands::observe`](https://docs.rs/bevy/0.16.1/bevy/prelude/struct.EntityCommands.html#method.observe),
which didn't play nice with the `Bundle`-based spawning paradigm and led to all sorts of nasty cache-invalidation bugs.

To fix this, we've migrated observers over to use relations.
Because Bevy currently [only supports one-to-many relations](https://github.com/bevyengine/bevy/issues/18121), we've [had to get creative](https://github.com/bevyengine/bevy/issues/17607)

In practice, we've found that entity observers are used in one of two distinct ways.
The first is a pattern that we're calling "universal observers", where an observer is spawned via `App::add_observer`,
which either watches for "matching events targeting any observer", or restricts itself to a subset by explicitly watching some entities.
You might use universal observers for responding to lifecycle events, updating widget state across a whole class of widgets,
or handling some complex, low-throughput combat event.

While we could add a `Watching(Vec<Entity>)` / `WatchedBy(Entity)` relationship pair to handle this,
that would only allow one universal observer per entity, regardless of the kind!
Instead, we've opted for a simpler design, with only a `Watching<Vec<Entity>>` component,
and handle cleanup and invalidation manually.
Because users don't *care* about spawning universal observers inside of a nested `spawn` call, this is a fine compromise!

The other common pattern is something we're calling "bespoke observers": something that tracks exactly one entity
and adds unique behavior. This might be a scripted event tied to a door,
a callback defining the logic for what happens when a button is pressed, or some unique boss behavior.
For things like "behavior of an entire type of enemy", using observers or systems with the help of some components makes more sense,
but there really are "truly one-off" behaviors in games (and applications!).

We want to spawn and despawn this with the entity, it should mirror the setup of our ordinary parent-child relationship,
slotting in smoothly into our hierarchies and taking advantage of the niceties for spawning and cleanup granted by relations.
We've chosen to create an `ObserverOf(Entity)` / `ObservedBy(Vec<Entity>)` relation pairing to track this.

That means that you can now spawn in observers just like children!

```rust
fn ways_to_spawn_observers(mut commands: Commands){
    // Manually inserting the Relation component
    let player_entity = commands.spawn(Player).id();
    commands.spawn(Observer::new(player_observer_fn)).insert(ObserverOf(player_entity));

    // Using the `ObservedBy` component with the `SpawnableList` trait
    commands.spawn(Ally,
        ObservedBy::spawn_one(ally_observer_fn)
    );

    // Using the `observers!` macro
    commands.spawn(
        Boss,
        observers![
            observer_1,
            observer_2
        ]
    )
}
```

## Original targets

`bevy_picking`'s `Pointer` events have always tracked the original target that an entity-event was targeting,
allowing you to bubble events up your hierarchy to see if any of the parents care,
then act on the entity that was actually picked in the first place.

This was handy! We've enabled this functionality for all entity-events: simply call `On::original_target`.

## Expose name of the Observer's system

The name of the Observer's system is now accessible through `Observer::system_name`,
this opens up the possibility for the debug tools to show more meaningful names for observers.
