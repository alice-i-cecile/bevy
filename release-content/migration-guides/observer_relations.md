---
title: Storing Observers as Relations
pull_requests: [TODO]
---

The central `Observers` data storage is no longer the ultimate source of truth for observers.
Instead, this is fundamentally driven by the `Observer` component.
The `ObserverOf` relation has been added to enable easy spawning of bespoke observers (which watch a single entity) as part of bundles.
When this component is added / removed, the corresponding `ObserverDescriptor` data is updated accordingly, using universal observers.

In order to make this synchronization reliable, `ObserverDescriptor` has been split out into its own component, which is immutable.
This component is automatically added with `Observer` via required components: its value can be set on component insertion.

To watch or unwatch entities *after* an observer has been spawned, TODO.

As part of this work, `ObserverDescriptor::with_entities`, `ObserverDescriptor::with_events` and `ObserverDescriptor::with_components`
have been renamed to `set_entities`/`set_events`/`set_components`. Contrary to their names and documentation, these overwrote the set of watched objects, rather than appending to them.
