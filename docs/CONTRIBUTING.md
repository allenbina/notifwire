# Contributing to notifwire

Thanks for your interest in contributing! notifwire is open source and welcomes
contributions.

> notifwire is in the design stage. Right now the most valuable contributions are
> feedback on the [spec](SPEC.md) and use cases — but code, docs, and plugins
> are all welcome as the build comes together.

## Ways to contribute

- **Shape the design** — read the [spec](SPEC.md) and open an issue with
  feedback, gaps, or ideas
- **Report bugs** — open an issue with details
- **Suggest features** — open an issue describing the use case
- **Write code** — fix bugs, add features, improve docs
- **Write plugins** — extend notifwire for everyone
- **Improve docs** — clarity helps everyone

## Development setup

notifwire is built with [Tauri v2](https://v2.tauri.app/) (Rust backend, web UI
frontend). You'll need the Rust toolchain and Node.js. Build instructions will
be added here as the codebase lands.

```bash
git clone https://github.com/allenbina/notifwire
cd notifwire
# build steps coming soon
```

## Code style

- Rust: `cargo fmt` and `cargo clippy` clean
- Keep functions small and focused
- Document public APIs and plugin contracts
- Match the surrounding code

## Pull request process

1. Fork the repo
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Make your changes with clear commits
4. Add tests for new behavior
5. Ensure tests pass
6. Open a pull request with a clear description

## Commit messages

Write clear, descriptive commit messages. Explain the why, not just the what.

## Code of Conduct

Be respectful. See our [Code of Conduct](CodeOfConduct.md).

---

<p align="center">
  <sub>part of the <em>wire</em> family</sub>
</p>
