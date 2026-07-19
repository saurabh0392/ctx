# Alpha feedback to issue to agent fix pipeline (plan, draft for discussion)

Status: draft
Date: 2026-07-05
Owner: Saurabh Sharan

## The problem

ctx is local-first with no telemetry, which is a trust promise, not an accident. Alpha and beta users
run it on their own machines and are not collaborators on the repo, so they cannot open issues
directly, and we cannot ship a credential in the client to do it for them. We still need to learn what
is working, catch coherence regressions, and collect bug reports, then act on them without an admin
hand-triaging every one.

## Goals

- Capture improvement metrics and bug reports from local users without breaking the no-telemetry
  promise: opt-in, user-reviewed, redacted, user-initiated.
- Route a report into a GitHub issue on the private repo without giving the user repo access or a
  GitHub account.
- Let a GitHub-side agent triage each issue, attempt a fix where it is safe, prove the fix against the
  test and coherence gates, and hand a clean draft PR to the admin.
- Keep the admin as the only authority that merges and deploys.

## Principles (the non-negotiables)

1. No credential in the client. The GitHub token lives server-side only.
2. Redaction happens on the user's machine, before anything leaves it. The user is the primary
   redactor; the server does a second scrub.
3. The agent never merges and never deploys. Branch protection enforces admin review plus green CI.
4. The coherence suite and `ci.yml` are the QA backbone. An agent PR is trustworthy only because it
   clears the same gates a human PR does.
5. Every user-supplied string is untrusted input to a downstream agent, so it is fenced before it is
   ever read by triage.

## Architecture, three planes

### 1. Client: ctx dashboard

- A "Report an issue" modal (self-contained page in `src/dashboard.html`).
- Compose fields: title, type (bug / coherence-regression / idea), free-text description.
- Auto-attaches a redacted diagnostic bundle built from ctx's own aggregates (see below). The user
  can view and edit it before sending.
- Redaction: any example text the user pastes can be blacked out inline. Image attach with crop and
  blackout for screenshots.
- Preview before send. The endpoint URL is a config value, so it can point at staging or prod.

### 2. Intake: AWS

- **Lambda Function URL** with one route, `POST /report`. API Gateway only if a usage plan or WAF is
  wanted in front.
- **PAT in SSM Parameter Store as a SecureString** (free for a single param; Secrets Manager only if
  managed rotation is wanted). Fetched once per cold start and cached in the execution context, not
  per request. Fine-grained PAT scoped to `issues: write` on the one repo.
- **S3 for images, from v1**: alpha users can attach as many screenshots as they want, so images go
  to S3, never inlined in the issue body. The Lambda returns a presigned **POST** per image (presigned
  POST rather than PUT, so the policy can enforce `content-type: image/*` and a max object size). The
  browser uploads each image straight to S3, and the issue links public-read objects with unguessable
  UUID keys, so they render in the issue and persist (presigned GET URLs expire, so they cannot be the
  link in a lasting issue). A per-report cap and a bucket lifecycle rule bound cost and abuse.
- IAM least-privilege: the function role gets `ssm:GetParameter` on the one param and `s3:PutObject`
  on the one bucket, nothing else.

### 3. GitHub: issue to fix to review

- Issue created by the intake with labels (`alpha-report`, type, severity).
- **Triage agent** (Claude Code Action, headless, `ANTHROPIC_API_KEY` secret) on issue open or label:
  classify, set severity, comment a repro hypothesis, decide auto-fixable vs human-only. Narrow,
  well-scoped bugs are routed on; anything ambiguous is labeled and waits.
- **Auto-fix agent** on an `agent-fix` label: branch, write the fix, run `cargo test` plus the
  coherence suite in the job, and open a draft PR linked to the issue with the diff and gate results.
- **QA**: the PR's `ci.yml` runs fmt, tests, and coherence. Once those deterministic gates pass, the
  admin runs `make pr-fitcheck PR=<number>` locally on the exact PR head. That command publishes the
  required `Local Fitcheck` status; the model does not run in GitHub Actions.
