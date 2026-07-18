---
name: ctx
description: Inspect local CTX context status or recover a result that CTX explicitly marked as trimmed. Use only when the user asks about CTX, context savings, trimming status, or a CTX rewind id.
---

# CTX

CTX is local context-efficiency software. In Codex it observes local tool activity and can shorten
eligible shell output before Codex reads it, but it does not replace built-in or MCP results after
they run.

- Use the CTX MCP status capability when the user asks whether CTX is active or what it saved.
- Use `ctx_expand` only with a rewind id from a visible `[ctx trimmed ... id: ...]` marker.
- Explain Codex capability precisely: shell output may be trimmable after its Codex-specific safety
  check; built-in and MCP results are observed only.
- Do not route normal shell work through CTX manually. The trusted plugin hook owns that decision.
- Never claim that installation alone means hooks are trusted or that output has been shortened.
