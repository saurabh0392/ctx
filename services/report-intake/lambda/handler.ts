// Capability-authenticated report and beta check-in intake. All S3 objects stay private.

import { SSMClient, GetParameterCommand } from '@aws-sdk/client-ssm';
import { S3Client, PutObjectCommand, GetObjectCommand } from '@aws-sdk/client-s3';
import { createPresignedPost } from '@aws-sdk/s3-presigned-post';
import { getSignedUrl } from '@aws-sdk/s3-request-presigner';
import { createHmac, createHash, randomUUID, timingSafeEqual } from 'crypto';

const REGION = process.env.AWS_REGION!;
const BUCKET = process.env.BUCKET!;
const REPO = process.env.GITHUB_REPO!;
const TOKEN_PARAM = process.env.SSM_TOKEN_PARAM!;
const TOKENS_PARAM = process.env.SSM_TOKENS_PARAM!;
const CAPABILITY_SECRET_PARAM = process.env.SSM_CAPABILITY_SECRET_PARAM!;
const MAX_IMAGES = parseInt(process.env.MAX_IMAGES || '3', 10);
const MAX_IMAGE_MB = parseInt(process.env.MAX_IMAGE_MB || '5', 10);

const ssm = new SSMClient({ region: REGION });
const s3 = new S3Client({ region: REGION });
let cachedGithubToken = '';
let cachedCapabilitySecret = '';

