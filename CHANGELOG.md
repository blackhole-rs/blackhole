# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] — Blackhole fork

### Added

- New workspace crates: `blackhole-mailbox` (rendezvous server, skeleton) and `blackhole-transit` (transit relay, skeleton).
- `AI_POLICY.md` and `vouch` integration for contribution gating.
- URI scheme parser accepts both `blackhole-transfer:` (primary) and legacy `wormhole-transfer:` for inbound compatibility with magic-wormhole share links.

### Changed

- Forked from [`magic-wormhole.rs`](https://github.com/magic-wormhole/magic-wormhole.rs) v0.8.0. All identifiers renamed: library crate `magic-wormhole` → `blackhole`, CLI crate `magic-wormhole-cli` → `blackhole-cli`, binary `wormhole-rs` → `blackhole`.
- Default rendezvous server: `ws://relay.magic-wormhole.io:4000/v1` → `ws://relay.blackhole.rs:4000/v1`.
- Default transit relay: `tcp://transit.magic-wormhole.io:4001` → `tcp://transit.blackhole.rs:4001`.
- Production `APP_ID`: `lothar.com/wormhole/text-or-file-xfer` → `blackhole.rs/file-transfer`. **This is a protocol fork point: Blackhole clients no longer interoperate with magic-wormhole clients on file transfer.**
- Outbound URI scheme: `wormhole-transfer:` → `blackhole-transfer:`.
- License: `blackhole` (library) and `blackhole-cli` remain EUPL-1.2 (preserved from upstream). New `blackhole-mailbox` and `blackhole-transit` crates are MIT, matching their Python upstreams.

### Notes

The default mailbox and transit URLs point at infrastructure that is not yet deployed. The CLI will fail to send/receive until the Blackhole servers are running. Override with `--rendezvous-server` / `--relay-server` to use a different mailbox in the meantime.

---

The full changelog history inherited from `magic-wormhole.rs` upstream lives in [`CHANGELOG-upstream.md`](./CHANGELOG-upstream.md).
