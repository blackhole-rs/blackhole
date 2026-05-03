# `blackhole` — command-line client

Part of the [Blackhole](https://github.com/blackhole-rs/blackhole) workspace. Sends and receives files end-to-end encrypted via short pronounceable codes.

## Install

Prebuilt binaries are not yet published. Build from source:

```sh
cargo install --path . --locked
```

## Usage

```sh
blackhole send <PATH>
blackhole receive <CODE>
blackhole --help
```

See the [workspace README](../README.md) for the full project layout and the rest of the components (mailbox, transit, library).

## License

[EUPL-1.2](../LICENSE).
