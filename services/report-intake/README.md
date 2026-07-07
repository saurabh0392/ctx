# Report intake (AWS)

Backs the dashboard "Report an issue" modal. A public Lambda Function URL that alpha users' dashboards
POST to, so they can file a GitHub issue with screenshots without a GitHub account or repo access. The
GitHub PAT stays server-side in SSM; images go straight to S3 via presigned POST.

## Endpoint (deployed to 553239736682 / us-east-1)

- URL: `https://yds5zrqx7pbhf7jcigvsjirepm0twbga.lambda-url.us-east-1.on.aws/`
- Bucket: `ctxreportintake-images2d38c313-govghgdw8vsa` (images public-read under `images/`, UUID keys,
  no listing, 90-day expiry)

Put the URL into the dashboard modal's `REPORT_ENDPOINT`. Do not bake it into a widely distributed
build until the abuse hardening below is in place.

## Two actions

- `POST { action: "presign", images: [{ name, contentType }] }` returns `{ uploads: [{ key, url, fields }] }`.
  The browser POSTs each image directly to S3 with those fields.
- `POST { action: "submit", report: { kind, title, description, example, bundle }, imageKeys: [...] }`
  creates the GitHub issue, links the images, labels it `alpha-report` plus the kind.

## Setup the account owner still does

1. Store the fine-grained PAT (Issues read/write on the repo) in SSM:
   ```
   aws ssm put-parameter --name /ctx/report-intake/github-token --type SecureString --value '<PAT>' --region us-east-1
   ```
2. Create the issue labels in the repo so `submit` does not 422: `alpha-report`, `coherence-regression`
   (`bug` and `enhancement` usually already exist).

## Operate

```
cd services/report-intake && npm install
CDK_DEFAULT_ACCOUNT=553239736682 CDK_DEFAULT_REGION=us-east-1 npx cdk deploy
npx cdk destroy   # tear it all down
```

## Guards in place, and what is not

In: per-report caps (25 images, 10 MB each), image-only content type enforced in the presigned POST
policy, text length caps, untrusted text fenced in a code block, least-privilege IAM (reads only the
one SSM param, writes only `images/`).

Not yet (Phase 2, before wide distribution): rate limiting and WAF on the Function URL. Until then the
endpoint is public and unthrottled, so keep the URL out of any broadly shipped build.
