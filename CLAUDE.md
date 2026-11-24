# Bobba

MITM proxy for Habbo, written in Rust, that runs G-Earth extensions.

## Product

- **Users:** mostly players who run ready-made extensions; also extension developers.
- **Moment of use:** the player opens Habbo, Bobba connects on its own (automatic mode), and their extensions run. This path has to work on the first try; Direct mode is a developer tool.

## Ecosystem

- **[G-Earth](https://github.com/G-Realm/G-Earth)** is the reference implementation. When the behaviour of the extension protocol or of packet expressions is unclear, its source is the answer.
- **[Sulek](https://sulek.dev/)** maps each `hotel_version` to the name and AS3 class of every packet header. Bobba downloads that map once per hotel version and caches it on disk. Confident names stay cached; unsure ones are fetched again after a day. When Sulek is offline and there is no cache, packets show only their ids. Bobba then looks up Sulek's class names in the installed client's SWF to read each packet's exact structure and field names.

## G-Earth contract

Every G-Earth extension runs on Bobba unchanged: same extension protocol, same packet expressions, `.jar` and G-Python included. When Bobba's behaviour could diverge from G-Earth's, raise the divergence with the maintainer before picking a side.

## Platforms

- Flash/AIR client only. Unity and Nitro may come later; design for what exists today.
- macOS and Windows. Most players are on Windows, and Windows is not yet tested: every OS-specific change covers both adapters and states what was not verified on Windows. Linux is out of scope.
- Reference build: habbo.com.br, `MAC63-202611041140-63291706`.

## Sensitive data

- Anything that identifies a Habbo account stays out of commits, tests and fixtures: SSO tickets, credentials, names, figures, room and user ids, traffic captured from a real session. Fixtures use synthetic data.
- Habbo's SWF and memory dumps never enter the repo; tests that need them read the installed client and stay `#[ignore]`.
- Bobba itself takes no action in the game; packets are sent by the player or by an extension.

## Verification

The pre-commit hook lives in `.githooks/`; enable it once per clone with `git config core.hooksPath .githooks`. It runs, in order:

1. `cargo test --manifest-path .lint/Cargo.toml`: the code has no comments; names carry the meaning.
2. `cargo fmt --all -- --check`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test --workspace`

UI tests run headless against fake states in `crates/app/src/scenes.rs`.

`#[ignore]` tests need the network, G-Python, the locally installed client or the live hotel. The ones in `.dev/live-tests` log into a real Habbo account and run only when the maintainer asks.
