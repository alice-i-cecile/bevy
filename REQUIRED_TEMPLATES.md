# Required templates: world-aware required components

Proof of concept for [bevy#20062](https://github.com/bevyengine/bevy/issues/20062), on branch `required-templates`.

`#[require(...)]` entries now use `bsn!` syntax and mean exactly what they mean in `bsn!`. Required components can
therefore be built from templates that load assets, read resources, or do anything else with the `World`.

```rust
#[derive(Component)]
#[require(
    Mesh3d("models/torus/torus.gltf#Mesh0/Primitive0"), // patches Mesh3d's template: loaded via the AssetServer
    ~{template(ring_material)},                          // any Template value
    ~{template(ring_name)},                              // can read the Ring being inserted
)]
struct Ring { points: u32 }

fn ring_name(context: &mut TemplateContext) -> Result<Name> {
    let points = context.inserting::<Ring>().map_or(0, |ring| ring.points);
    Ok(Name::new(format!("Ring worth {points} points")))
}
```

`world.spawn(Ring { points: 10 })` and `commands.spawn(...)` both produce an entity whose mesh, material and name
were built from the `World`. `on_add` hooks and `Add` observers already see those values.

## Requirements

Agreed with the maintainer:

1. **World access:** required components get full template context, meaning mutable access to the `World` and the target entity, the same as BSN's `TemplateContext`.
2. **Unified with BSN:** `#[require]` entries match `bsn!` syntax and semantics exactly.
3. **Hard guarantees:**
   - hooks and observers never see a placeholder;
   - `commands.spawn` behaves the same as `world.spawn`;
   - construction can fail, with a defined failure policy.
4. **Performance:** breaking changes are OK if justified, but plain `Default` requires must stay fast.
5. **Stretch goal:** remove `SceneComponent` in favor of the unified mechanism.

Requirements 1–4 are met. For 4, hardware counters show the fast path at parity with `main`, but some Criterion
benches still move by up to ~10% with code layout; see [Benchmarks](#benchmarks). The stretch goal is met in part; see [SceneComponent](#scenecomponent).

## Why this is hard

Required components are written in `BundleInfo::write_components`, in the middle of an archetype move. At that point
the only things available are `&mut Table` and `&mut SparseSets`, and the entity is half-moved, so any `World` access
would be unsound.

That is why the issue's proposals (a `FromWorld`, `&World` or `DeferredWorld` constructor) can't work as written.
The usual workaround is to insert a placeholder and fix it up in a hook, but then hooks and observers see the wrong value.

## Design: build, then write

When an insert needs a required template, the insert splits into two phases:

1. **Build (before the move, with full world access).** The archetype-graph edge for (current archetype, bundle)
   already records exactly which required components are missing. Every missing one is built into pooled scratch
   memory, both templates and plain constructors:
   - each template gets a `TemplateContext` for the target entity;
   - plain constructors are simply called.
2. **Write (one normal archetype move).** The explicit components and the built required components are inserted
   together as one dynamic bundle. That bundle has no requirements left to fill, so `write_components` never sees
   a template, and hooks and observers see every final value.

This is the same model `bsn!` already uses: `ResolvedScene::apply` builds every component template against the entity,
collects the results in a `BundleWriter`, then does one insert. Required templates do the same thing, driven by the
component graph instead of a scene.

### Fast path, and typed routing

`BundleInfo` and `ArchetypeAfterBundleInsert` cache a `has_required_templates` flag.

- **Inserts.** `BundleInserter::new_with_id` checks the *bundle's* flag first. If no component of the bundle requires
  a template, it builds the inserter exactly as before. Otherwise a cold path checks the archetype edge's flag and
  returns `Err(NeedsRequiredTemplates)` when this particular insert needs templates. Every insert has to go through
  that `Result`, so a call site that forgets the slow path doesn't compile.
- **Spawns and batch inserts.** These check the bundle flag once themselves, then use constructors whose `unsafe`
  contract is "this bundle has no required templates" (`BundleSpawner::new_with_id`, `BundleInserter::new_unchecked`).

Plain requires behave exactly as before.

A required template whose type *is* the component (the blanket `Clone` impl of `Template`) is detected when it is
registered and gets a plain constructor. So `#[require(Health(3))]` stays on the fast path, even though it now goes
through `bsn!`'s template patching.

### Slow path

`EntityWorldMut::insert_scratch_with_required_templates`:

- **Scratch memory.** The bundle's explicit components are moved into pooled scratch (one pool entry per nested
  insert, boxed so that taking and returning one is a pointer move). The scratch is one list: explicit components
  first, then required ones as they are built.
- **Build order.** Plain requirements are built first. Templates are then built in reverse order of the required
  components list. That list is depth-first, with every component *after* the ones it requires, so a template is built
  before the templates of the components it requires.
  - **Consequence for siblings:** two templates required directly by the same component are built in *reverse
    declaration order*. A template that reads a sibling through `inserting` must be declared *before* that sibling.
- **Reading the insert.** `TemplateContext::inserting::<C>()` returns any component this insert adds: an explicit
  component, or a required one that has already been built. That makes "props" work, including for inherited
  requirements (A requires B, and B requires a templated C that reads B).
  - Under `insert_if_new`, an explicit component the entity already has is discarded, so `inserting` returns the
    entity's existing value instead.
- **Re-planning.** Templates have `&mut World`, so they can change the target entity. For example, a template may
  insert a component that another template would have built. When the entity's archetype changes, the edge is looked
  up again and building continues. Values built for components that have since turned up are dropped, because required
  components never overwrite existing ones.
- **The write** goes through the original archetype edge for (current archetype, bundle), with
  `BundleInserter::insert_prebuilt`. The built values are put in the order of that edge's required components, and
  `write_components` writes them instead of calling constructors (a `WriteRequiredComponents` parameter, so the fast
  path compiles exactly as before). There is no second, dynamic bundle and no hashing of the write set.
- **Planning reads the edge in place.** The edge's required components are a boxed slice, and edges are cached once
  and never removed, so the build loop keeps a pointer to it across template builds instead of cloning each
  constructor's `Arc`.
- **Requirements registered after a bundle was cached** are missed, the same as on `main` for plain requires: the
  cached `BundleInfo` and its edges don't see them.
- **Cycles.** A template that, while being built, re-enters *itself* for the same entity is caught by a per-world build
  stack and returned as an error. For example, a template for `Egg` inserts `Chicken`, which requires the same `Egg`
  template. The stack is keyed on the template's identity, so different templates for the same component don't trip
  it. A drop guard pops the stack, so a panicking template doesn't leave it dirty.

### Failure policy

If any requirement fails to build, **nothing from the bundle is inserted**: the explicit components and the built
values are dropped. Where the error goes depends on the API:

| API | Where the error goes |
| --- | --- |
| `commands.spawn` / `EntityCommands::insert` | The command's error handler, like any other failing command |
| `bsn!` / `spawn_scene` (`BundleWriter::write` is now fallible) | `ApplySceneError::TemplateBuildError` |
| `World::spawn` / `EntityWorldMut::insert` and other infallible APIs | The world's `FallbackErrorHandler` (panics by default), with `ErrorContext::RequiredTemplate` |
| `try_insert_batch(_if_new)`, and `Commands::insert_batch` (which uses it) | `TryInsertBatchError::required_template_errors`, per entity. The other entities are still inserted |
| `spawn_batch`, `insert_batch(_if_new)`, `Commands::spawn_batch` | The world's `FallbackErrorHandler`, per entity |
| `resource_scope` re-inserting a resource whose templated requirement was removed during the scope | The world's `FallbackErrorHandler`, called from a `Drop` impl. The resource value is lost, and with the default panicking handler this can abort if the scope was already unwinding |

The template's error keeps its `Severity`, so the default `match_severity` handler logs a `BevyError::warning` instead of
panicking. The public docs of `spawn`, `insert`, `insert_if_new` and the batch APIs state this.

A failed spawn also despawns the entity, so spawning stays all-or-nothing. `World::spawn` still returns an
`EntityWorldMut` for the despawned entity, so chained calls on it panic. Side effects that a template already made (for
example entities it spawned) are not rolled back. Planning may have created the target archetype.

One gap remains: the bundle's effect (for example `children![]`) cannot be dropped without being applied under the
current `DynamicBundle` API, so on failure it is leaked. That is memory-safe, but fixing it properly needs a
`DynamicBundle::drop_effect`.

### Semantics

| Situation | Behavior |
| --- | --- |
| Component inserted explicitly in the same bundle | The explicit value wins, and the template is not built |
| Component already on the entity | The existing value is kept, and the template is not built |
| What the template sees on `context.entity` | The entity before this insert, as in `bsn!` |
| Explicit and already-built required components | Readable through `context.inserting::<C>()` |
| `insert`, `insert_if_new`, `spawn`, `Commands`, `insert_by_id(s)`, `BundleWriter` / BSN | Supported |
| `spawn_batch`, `insert_batch(_if_new)`, `try_insert_batch`, `Commands::spawn_batch` | Supported (falls back to per-entity inserts when the bundle has any required templates) |
| `insert_resource`, and `resource_scope` re-inserting a resource | Supported |

### Syntax

Every entry is parsed by `bsn!`'s own parser (moved into `bevy_ecs_macro_logic`) and generated by the same codegen,
so each entry means what it means in `bsn!`. Two forms keep their existing meaning, because they aren't `bsn!` entries
and removing them would break a lot of code for no gain:

| `#[require(...)]` | Meaning |
| --- | --- |
| `B` | `B::default()` (unchanged) |
| `B = expr` | `expr` evaluated on every insert, then `.into()` (unchanged) |
| `B(a)`, `B { f: v }`, `B::Variant`, `B::new(..)` | Exactly `bsn!`: patches `<B as FromTemplate>::Template::default()` (or calls its constructor). The whole entry, including argument expressions like `next_id()`, is evaluated again for every insert, as before |
| `~T { .. }` / `~T::new(..)` | Exactly `bsn!`: patches or constructs a type `T` that is itself a `Template` |
| `~{expr}`, `func(..)` | Exactly `bsn!`: `expr` or `func(..)` is a `Template` value |

Names (`#Name`), scene includes (`@...`) and related scene lists (`Children [..]`) are compile errors in `#[require]`.
Entries that don't name their component (`~{expr}`, `func(..)`) can't be checked for duplicates at compile time. A
duplicate panics when the component is registered, which can be in the middle of a `spawn`.

**Breaking change.** The argument forms (`B(..)`, `B { .. }`, `B::Variant`) now need `B: FromTemplate`, which for
most types means `Default + Clone`, and they pick up `bsn!`'s `.into()` rules for field values.
In practice the break was small. `cargo check --workspace --all-targets` (all crates, examples, tests and benches)
needed no changes to engine or example code. Only 12 test-only component types in `bevy_ecs` needed a `Default` (and
sometimes `Clone`) derive, and doc examples had to add the same derives. One `bsn!` quirk carries over: struct update
from a generic expression (`..Default::default()`) can't be type-inferred. That's redundant anyway, since a `bsn!`
patch already leaves unspecified fields at their defaults.

## Alternatives considered

- **`FromWorld` / `&World` / `DeferredWorld` constructors (the issue's proposals).** These would have to run
  mid-move, where no world access is sound. Running them before the move is exactly this design, and at that point
  there's no reason to stop at `&World`.
- **Placeholder, then patch in a hook.** This is today's workaround. It breaks the "never a placeholder" guarantee and costs two writes.
- **Two archetype moves, with hooks deferred until the end.** The entity would sit in the world with components
  whose hooks haven't run, visible to arbitrary template code. `inserting::<C>()` gives most of the benefit without that hazard.
- **Staging only the templated values and letting `write_components` fill in the rest.** Rejected: it needs a "stage"
  threaded through `write_components`, any insert path that doesn't provide one panics mid-move (which is unsound),
  and it can change constructor priority under inheritance.
- **A separate `~` marker for templates in `#[require]`.** Rejected: in `bsn!`, `~` marks a type that is itself a
  `Template`, so a different meaning in `#[require]` would invert it.
- **Every require as a template, plus a `Template::CONTEXT_FREE` const for the fast path.** Rejected: it needs a trait
  change, and comparing the template's `TypeId` with the component's gets the same fast path without one.
- **Building each `#[require]` template once at registration, then cloning it per insert.** Rejected: it silently turns
  `#[require(Id(next_id()))]` into "the same id for every entity".

## SceneComponent

**What changed.** Scene components now require a per-component marker, `SceneApplied<C>`, whose template
`ApplySceneComponent<C>` applies `C::scene(Props::default())` before `C` is inserted:

- **Everywhere, not just in scenes.** `world.spawn(Player { .. })` and `commands.spawn(..)` produce the full scene
  (components and children). This previously logged "spawned without its scene".
- **No overwriting.** The scene is applied with `InsertMode::Keep` (the new `BundleWriter::write_with_mode`), so it never
  overwrites components the entity already has. The inserted `C` wins over the scene's own `C`.
- **Nesting and several scene components.** Nested scene components (`@Base` inside `Derived`'s scene) work, and so
  does inserting a second scene component on the same entity.
- **Hook visibility.** `Player`'s `on_add` hook sees the scene's components and children. In the `@` path it sees the
  components but not the children, because BSN spawns related entities after the root insert.
- **`@Player` in `bsn!` is unchanged.** There, `SceneApplied<Player>` is part of the scene, so the template never runs.
- **Without `ScenePlugin`,** inserting a scene component logs an error and skips the scene, like before.
  A scene that fails to resolve (ex: a missing asset dependency) is an error, which fails the insert.

**Why `SceneComponent` isn't deleted.** Feathers (20 controls, 36 derives) depends on two things that only exist when a
scene is *resolved*, before any values are built:

1. **Patch-merge inheritance.** `FeathersToolButton` includes `@FeathersButton` and then patches
   `Node { padding, min_width }`, keeping the base button's `height`, `justify_content` and so on.
   Insert-time requirements can only *override* whole values.
2. **Props that aren't component data.** `FeathersButtonProps { caption: Box<dyn SceneList>, .. }` passes children as
   scene content.

So the *trait* still has to exist, as a resolve-time entry point. The *derive* could become
`#[require(@Self::scene)]`, and there is a concrete route to letting plain `C { .. }` in `bsn!` pull in `C`'s scene with
patch-merging: register scene bases in a `TypeId`-keyed resource that `ResolvedSceneRoot::resolve` reads. But that
changes behaviour for existing `bsn!` users, so it's a maintainer decision.

**Remaining scene-component gaps:**
- The scene is applied with its own insert, so it takes one extra archetype move.
- Components that are both in the scene and in the explicit bundle trigger hooks twice (add, then replace).
- `Ready` fires before `C` is inserted.
- The scene is resolved again on every insert.
- The scene's `C` is not skipped inside *cached* (asset) scenes. There the scene inserts its own `C` first, so
  `C`'s hooks see a value the inserted `C` then replaces. This **breaks the "never a placeholder" guarantee** for
  scene components whose scene comes from an asset.
- `SceneApplied<C>` records that the scene was applied, but nothing ties it to the scene's results. Removing `C`
  with its requirements and inserting it again applies the scene again, which duplicates its children.
  Anything that moves `C` without `SceneApplied<C>` has the same effect (`take`, cloning without required components).

Writing the scene into the same `BundleWriter` as the required components would fix all but the last two, but it needs
a multi-component template trait in `bevy_ecs`.

## Invariants and how they're tested

Four rounds of adversarial review (correctness, architecture, performance, style, API, docs and testing) found no
remaining memory unsafety. What the code now guarantees:

- **No world access mid-move.** A bundle with required templates can only be inserted with `insert_prebuilt`, after
  every requirement is built (typed routing), so `write_components` never reaches a template. This includes
  `resource_scope`'s re-insert.
- **The planned requirements stay valid while templates run**, because archetype edges are cached once, never
  removed, and keep their required components in a box. The build loop relies on this to read them in place.
- **Every value is built once and dropped once**, including on failure, re-planning and nested inserts
  (`required_templates_drop_each_value_once`).
- **Hooks and observers see built values** (`required_templates_build_before_hooks`, for `spawn`, `insert`,
  `insert_if_new`, `Commands`, `insert_by_id` and `insert_by_ids`).
- **All-or-nothing on failure**, with errors routed as in the failure table (`required_templates_failure_*`,
  `required_templates_try_insert_batch_returns_errors`, `scene_with_failing_required_template`).
- **Arguments are evaluated per insert** (`required_templates_evaluate_arguments_per_insert`).
- **Nested inserts and panics leave no stale state.** The scratch pool has one entry per nested insert, and the cycle
  stack is popped by a drop guard (`required_templates_nested_inserts`, `required_templates_recover_from_panics`).
- **Cycles are errors, not hangs**, including two scene components whose scenes contain each other
  (`required_templates_cycle_panics`, `scene_components_in_a_cycle`).
- **Grammar:** scene-only `bsn!` syntax is a compile error (`component_require_scene_syntax` compile-fail test).

Review findings that were checked and deliberately not changed:
- `#[non_exhaustive]` on `TemplateContext`: its new private field already prevents struct literals and exhaustive
  destructuring outside `bevy_ecs`.
- A comment at the bundle-effect leak site: the rustdoc of `insert_with_required_templates` and the known gaps below
  already state it.
- Linear scans over the built components while planning: requirement counts are small, and archetype work dominates.

## Breaking changes

- The argument forms of `#[require]` (`B(..)`, `B { .. }`, `B::Variant`) need `B: FromTemplate` (see [Syntax](#syntax)).
- A bare lowercase path, like `#[require(my_component)]`, now means a `bsn!` template value (as in `bsn!`), not
  `my_component::default()`. Components with lowercase names need `my_component = my_component::default()`.
- `BundleWriter::write` and `write_with_relationship_hook_insert_mode` return `Result`.
- `TemplateContext` has a private field, so it can only be built with `TemplateContext::new`.
- `TryInsertBatchError` has a new `required_template_errors` field, and is no longer `Clone` (`BevyError` isn't).
- `Commands::spawn` and `EntityCommands::insert` can fail, through the command's error handler.
- Scene components apply their scene when inserted outside of a scene.

## Benchmarks

`benches/benches/bevy_ecs/components/required.rs` also runs on `main`. `required_templates.rs` runs on the branch only.
Each iteration spawns or inserts 1,000 entities.

Numbers are from **redwood** (Fedora, Ryzen 9 9950X3D, idle, `powersave` governor), measured on 2026-09-21 at
`d7fd6aa044` against its merge base `a62cce8c05`. Later commits only touch the slow path, docs and tests.

**Stability, laptop vs redwood.** Re-measuring `main` against its own baseline, with no code change:

| | Laptop (Windows, other apps running) | redwood |
| --- | --- | --- |
| Typical confidence interval | ±5–10% | ±0.1–1% |
| Same binary, two runs | up to 1.9× apart (1.39 ms vs 729 µs) | most benches within ±1.5%, all within ±4.4% |
| Instruction counts (perf counters) | not available | identical across runs, to 0.1 instr/op |

Laptop numbers were useless for anything under ~15%. Even on redwood, builds of the *same source* from *different
directories* can differ by 20–30 instructions per op, because codegen-unit partitioning depends on paths. So
instruction counts were only compared between builds from the same directory.

**Fast path, Criterion.** `main` was measured before and after the branch, and the branch is compared with their mean.
With the default 16 codegen units, and with 1:

| Bench | Δ, default | Δ, 1 codegen unit |
| --- | --- | --- |
| `add_remove/table` | +4.2% | −0.5% |
| `add_remove/sparse_set` | +1.1% | −1.0% |
| `insert_simple/base` | +1.0% | −2.3% |
| `insert_simple/unbatched` | +7.1% | +1.0% |
| `required_default/spawn` | +4.2% | −2.3% |
| `required_default/insert` | −0.5% | +6.9% |
| `required_default/spawn_batch` | +0.2% | −0.1% |
| `required_default/insert_batch` | +0.3% | +4.0% |
| `required_default/commands_spawn` | +0.7% | −1.0% |
| `spawn_world/1_entities` | −3.6% | +9.0% |
| `spawn_world/100_entities` | +2.4% | +11% |
| `spawn_world/10000_entities` | −0.4% | +2.5% |

The two `main` runs differed by up to 4.4% (`spawn_world/1_entities`), and by 0.3–1.5% for most benches.

**Fast path, instruction and cycle counts.** A standalone harness reads hardware counters (`perf-event2`) for the same
operations. Cycles are two runs:

| Operation | `main` instr/op | branch instr/op | `main` cycles/op | branch cycles/op |
| --- | --- | --- | --- | --- |
| insert only | 735.6 | 711.9 (−3.2%) | 159 | 159 |
| insert + remove | 1548.5 | 1486.3 (−4.0%) | 322–324 | 318 |
| `add_remove/table` loop | 1512.5 | 1449.5 (−4.2%) | 305–308 | 299–301 |
| `add_remove/sparse_set` loop | 1064.0 | 1032.0 (−3.0%) | 229–230 | 223–225 |
| `insert_simple/unbatched` loop | 456.5 | 467.4 (+2.4%) | 139–143 | 138–140 |
| plain spawn | 364.6 | 367.8 (+0.9%) | 112–131 | 112–113 |
| spawn with 2 value requires | 518.5 | 529.2 (+2.1%) | 157–170 | 163 |
| `spawn_batch` with requires | 385.4 | 376.5 (−2.3%) | 90–114 | 89 |
| `insert_batch` with requires | 565.9 | 566.2 (0.0%) | 136–137 | 137 |
| `commands.spawn` with requires | 636.8 | 643.1 (+1.0%) | 209 | 202–204 |

In cycles, every operation is within `main`'s run-to-run spread. The Criterion regressions don't line up with the
counters, and they move when only the codegen-unit count changes: `add_remove` and `insert_simple/unbatched` are slower
with 16 units and at parity with 1, while `spawn_world` and `required_default/insert` do the reverse. So they come from
how the benchmark binary's code is partitioned and laid out rather than from work the fast path does. That's still
what a user of that binary would see, and 1-entity `spawn_world` with one codegen unit is the largest (+9–11%). It
hasn't been pinned down further.

**Binary size.** The harness's `.text` grows from 559.8 KB on `main` to 596.6 KB on the branch (+6.6%). Before
`d7fd6aa044` it was 612.4 KB (+9.4%).

**The optimization commits, one at a time** (instr/op, same directory, each against the commit before it):

| Commit | Effect |
| --- | --- |
| `15979cae45` spawn the rest of a batch without checking the mode per bundle | `spawn_batch`: −6.0 instr/op, cycles unchanged |
| `7ef74e1bef` always inline `BundleInserter::new_with_id` | insert: −39, insert + remove: −39, `add_remove`: −39 / −43; cycles −3 to −7 |
| `6f63743699` skip building a `Result` on the insert fast path | +0 to +5.5 instr/op, no cycle change: **no measurable benefit**, reverted in `3429be9808` |
| `d7fd6aa044` keep the slow path out of each bundle's generic code | `.text` −13.3 KB; spawn with requires and `commands.spawn` −7, remove −11, insert + remove −12.6 |

**Template path, per 1,000 entities (Criterion):**

| | template | hook workaround (placeholder, then `on_add` overwrite) | explicit value |
| --- | --- | --- | --- |
| `spawn` | 77.7 µs → 66.8 µs | 36.3 µs | 22.7 µs |
| `insert` | 72.8 µs → 62.8 µs | 40.6 µs | — |
| `spawn_batch` | 77.8 µs → 65.4 µs | 26.3 µs | 10.3 µs |
| `insert_batch` | 72.2 µs → 61.8 µs | 31.7 µs | — |

The arrows show the change from E (the later run; the other columns are from the same runs and didn't change beyond
noise). A templated requirement now costs ~20–30 ns more per entity than the hook workaround (down from ~40–60 ns
before the optimization commits), and hooks never see a placeholder. The batch APIs insert one entity at a time, so
batching gives templates nothing: `spawn_batch` costs the same as single spawns, while explicit values drop from
22.7 µs to 10.3 µs.

**The slow path, after E** (write through the original edge, and cheaper planning and scratch). Measured on
2026-09-22, `3429be9808` against `4c10b33243`. Hardware counters for one template requirement that reads a resource:

| Operation | before instr/op | after instr/op | before cycles/op | after cycles/op |
| --- | --- | --- | --- | --- |
| spawn with a template | 1801.5 | 1595.9 (−11%) | 429–433 | 354–357 (−18%) |
| insert with a template | 1819.8 | 1614.6 (−11%) | 432 | 342–346 (−20%) |
| `spawn_batch` with a template | — | 1552.8 | — | 347–350 |
| spawn with the hook workaround | 698.4 | 697.4 | 195–202 | 202–228 |

Each commit on its own:

| Commit | Template spawn / insert, instr/op | Cycles/op |
| --- | --- | --- |
| `d4efc13190` write through the original archetype edge | −43 / −42 | +14 / −20 |
| `832a3c5c76` read planned requirements from the edge instead of cloning `Arc`s | −79 / −79 | −50 / −27 |
| `9a9df9fd5c` spawn straight into the target archetype (**reverted** in `48651a78e8`) | −29 / +16 | no change |
| `4c10b33243` box the pooled scratch, and reuse its entity references | −84 / −84 | −38 / −40 |

Spawning straight into the target archetype needs the entity out of the empty archetype first (templates need it to
exist while they run), which costs about as much as the archetype move it saves, so it was reverted.

The fast path is unchanged: at most +7 instr/op (`add_remove`), cycles within run-to-run spread, and `.text` is
6.1 KB smaller. Criterion on the same pair shows the template benches 21–26% faster, while the hook workaround and
explicit benches, whose code didn't change, also measure 3–5% faster, so that run's baseline was slow by about that much.

Where a templated spawn's ~1,600 instructions go (counted with a temporary instrumentation patch, with the cost of
reading the counter subtracted): ~110 for the empty spawn, ~160 for the scratch and copying the explicit components,
~75 for planning, ~125 for the template itself, ~200 for the rest of the build loop (cycle guard, context, bookkeeping),
~210 for ordering the built values and looking up the edge again, ~485 for the archetype move with hooks and observers,
and ~125 for flushing and returning the scratch. The rest is spread thinly, so there is no single large cost left.

**Reproducing on redwood.** The Criterion script is `bench-on-redwood/bench-on-redwood.sh` in
[`bevy-work-helpers`](https://github.com/alice-i-cecile/bevy-work-helpers) (branch `bench-on-redwood` until it's
merged). Run it from inside a Bevy checkout that has both `required-templates` and `required-templates-bench-base`,
for example `HOST=linen@redwood ../bevy-work-helpers/bench-on-redwood/bench-on-redwood.sh`. It:
1. bundles only the new commits and fetches them on `$HOST`;
2. saves `required-templates-bench-base` (`main` plus the benchmark commits only) as the `main` criterion baseline;
3. compares the branch against that baseline;
4. runs the template benches.

The instruction and cycle counts come from a separate harness that only exists on redwood: `~/icount-bisect`, which
builds against the Bevy checkout in `~/rt-bisect` through a path dependency. Check out a commit in `~/rt-bisect`, then
`cargo build --release` in `~/icount-bisect` (add `--features tip` for the template cases) and run
`target/release/icount`. `ICOUNT_FILTER` limits which cases run.

Set `CARGO_PROFILE_BENCH_CODEGEN_UNITS=1` (with a separate `CARGO_TARGET_DIR`) for the single-codegen-unit numbers.

**Gotcha.** Sharing one `CARGO_TARGET_DIR` between two worktrees silently reused the other worktree's proc-macro and
`bevy_ecs` artifacts. Keep target dirs separate, and share `CRITERION_HOME` if you need baselines across them.
The script avoids the problem by switching branches inside a single checkout.

## Known gaps and follow-ups

- Bundle effects are leaked when a requirement fails (needs `DynamicBundle::drop_effect`).
- Batch APIs fall back to per-entity inserts when the bundle has any templated requirement. This is decided per
  bundle, not per archetype, so it happens even when every target already has the component.
- **Batching.** Batch APIs with templated requirements insert one entity at a time, and a cached spawner or inserter
  can't be kept across entities, because templates can change the world (and reallocate archetypes and tables)
  between them. With the boxed scratch pool there is little left to share: the batch forms cost about the same as
  single spawns and inserts. See [the slow path, stage by stage](#benchmarks).
- **Sibling order.** Templates build in reverse declaration order among siblings. Proper dependency ordering
  would need the requirement tree, not the flattened list.
- The scene-component gaps listed above.
