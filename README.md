# Glacis

**The Chain of Custody for AI Inference**

A Rust workspace for building cryptographic chain-of-custody infrastructure for AI systems.

## Workspace Structure

This repository is organized as a Cargo workspace with the following crates:

### `glacis-core`
Core library implementing the cryptographic chain of custody primitives.

**Status:** Namespace reservation

### `glacis-launch`
Hollywood OS-style TUI launcher providing a cinematic demonstration of the Glacis system aesthetics.

**Status:** ✅ Functional demo

Run it with:
```bash
cargo run --package glacis-launch
```

See [`glacis-launch/README.md`](glacis-launch/README.md) for details.

## Quick Start

```bash
# Clone the repository
git clone https://github.com/Glacis-io/glacis-cli
cd glacis

# Run the demo launcher
cargo run --package glacis-launch

# Build all workspace members
cargo build --workspace
```

## License

MIT OR Apache-2.0

## Authors

Joe Braidwood <joe@glacis.io>
