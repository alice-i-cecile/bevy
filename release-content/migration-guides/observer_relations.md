---
title: Storing Observers as Relations
pull_requests: [TODO]
---

The central `Observers` data storage is no longer the ultimate source of truth for observers.
Instead, this is fundamentally driven by the `Observer` component.
The `ObserverOf` relation has been added to enable easy spawning of bespoke observers (which watch a single entity) as part of bundles.
When this component is added / removed, the corresponding `ObserverDescriptor` data is updated accordingly, using universal observers.