- **Review and deploy**: green CI flips the draft to ready-for-review and assigns the admin. The admin
  reviews and merges after Local Fitcheck passes. Merge plus tag triggers `release.yml`, gated on
  deterministic preflight, platform builds/tests, and behavioral coherence.

## The diagnostic bundle

ctx already computes everything worth sharing on the dashboard, in aggregate form. The bundle is a
versioned JSON of counts and rates only:

- ctx version, config flags, bundle schema version.
- Per tool: `stage`, `decisions`, `reclaimed`, `reread_delta`, `correction_delta`, `recoveries`.
- Weekly net-ahead numbers.
- No file paths, no command text, no tool output. Paths, if ever included, are hashed, not raw.

The one metric to watch closely is recoveries per earned tool: a rising `ctx_expand` rate is the tool
telling us the earn-it gate is too loose.

## Security review (called out first, per repo policy)

- Client holds no token. This disqualifies any "dashboard talks to GitHub directly" design.
- The Function URL is a public write surface, so it needs throttling: an API Gateway usage plan, a WAF
  rate rule, or a lightweight captcha or short-lived signed nonce minted per dashboard session.
- SSM and S3 access is least-privilege and single-resource.
- Presigned uploads are a write path into your bucket, so the Lambda caps images per report,
  constrains `content-type` and max size through presigned POST conditions, rate-limits presign
  requests, and the bucket has a short lifecycle. Objects are public-read with unguessable UUID keys
  and listing is denied, so a URL reveals one image and nothing else.
- Every user string is fenced (code-blocked, length-capped, control chars stripped) before triage
  reads it. This is the prompt-injection seam between an anonymous reporter and the auto-fix agent.
- The agent's GitHub token can open PRs, comment, and label. It cannot merge, push to main, or
  publish. Branch protection requires admin review and green CI on main.

## Phasing (each phase ships and is testable on its own)

- **Phase 0, zero infra.** Add labels and an issue template. `ctx report` prints a redacted bundle the
  admin pastes into an issue during earliest alpha. Validates the schema before any service exists.
- **Phase 1, self-serve with images.** Lambda plus SSM plus S3 plus the modal. Users file their own
  reports with unlimited screenshots via presigned S3 uploads, text and images from day one, since
  diagnostics from alpha users lean on screenshots.
- **Phase 2, abuse hardening.** Rate limiting, captcha or signed nonce, WAF. Split out so Phase 1 can
  ship, but landed before alpha widens.
- **Phase 3, triage agent.** Comment and label only, no code changes.
- **Phase 4, auto-fix agent.** Draft PRs, gated by coherence and `ci.yml`.

## Cost sketch

- Lambda, SSM SecureString, and S3 sit inside free tiers at alpha scale, so cents per month.
- Agent runs cost tokens per triage or fix, so both are gated on labels rather than firing on every
  issue.

## Open decisions to settle before building

1. **Repo topology.** Single private repo with the intake proxy (recommended), or a separate public
   feedback repo where users with GitHub accounts file directly and we port real bugs across (no
   proxy, but requires users to have GitHub accounts and makes the reports public).
2. **Bundle contents.** The minimum viable metric set, and the redaction line: are paths dropped
   entirely or hashed.
3. **Abuse control.** Signed nonce minted by the dashboard (weak, but low friction), captcha
   (friction), or rate-limit only (spam risk). Pick the tradeoff.
4. **Auto-fix scope.** Which issue classes are ever auto-fixed versus always human, and the confidence
   threshold the triage step uses to route to auto-fix.
5. **Ownership.** Who runs the AWS account and its budget, and the bot GitHub identity: a GitHub App
   installation (production-grade, rotating), a machine user, or a fine-grained PAT (simplest).
6. **Images.** Resolved: full S3 presigned from v1, unlimited screenshots per report, bounded by a
   per-report cap and per-image size limit for cost and abuse.
