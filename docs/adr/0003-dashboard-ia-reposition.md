# 0003. Dashboard information architecture: reposition off cost

- Status: accepted
- Date: 2026-06-07
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-1, CTX-6

## Context

The dashboard sold ctx as a cost/savings tool: it led with a Savings story (mostly `n/a`) and a
Prompt Stats budget hero. That contradicts the strategy in
`docs/strategy-context-truth-layer.md`, which positions ctx as the context truth and safety layer.
The old nav had nine tabs mixing customer value, cost framing, and dev tooling, and the Context
home had a panel-alignment defect. The clickable prototype (`docs/prototypes/dashboard-revamp.html`,
CTX-7) was approved as the visual spec.

## Decision

The customer-facing dashboard is three core screens plus two setup screens:

- Your context: Home (`context`), Proof (`proof`), Activity (`trace`).
- Setup: Profiles (`profiles`), Settings (`settings`).

Savings and Prompt Stats are removed from the customer nav and retired (CTX-4). "Proving"
(`experiment`) stays dev-gated and hidden by default. Tab ids are kept stable where a tab is being
reframed rather than rebuilt: Home reuses the `context` panel and Activity reuses the `trace`
panel, so existing wiring, deep links, and the request-trace data keep working. The default
landing tab stays Home.

No customer screen frames ctx around tokens saved or dollars. Value is told as truth and safety:
what ctx saw, what it left alone, and the causal proof that any trim it made was safe on the
user's own work.

## Alternatives considered

- **Keep Savings/Prompt Stats as secondary tabs.** Rejected: they anchor the wrong mental model
  (ctx as a cost tool) and were mostly empty, which reads as a broken product.
- **Rename tab ids to home/activity.** Rejected for now: renaming the panel ids would churn
  navigation, boot, deep links, and the request-trace loader for no user benefit. We reframe copy
  and nav labels instead and keep ids stable.
- **Build brand-new Home and Activity panels from the prototype markup.** Rejected: the prototype
  is a standalone design with its own CSS. Porting wholesale would duplicate styles and risk drift.
  We reuse the real dashboard's CSS vocabulary and reframe the existing panels.

## Consequences

- The USP (Proof) is now a first-class destination, backed by `/api/context/proof` (ADR 0002).
- Customers see five items instead of nine; dev tooling is hidden unless enabled.
- Stable tab ids mean "Home" is still `tab-context` and "Activity" is still `tab-trace` in code;
  contributors must know the label/id mapping (documented here).
- Retiring Savings/Prompt Stats removes their panels, JS, and the budget modal (CTX-4); any code
  that referenced those loaders must be cleaned up so boot does not call missing functions.
