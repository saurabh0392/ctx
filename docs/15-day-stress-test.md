# 16-day ctx stress test (automated)

Hands-off product validation while you work normally in Claude Code on **your real project** (e.g. The Gaffer). ctx rotates experiment phases on a calendar, patches `~/.ctx/config.toml`, syncs Claude Code hooks, and sends macOS notifications on phase changes.

**Two different things:**

| Term | Meaning |
|------|---------|
| **Local ctx binary** | `cargo install --path ~/Projects/ctx` — the build you are stress-testing |
| **Corpus / project path** | Where you actually work in Claude Code — **not** the ctx source repo |

## One-time setup

```bash
# Real project work (recommended)
ctx experiment plan init --corpus ~/Documents/the-gaffer --template gaffer

# ctx source repo — only if you are developing ctx itself
ctx experiment plan init --corpus ~/Projects/ctx --template ctx

ctx experiment install-schedule   # daily 09:00 tick (macOS launchd)
ctx experiment tick               # run immediately (applies day 1 phase + syncs hooks)
```

Confirm background services from `ctx setup` are running (dashboard + ingest every 5 min).

Recommended config for similarity auto-profile:

```toml
similarity_min_avg_match = 0.5
similarity_min_confidence = 0.35
auto_apply_recommendations = false
```

**Reload your IDE window** after `ctx experiment tick` on day 1 and again when day 3 starts (hooks turn back on).

## What happens each day

`ctx experiment tick` (automatic at 09:00 if scheduled):

1. Resolves **day N** from `started_at` in the plan file
2. Applies the phase config patch if the phase changed (with config backup)
3. Syncs Claude Code hooks (off during pre-ctx, on afterward)
4. Runs ingest + refreshes A/B verdicts
5. Prints a digest (volume, sample gates, verdicts)
6. Notifies on phase change, days 7/10/15, and when an A/B arm hits 100+ samples

You do not need to open the dashboard daily.

## Phase calendar

| Days | Phase | What changes |
|------|-------|--------------|
| 1–2 | **pre_ctx** | **Without ctx.** Hooks and filters stripped. Ingest + statusLine only. This is your true baseline. |
| 3 | **ctx_warmup** | **ctx fully on.** All gates at 100%, profile `all`. Establishes ctx-on spend before A/B. |
| 4–7 | profile_ab | Profile filter 50/50 A/B |
| 8 | auto_pinned_* | Pin a profile, auto on |
| 9–10 | auto_pinned_all | Pin `all`, let auto pick |
| 11–12 | inject_ab | System prefix 50/50 only |
| 13 | adaptive_ab | Adaptive prefix 50/50 only |
| 14 | compress_ab | Output compression 50/50 only (`compress_pct = 50`) |
| 15 | compress_sgr_ab | Session-grounded retention 50/50 only (`compress_sgr_pct = 50`, compression stays on) |
| 16 | lock_in | Digest only. Decide and lock config |

### How to read the baseline

- **pre_ctx (days 1–2):** Claude Code behaves as if ctx were not installed. No tool filtering, no prefix injection, no compression hook. Dashboard still ingests sessions so you can compare spend per turn.
- **ctx_warmup (day 3):** ctx hooks run on every prompt with all features enabled. Compare day 3 averages to days 1–2 in the Experiment tab baseline card.
- **Feature A/B (days 4+):** Each phase flips one gate 50/50. Control = that feature skipped on ~half of prompts (ctx still installed). This is not the same as pre_ctx.

## Day 16

```bash
ctx experiment digest
ctx experiment apply    # optional — disables harmful/no-benefit prefix features
```

Write one paragraph you can defend with numbers: pre-ctx vs ctx-on cost per turn, per-feature A/B verdicts, auto match rate, compression chars removed, and which features stay on.

## Files

| Path | Purpose |
|------|---------|
| `~/.ctx/experiment-plan.toml` | Plan definition |
| `~/.ctx/experiment-plan-state.json` | Last applied phase, notification state |
| `~/.ctx/experiment-journal.jsonl` | Daily tick log |
| `~/.ctx/experiment-plan-backup.toml` | Config snapshot before first patch |
| `~/.ctx/ab-results.json` | A/B verdicts (existing) |

Templates in repo: [`experiment-plan.ctx.toml`](experiment-plan.ctx.toml), [`experiment-plan.gaffer.toml`](experiment-plan.gaffer.toml).
