# KvasirMC engine

The Minecraft server engine behind KvasirMC: a hard fork of
[Pumpkin](https://github.com/Pumpkin-MC/Pumpkin), a Minecraft server written in Rust.
It serves one version, **Minecraft 26.3** (Java and Bedrock).

The fork starts at upstream commit `2709e8b7a` (2026-11-01). Upstream's history is kept
below our commits; the original README is in [`docs/upstream/README.md`](docs/upstream/README.md).
Upstream fixes are ported by hand when wanted. This repository is not affiliated with the
Pumpkin project, and bugs found here should not be reported to it unless they are
confirmed against upstream's own code.

## What differs from upstream

- **Restructured code.** Upstream's longest functions (up to 1,400 lines) and files over
  3,000 lines were split into modules by concept. Every split was checked against a test
  that records the old behaviour first (packet traces, structure and command outputs,
  world generation), so the split preserves behaviour.
- **Bug fixes** found by those tests, each in its own commit with a regression test:
  server freezes (throw-all from a result slot, minecarts on activator rails,
  `/fill ... destroy`), item creation and loss in Bedrock inventory handling, heightmap
  corruption, missing amethyst geodes, and more.
- **Our own crates** (`crates/kvasir-*`): mob AI level of detail, redstone dust networks
  settled in one pass, an adaptive sequential/parallel tick, tick-phase timing,
  resumable world pre-generation, the `.pump` chunk frame codec and a one-engine-per-world
  lock.
- **One plugin interface.** Only WIT `v0.1` is kept; the `v0.2` host was removed.

## Build

```sh
cargo build --release -p pumpkin
```

The binary is `target/release/pumpkin`. It reads `pumpkin.toml` from its working directory.

## Licence

GPL-3.0, like upstream (see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE)).