const json = (code: number, body: unknown) => ({ statusCode: code, headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
const cap = (s: unknown, n: number) => String(s ?? '').slice(0, n);
const oneLine = (s: unknown, n: number) => cap(s, n).replace(/[\r\n]+/g, ' ').trim();
const fence = (s: unknown, n: number) => '```\n' + cap(s, n).replace(/```/g, "'''") + '\n```';
const exactKeys = (value: any, allowed: string[]) => !!value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).every(k => allowed.includes(k));
const participantId = (token: string) => createHash('sha256').update(token).digest('hex').slice(0, 16);
const constantEqual = (a: string, b: string) => {
  const aa = Buffer.from(a), bb = Buffer.from(b);
  return aa.length === bb.length && timingSafeEqual(aa, bb);
};

const LABELS: Record<string, string> = { bug: 'bug', coherence: 'coherence-regression', idea: 'enhancement' };
const PRODUCT_EVENTS = new Set([
  'setup_completed', 'dashboard_opened', 'context_bill_viewed', 'autopilot_paused',
  'autopilot_resumed', 'rewind_expanded', 'server_pruned', 'server_unpruned',
  'context_report_exported', 'beta_checkin_previewed', 'beta_checkin_sent',
]);
const SNAPSHOT_FIELDS = [
  'schema_version', 'participant_id', 'ctx_version', 'release_channel', 'os', 'arch', 'installed_at',
  'active_days_total', 'active_days_last7', 'sessions_total', 'sessions_last7', 'decisions_total',
  'decisions_joined', 'bill_ready', 'sink_tokens', 'reclaimable_tokens', 'reclaimed_tokens',
  'applied_trims', 'reexpansions', 'suspected_recovery_events', 'tools_watching', 'tools_trialing',
  'tools_earned', 'pruned_server_count', 'insight_action_count', 'latest_net_ahead_state',
  'product_event_counts',
];
const ANSWER_FIELDS = ['learned_something', 'changed_behavior', 'keep_using', 'price_interest_25_per_developer'];
const SNAPSHOT_STRINGS = ['participant_id', 'ctx_version', 'release_channel', 'os', 'arch', 'installed_at', 'latest_net_ahead_state'];
const SNAPSHOT_NUMBERS = [
  'schema_version', 'active_days_total', 'active_days_last7', 'sessions_total', 'sessions_last7',
  'decisions_total', 'decisions_joined', 'sink_tokens', 'reclaimable_tokens', 'reclaimed_tokens',
  'applied_trims', 'reexpansions', 'suspected_recovery_events', 'tools_watching', 'tools_trialing',
  'tools_earned', 'pruned_server_count', 'insight_action_count',
];

async function parameter(name: string): Promise<string> {
  const out = await ssm.send(new GetParameterCommand({ Name: name, WithDecryption: true }));
  return out.Parameter?.Value || '';
}

async function githubToken(): Promise<string> {
  if (!cachedGithubToken) cachedGithubToken = await parameter(TOKEN_PARAM);
  if (!cachedGithubToken) throw new Error('GitHub token not found');
  return cachedGithubToken;
}

async function capabilitySecret(): Promise<string> {
  if (!cachedCapabilitySecret) cachedCapabilitySecret = await parameter(CAPABILITY_SECRET_PARAM);
  if (cachedCapabilitySecret.length < 32) throw new Error('capability secret must be at least 32 characters');
  return cachedCapabilitySecret;
}

async function activeParticipants(): Promise<Set<string>> {
  let raw = '';
  try { raw = await parameter(TOKENS_PARAM); } catch { return new Set(); }
  const ids = new Set<string>();
  for (const line of raw.split(/\r?\n/)) {
    const text = line.trim();
    if (!text || text.startsWith('#')) continue;
    const token = text.split('=')[0].trim().split(/\s/)[0];
    if (token) ids.add(participantId(token));
  }
  return ids;
}

async function authenticate(value: unknown): Promise<string | null> {
  if (typeof value !== 'string' || value.length > 256) return null;
  const parts = value.split('.');
  if (parts.length !== 5 || parts[0] !== 'v1') return null;
  const [version, id, expiryText, scopes, signature] = parts;
  const expiry = Number(expiryText);
  if (!Number.isSafeInteger(expiry) || expiry < Math.floor(Date.now() / 1000)) return null;
  if (!scopes.split('-').includes('feedback') || !(await activeParticipants()).has(id)) return null;
  const expected = createHmac('sha256', await capabilitySecret()).update(`${version}.${id}.${expiryText}.${scopes}`).digest('hex');
  return constantEqual(signature, expected) ? id : null;
}

function validateBundle(value: any): any | null {
  if (value == null) return null;
  if (!exactKeys(value, ['schema', 'tools']) || value.schema !== 1 || !Array.isArray(value.tools) || value.tools.length > 50) throw new Error('invalid diagnostic bundle');
  return {
    schema: 1,
    tools: value.tools.map((t: any) => {
      if (!exactKeys(t, ['tool', 'stage', 'decisions', 'reclaimed', 'recoveries', 'reread'])) throw new Error('invalid diagnostic tool row');
      const number = (v: any) => Number.isFinite(v) ? Math.max(0, Math.floor(v)) : 0;
      const reread = t.reread == null ? null : Number(t.reread);
      if (typeof t.tool !== 'string' || typeof t.stage !== 'string' || (reread != null && !Number.isFinite(reread))) throw new Error('invalid diagnostic tool value');
      return { tool: oneLine(t.tool, 120), stage: oneLine(t.stage, 30), decisions: number(t.decisions), reclaimed: number(t.reclaimed), recoveries: number(t.recoveries), reread };
    }),
  };
}

function validateCheckin(value: any, participant: string): any {
  if (!exactKeys(value, ['schema', 'snapshot', 'answers']) || value.schema !== 'ctx.beta-checkin.v1') throw new Error('invalid check-in envelope');
  if (!exactKeys(value.snapshot, SNAPSHOT_FIELDS) || value.snapshot.schema_version !== 1 || value.snapshot.participant_id !== participant) throw new Error('invalid check-in snapshot');
  if (!exactKeys(value.answers, ANSWER_FIELDS)) throw new Error('invalid check-in answers');
  if (SNAPSHOT_NUMBERS.some(key => !Number.isSafeInteger(value.snapshot[key]) || value.snapshot[key] < 0)) throw new Error('invalid numeric snapshot field');
  if (SNAPSHOT_STRINGS.some(key => typeof value.snapshot[key] !== 'string')) throw new Error('invalid string snapshot field');
  if (ANSWER_FIELDS.some(key => typeof value.answers[key] !== 'string')) throw new Error('invalid check-in answer value');
  if (typeof value.snapshot.bill_ready !== 'boolean') throw new Error('invalid bill_ready field');
  const events = value.snapshot.product_event_counts;
  if (!events || typeof events !== 'object' || Array.isArray(events) || Object.entries(events).some(([k, v]) => !PRODUCT_EVENTS.has(k) || !Number.isSafeInteger(v) || Number(v) < 0)) throw new Error('invalid product event counts');
  const snapshot: any = {};
  for (const key of SNAPSHOT_FIELDS) snapshot[key] = value.snapshot[key];
  for (const key of SNAPSHOT_NUMBERS) snapshot[key] = Math.floor(snapshot[key]);
  snapshot.participant_id = participant;
  snapshot.ctx_version = oneLine(snapshot.ctx_version, 30);
  snapshot.release_channel = oneLine(snapshot.release_channel, 20);
  snapshot.os = oneLine(snapshot.os, 30);
  snapshot.arch = oneLine(snapshot.arch, 30);
  snapshot.installed_at = oneLine(snapshot.installed_at, 50);
  snapshot.latest_net_ahead_state = oneLine(snapshot.latest_net_ahead_state, 30);
  if (!['net_ahead', 'unconfirmed', 'behind', 'no_data'].includes(snapshot.latest_net_ahead_state)) throw new Error('invalid net-ahead state');
  return {
    schema: 'ctx.beta-checkin.v1', snapshot,
    answers: {
      learned_something: cap(value.answers.learned_something, 500),
      changed_behavior: cap(value.answers.changed_behavior, 500),
      keep_using: oneLine(value.answers.keep_using, 100),
      price_interest_25_per_developer: oneLine(value.answers.price_interest_25_per_developer, 100),
    },
  };
}

export const handler = async (event: any) => {
  let body: any;
  try { body = JSON.parse(event.body || '{}'); } catch { return json(400, { error: 'bad json' }); }
  try {
    const participant = await authenticate(body.credential);
    if (!participant) return json(403, { error: 'invalid, expired, or revoked capability' });

    if (body.action === 'presign') {
      if (!exactKeys(body, ['action', 'credential', 'images'])) return json(400, { error: 'unknown field' });
      if (!Array.isArray(body.images) || body.images.length > MAX_IMAGES) return json(400, { error: `at most ${MAX_IMAGES} images` });
      const uploads = await Promise.all(body.images.map(async (img: any) => {
        if (!exactKeys(img, ['name', 'contentType'])) throw new Error('invalid image descriptor');
        if (typeof img.name !== 'string' || typeof img.contentType !== 'string') throw new Error('invalid image descriptor value');
        const type = String(img.contentType || '').toLowerCase();
        if (!['image/png', 'image/jpeg', 'image/webp', 'image/gif'].includes(type)) throw new Error('unsupported image type');
        const ext = type === 'image/jpeg' ? 'jpg' : type.split('/')[1];
        const key = `images/${participant}/${randomUUID()}.${ext}`;
        const presigned = await createPresignedPost(s3, {
          Bucket: BUCKET, Key: key,
          Conditions: [['content-length-range', 1, MAX_IMAGE_MB * 1024 * 1024], ['eq', '$Content-Type', type]],
          Fields: { 'Content-Type': type }, Expires: 300,
        });
        return { key, url: presigned.url, fields: presigned.fields };
      }));
      return json(200, { uploads });
    }

    if (body.action === 'submit') {
      if (!exactKeys(body, ['action', 'credential', 'report', 'imageKeys'])) return json(400, { error: 'unknown field' });
      const r = body.report;
      if (!exactKeys(r, ['kind', 'title', 'description', 'example', 'bundle'])) return json(400, { error: 'invalid report fields' });
      if (!['bug', 'coherence', 'idea'].includes(r.kind)
        || typeof r.title !== 'string'
        || typeof r.description !== 'string'
        || typeof r.example !== 'string'
        || !Array.isArray(body.imageKeys)) return json(400, { error: 'invalid report values' });
      const title = oneLine(r.title, 140) || '(no title)';
      const kind = r.kind;
      const bundle = validateBundle(r.bundle);
      const rawKeys = body.imageKeys;
      if (rawKeys.length > MAX_IMAGES) return json(400, { error: `at most ${MAX_IMAGES} images` });
      const keys: string[] = rawKeys.filter((k: any) => typeof k === 'string' && k.startsWith(`images/${participant}/`) && /^[a-z0-9/_.-]+$/i.test(k));
      if (keys.length !== rawKeys.length) return json(400, { error: 'invalid image key' });

      let md = cap(r.description, 4000) + '\n';
      if (r.example) md += `\n**Example**\n${fence(r.example, 6000)}\n`;
      if (bundle) md += `\n<details><summary>Diagnostic snapshot (reviewed by participant)</summary>\n\n${fence(JSON.stringify(bundle, null, 2), 8000)}\n</details>\n`;
      if (keys.length) {
        const urls = await Promise.all(keys.map((k: string) => getSignedUrl(s3, new GetObjectCommand({ Bucket: BUCKET, Key: k }), { expiresIn: 7 * 86400 })));
        md += '\n**Screenshots (private links expire in 7 days)**\n' + urls.map(u => `![screenshot](${u})`).join('\n') + '\n';
      }
      md += `\n<sub>Filed via ctx beta participant ${participant}.</sub>`;
      const res = await fetch(`https://api.github.com/repos/${REPO}/issues`, {
        method: 'POST',
        headers: { authorization: `Bearer ${await githubToken()}`, accept: 'application/vnd.github+json', 'content-type': 'application/json', 'user-agent': 'ctx-report-intake' },
        body: JSON.stringify({ title, body: md, labels: ['beta-report', LABELS[kind] || 'bug'] }),
      });
      if (!res.ok) return json(502, { error: `github ${res.status}`, detail: cap(await res.text(), 300) });
      const issue = await res.json() as any;
      return json(200, { url: issue.html_url, number: issue.number });
    }

    if (body.action === 'checkin') {
      if (!exactKeys(body, ['action', 'credential', 'checkin'])) return json(400, { error: 'unknown field' });
      const checkin = validateCheckin(body.checkin, participant);
      const receipt = randomUUID();
      await s3.send(new PutObjectCommand({
        Bucket: BUCKET,
        Key: `checkins/${participant}/${receipt}.json`,
        Body: JSON.stringify({ received_at: new Date().toISOString(), ...checkin }),
        ContentType: 'application/json',
        ServerSideEncryption: 'AES256',
      }));
      return json(200, { receipt });
    }

    return json(400, { error: 'unknown action' });
  } catch (e: any) {
    return json(400, { error: cap(e?.message || 'error', 200) });
  }
};
