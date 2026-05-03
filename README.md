<p align="center">
  <a href="https://blackhole.rs">
    <img src="https://github.com/blackhole-rs/.github/blob/main/profile/blackhole.png?raw=true" alt="Blackhole" width="160">
  </a>
</p>

<h1 align="center">Blackhole</h1>

<p align="center"><em>Send things into the void, safely.</em></p>

<p align="center">
  Protocol library, CLI, and server components for end&#8209;to&#8209;end encrypted, ephemeral file transfer.<br/>
  Same primitives as <a href="https://github.com/magic-wormhole/magic-wormhole">magic&#8209;wormhole</a>.
</p>

<p align="center">
  <a href="https://github.com/blackhole-rs/blackhole/blob/main/LICENSE">
    <img alt="License: EUPL-1.2" src="https://img.shields.io/badge/license-EUPL--1.2-86d8f5?style=flat-square">
  </a>
  <a href="https://discord.gg/dA4jCZCcD3">
    <img alt="Discord" src="https://img.shields.io/badge/chat-Discord-8b7cff?style=flat-square&logo=discord&logoColor=white">
  </a>
</p>

---

This repository is the Rust core of Blackhole: the protocol library, the CLI client, and the rendezvous (mailbox) and transit relay servers. The browser app lives at [`blackhole-rs/blackhole-rs`](https://github.com/blackhole-rs/blackhole-rs) and consumes this code.

## Layout

```
.            library crate `blackhole` — protocol, transit, transfer
cli/         binary crate `blackhole-cli` — `blackhole` CLI
mailbox/     binary crate `blackhole-mailbox` — rendezvous server (WIP)
transit/     binary crate `blackhole-transit` — transit relay (WIP)
```

## Quick start

```sh
# Build everything
cargo check --workspace

# Run the CLI
cargo run --bin blackhole -- --help

# Run a server (skeletons today, real implementations coming)
cargo run -p blackhole-mailbox
cargo run -p blackhole-transit
```

## Status

| Component | State |
| --- | --- |
| `blackhole` (library) | Working — fork of `magic-wormhole.rs` |
| `blackhole-cli` | Working — sends/receives via upstream relays for now |
| `blackhole-mailbox` | Skeleton — port of Python `magic-wormhole-mailbox-server` is the next milestone |
| `blackhole-transit` | Skeleton — port of Python `magic-wormhole-transit-relay` to follow |

## Contributing

Read [AI_POLICY.md](./AI_POLICY.md) before opening a PR or issue. We use [`vouch`](https://github.com/mitchellh/vouch) to gate participation; unvouched contributions are auto-closed.

## Credit

This project is a fork of [`magic-wormhole.rs`](https://github.com/magic-wormhole/magic-wormhole.rs) by Fina Wilke, piegames, Brian Warner, and contributors. The wormhole protocol is the work of Brian Warner and the [magic-wormhole](https://github.com/magic-wormhole/magic-wormhole) project. Mailbox and transit servers are ports of the Python reference implementations.

## License

Mixed, per crate:

| Crate | License | Reason |
| --- | --- | --- |
| `blackhole` (library) | [EUPL-1.2](./LICENSE) | Preserved from upstream `magic-wormhole.rs` |
| `blackhole-cli` | EUPL-1.2 | Statically links the EUPL library |
| `blackhole-mailbox` | [MIT](./mailbox/LICENSE) | Clean port of MIT-licensed `magic-wormhole-mailbox-server` |
| `blackhole-transit` | [MIT](./transit/LICENSE) | Clean port of MIT-licensed `magic-wormhole-transit-relay` |
