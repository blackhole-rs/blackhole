# Maintenance

Notes for maintainers of the Blackhole Rust workspace.

## Syncing from upstream

This repository is a fork of [`magic-wormhole/magic-wormhole.rs`](https://github.com/magic-wormhole/magic-wormhole.rs) (EUPL-1.2). To pull in upstream changes:

```sh
git remote add upstream https://github.com/magic-wormhole/magic-wormhole.rs.git   # one-time
git fetch upstream
git merge upstream/main
```

Expect conflicts on every merge — Blackhole renamed the `magic-wormhole` crate to `blackhole` everywhere, so any upstream change to `use` statements, type references, or Cargo metadata will conflict. Resolve by keeping the Blackhole names.

## Release

Releases are GitHub Releases only. We do not publish to crates.io (the upstream `magic-wormhole` crate is the canonical library on crates.io; we are a soft fork with a different name).

To cut a release:

1. Bump `[workspace.package] version` in the root `Cargo.toml`.
2. Update `CHANGELOG.md`.
3. Tag and push: `git tag v0.x.y && git push --tags`.
4. Build release binaries (CI is not yet configured; manual `cargo build --release -p blackhole-cli` etc.).
5. Upload artifacts to the GitHub release.

## License obligations

Mixed-license workspace:

- `blackhole` (library) and `blackhole-cli` are EUPL-1.2 (preserved from upstream `magic-wormhole.rs`). Do not relicense without a deliberate decision — EUPL is weak copyleft and only relicensable to a compatible license per EUPL Article 5.
- `blackhole-mailbox` and `blackhole-transit` are MIT, matching the licenses of the Python upstreams (`magic-wormhole-mailbox-server`, `magic-wormhole-transit-relay`) they are ported from. They are clean re-implementations and have no derivative-work relationship to the EUPL Rust client library.

When adding code to `mailbox/` or `transit/`, do not import from the `blackhole` library crate. If you ever need wormhole client code in those binaries, the static linking would create an EUPL derivative-work obligation — flag it before adding the dep.
