# Glacis Launch

**Hollywood OS-style TUI launcher for the Glacis chain of custody system.**

## What is this?

This is a cyberpunk/industrial aesthetic demo launcher that showcases the Glacis vision without requiring the full cryptographic backend. It's perfect for:

- **Visual demos** - Screen recordings for pitches and documentation
- **UI/UX iteration** - Refine the aesthetic before backend complexity
- **Stakeholder communication** - Show the "feel" of the system immediately

## Features

### Cold Boot Sequence
A dramatic startup animation showing:
- Chain of custody initialization
- Enclave integrity verification
- Ed25519 key loading
- Witness network connection
- S3P protocol activation (0.1% sampling)
- Liability shield arming

### Live Dashboard
Three-panel interface displaying:
- **Left**: Live traffic logs with PII redaction warnings
- **Center**: ASCII art logo "THE CHAIN OF CUSTODY"
- **Right**: Real-time metrics (receipts minted, liability coverage, enclave status)

Simulated activity includes auto-incrementing receipt counts and randomly generated cryptographic receipt IDs.

## Running It

```bash
# From workspace root
cargo run --package glacis-launch

# Or build and run the binary directly
cargo build --package glacis-launch
./target/debug/glacis-launch
```

**Controls:**
- Press `q` to quit

## Recording a Demo

```bash
# Using asciinema
asciinema rec glacis-demo.cast -c "cargo run --package glacis-launch"

# Convert to GIF
agg glacis-demo.cast glacis-demo.gif
```

## Architecture Note

This is a **facade UI** - it generates no real cryptographic receipts and connects to no real enclaves. It exists purely to validate the aesthetic direction before implementing the actual:
- Azure Confidential Computing integration
- Ed25519 signature chains
- S3P sampling protocol
- Merkle tree construction

Think of it as a clickable wireframe with attitude.
