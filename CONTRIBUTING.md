# Contributing

CTX v0.5 is a closed, token-gated product beta rather than an open-source project. Unsolicited pull
requests cannot be accepted under the current proprietary license.

Beta participants can contribute through:

- the dashboard's preview-before-send issue report;
- the optional 7-day and 21-day product check-ins; or
- private email to `saurabhsharan03@gmail.com` for security or sensitive feedback.

If you have explicit repository access for development, run before proposing a change:

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked -- --test-threads=1
node scripts/coherence/claims.mjs
```

Behavioral coherence is an additional required CI gate. Fitcheck runs locally immediately before a
PR can merge:

```bash
make pr-fitcheck PR=<number>
```

The command requires a clean worktree at the exact PR head, uses the developer's local Claude Code
login (or `ANTHROPIC_API_KEY`), and posts the required `Local Fitcheck` commit status. Setup, auth,
unparseable output, and a `Rework` verdict all fail closed. GitHub Actions never runs the model.

Never commit participant tokens, capabilities, roster exports, check-in payloads, screenshots, or
ID-to-label mappings.
