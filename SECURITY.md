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
- CTX does not proxy agent traffic.
- Beta capabilities are bearer credentials stored in `~/.ctx/beta.json` with owner-only permissions on Unix.
- Invite roster removal revokes both an invite and capabilities derived from it.
- Binary updates require a valid download scope and SHA-256 match before replacement.
- Applied trims retain verbatim output locally. Treat `~/.ctx/ctx.db` as sensitive developer data.

The macOS beta is not Developer ID signed or notarized, and the Windows beta is not Authenticode
signed. That distribution limitation is disclosed in the installer and README.
