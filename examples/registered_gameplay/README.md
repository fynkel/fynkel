# Registered gameplay

Health is authored ECS data; Door is a local behaviour; Damage is a population system. They read and write the same native component storage through the shared transaction API.

```sh
fynkel codegen --project . --project-only
dart test
```

`@FynkelComponent` plus `@Expose` generates a `ComponentType<Health>` codec and schema. Call `registerComponents(world)` before creation or scene loading. `ObjectSpec.data`/`ComponentData` author the data, while `readData`, `writeData`, `addData` and `removeData` access it from a GameObject. `world.queryComponents` returns an owned read-only population snapshot. `world.updateComponents` reads one frozen population and applies one validated batch; its callback returns a value copy. Saving a returned record and modifying it later does not change the engine.

The example orders Damage before the fixed behaviour phase. Door's callback is inherited through a mixin, so the test also checks effective callback discovery. Health exercises exposed fields from a superclass and multiple mixins, class overrides, default precedence and an inherited object reference. The CLI freshness test checks this generated fixture to prevent field-discovery regressions. The tests cook authored scenes and prefabs, resolve forward and self references, instantiate prefabs, and reject a missing registration without replacing the existing scene.

Generated defaults are literal data shared by runtime and editor schemas. Supported component fields: finite `double`, `int`, `bool`, `String`, `AssetRef`, and nullable `GameObject`. Collections, nested records, typed asset wrappers and marker layouts are rejected. Schemas are immutable within a world. A version/layout change requires explicitly migrated authored data and a recreated world; a same-schema generated codec can be registered again.

Systems declare phases, dependencies and access. They run sequentially, with lexical ordering for otherwise equal nodes and one transaction per callback. Access/structural declarations are descriptive audit metadata, not permission enforcement or a promise of safe parallelism. Failures are available through `world.drainSystemFailures()`. Native tick phases sit between host fixedUpdate and afterPhysics; dependency edges cannot jump between the two schedules.
