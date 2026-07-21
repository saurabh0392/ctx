# Security policy

## Supported version

Security fixes are made against the latest token-gated beta release. During the v0.5 wave, that is
the only supported version.

## Report a vulnerability

Do not open a public issue. Email `saurabhsharan03@gmail.com` with:

- the affected CTX version and operating system;
- a concise reproduction or proof of concept;
- the impact you believe is possible; and
- whether any real user data or credentials were exposed.

You should receive an acknowledgement within three business days. Please allow time for a fix and
coordinated disclosure before publishing details.

## Security boundaries

- The dashboard binds to loopback only.
- Standard CTX does not proxy model traffic. An explicitly enabled model-path route is a loopback
  application reverse proxy to one fixed, displayed provider; it is never a generic or transparent
  proxy and CTX operates no cloud relay for that traffic.
- Beta capabilities are bearer credentials stored in `~/.ctx/beta.json` with owner-only permissions on Unix.
- Invite roster removal revokes both an invite and capabilities derived from it.
- Binary updates require a valid download scope and SHA-256 match before replacement.
- Applied trims retain verbatim output locally. Treat `~/.ctx/ctx.db` as sensitive developer data.

An opt-in model-path route has a broader local trust boundary: the CTX process sees prompts,
instructions, tool definitions and results, source content, and authorization headers transiently
in memory, then forwards them to the route's fixed OpenAI or Anthropic destination with ordinary TLS
verification. It persists neither raw model requests nor credentials. It persists content-free
route receipts and, only after provider acceptance, the exact original for an applied trim. The
dashboard shows the route, destination, lifecycle health, supported and unavailable paths, recovery
and purge controls, and immediate bypass command. No CTX CA, DNS rewrite, ambient proxy, generic
`CONNECT`, redirect following, or caller-selected upstream is used.

The macOS beta is not Developer ID signed or notarized, and the Windows beta is not Authenticode
signed. That distribution limitation is disclosed in the installer and README.
