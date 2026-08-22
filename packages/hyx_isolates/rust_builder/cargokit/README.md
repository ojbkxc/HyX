# cargokit

This directory should contain the [cargokit](https://github.com/irondash/cargokit)
build tool, vendored from the upstream repository.

## Setup

cargokit is normally pulled in as a git submodule or vendored by
`flutter_rust_bridge_codegen create`. Since this project was scaffolded
manually, run one of:

```bash
# Option A: git submodule
git submodule add https://github.com/irondash/cargokit.git packages/hyx_isolates/rust_builder/cargokit

# Option B: vendor from an existing flutter_rust_bridge project
cp -r /path/to/other/rust_builder/cargokit/* packages/hyx_isolates/rust_builder/cargokit/
```

CI is expected to materialize this directory before building.