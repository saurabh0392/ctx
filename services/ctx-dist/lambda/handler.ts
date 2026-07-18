// Token-gated distribution. Invite tokens mint revocable, scoped 90-day capabilities; subsequent
// update/download calls use the capability so the one-time invite is never stored by the client.

import { SSMClient, GetParameterCommand } from '@aws-sdk/client-ssm';
import { S3Client, GetObjectCommand } from '@aws-sdk/client-s3';
import { getSignedUrl } from '@aws-sdk/s3-request-presigner';
import { createHash, createHmac, timingSafeEqual } from 'crypto';

const BUCKET = process.env.BUCKET!;
const SSM_TOKENS_PARAM = process.env.SSM_TOKENS_PARAM!;
const SSM_CAPABILITY_SECRET_PARAM = process.env.SSM_CAPABILITY_SECRET_PARAM!;
const FEEDBACK_ENDPOINT = process.env.FEEDBACK_ENDPOINT || '';
const PRESIGN_TTL = parseInt(process.env.PRESIGN_TTL || '300', 10);
const CAPABILITY_TTL_DAYS = parseInt(process.env.CAPABILITY_TTL_DAYS || '90', 10);

const ssm = new SSMClient({});
const s3 = new S3Client({});
let cachedSecret = '';

const SUPPORTED = new Set([
  'aarch64-apple-darwin',
  'x86_64-apple-darwin',
  'x86_64-unknown-linux-gnu',
  'x86_64-pc-windows-msvc',
]);

function reply(status: number, body: unknown, contentType = 'application/json') {
  return { statusCode: status, headers: { 'content-type': contentType }, body: typeof body === 'string' ? body : JSON.stringify(body) };
}

async function s3Text(key: string): Promise<string> {
  const out = await s3.send(new GetObjectCommand({ Bucket: BUCKET, Key: key }));
  return await out.Body!.transformToString();
}

const participantId = (token: string) => createHash('sha256').update(token).digest('hex').slice(0, 16);

// Roster entries remain "invite-token = label". Capabilities carry only the derived participant id;
// removing the roster line revokes both the invite and every capability minted from it.
async function roster(): Promise<Map<string, { token: string; label: string }>> {
  let raw = '';
  try {
    const out = await ssm.send(new GetParameterCommand({ Name: SSM_TOKENS_PARAM, WithDecryption: true }));
    raw = out.Parameter?.Value || '';
  } catch {
    return new Map();
  }
  const out = new Map<string, { token: string; label: string }>();
  for (const line of raw.split(/\r?\n/)) {
    const text = line.trim();
    if (!text || text.startsWith('#')) continue;
    const [rawToken, ...rest] = text.split('=');
    const token = rawToken.trim().split(/\s/)[0];
    if (!token) continue;
    out.set(participantId(token), { token, label: rest.join('=').trim() });
  }
  return out;
}

async function secret(): Promise<string> {
  if (cachedSecret) return cachedSecret;
  const out = await ssm.send(new GetParameterCommand({ Name: SSM_CAPABILITY_SECRET_PARAM, WithDecryption: true }));
  cachedSecret = out.Parameter?.Value || '';
  if (cachedSecret.length < 32) throw new Error('capability secret must be at least 32 characters');
  return cachedSecret;
}

function constantEqual(a: string, b: string): boolean {
  const aa = Buffer.from(a), bb = Buffer.from(b);
  return aa.length === bb.length && timingSafeEqual(aa, bb);
}

async function mint(id: string): Promise<string> {
  const expiry = Math.floor(Date.now() / 1000) + CAPABILITY_TTL_DAYS * 86400;
  const unsigned = `v1.${id}.${expiry}.download-feedback`;
  const sig = createHmac('sha256', await secret()).update(unsigned).digest('hex');
  return `${unsigned}.${sig}`;
}

async function verifyCapability(value: string, entries: Map<string, { token: string; label: string }>, scope: string): Promise<string | null> {
  const parts = value.split('.');
  if (parts.length !== 5 || parts[0] !== 'v1') return null;
  const [version, id, expiryText, scopes, signature] = parts;
  const expiry = Number(expiryText);
  if (!Number.isSafeInteger(expiry) || expiry < Math.floor(Date.now() / 1000)) return null;
  if (!scopes.split('-').includes(scope) || !entries.has(id)) return null;
  const expected = createHmac('sha256', await secret()).update(`${version}.${id}.${expiryText}.${scopes}`).digest('hex');
  return constantEqual(signature, expected) ? id : null;
}

function verifyInvite(token: string, entries: Map<string, { token: string; label: string }>): string | null {
  let matched: string | null = null;
  for (const [id, entry] of entries) if (constantEqual(token, entry.token)) matched = id;
  return token ? matched : null;
}

export const handler = async (event: any) => {
  const method: string = event?.requestContext?.http?.method || 'GET';
  const rawPath: string = event?.rawPath || '/';
  const host: string = event?.headers?.host || event?.requestContext?.domainName || '';
  const endpoint = host ? `https://${host}/` : '/';

  try {
    if (method === 'GET') {
      const name = rawPath.replace(/^\/+/, '') || 'install.sh';
      if (name !== 'install.sh' && name !== 'install.ps1') return reply(404, 'not found', 'text/plain');
      let script: string;
      try { script = await s3Text(name); }
      catch { return reply(404, `${name} not published yet`, 'text/plain'); }
      script = script.split('__CTX_ENDPOINT__').join(endpoint);
      return reply(200, script, name.endsWith('.ps1') ? 'text/plain' : 'text/x-shellscript');
    }

    if (method !== 'POST') return reply(405, { error: 'method not allowed' });
    let body: any;
    try {
      const raw = event.isBase64Encoded ? Buffer.from(event.body || '', 'base64').toString() : event.body || '{}';
      body = JSON.parse(raw);
    } catch { return reply(400, { error: 'bad json' }); }
    const keys = Object.keys(body || {});
    if (keys.some(k => !['token', 'credential', 'target'].includes(k))) return reply(400, { error: 'unknown field' });

    const target = String(body.target || '').trim().slice(0, 80);
    if (!SUPPORTED.has(target)) return reply(404, { error: `unsupported target: ${target}` });
    const entries = await roster();
    const invite = String(body.token || '').trim().slice(0, 256);
    const existing = String(body.credential || '').trim().slice(0, 256);
    if (!!invite === !!existing) return reply(400, { error: 'provide exactly one invite or capability' });
    const id = invite ? verifyInvite(invite, entries) : await verifyCapability(existing, entries, 'download');
    if (!id) return reply(403, { error: 'invalid, expired, or revoked credential' });

    let manifest: any;
    try { manifest = JSON.parse(await s3Text('manifest/latest.json')); }
    catch { return reply(503, { error: 'no release published yet' }); }
    const entry = manifest?.targets?.[target];
    // Manual smoke releases may advance one platform before the cross-platform release workflow.
    // A target-level version keeps every untouched platform on its last known-good artifact.
    const version = String(entry?.version || manifest?.version || '');
    const expectedKey = `bin/ctx-${version}-${target}.tar.gz`;
    if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)
      || entry?.file !== expectedKey
      || !/^[a-f0-9]{64}$/i.test(String(entry?.sha256 || ''))) {
      return reply(503, { error: 'release manifest failed validation' });
    }
    const url = await getSignedUrl(s3, new GetObjectCommand({ Bucket: BUCKET, Key: entry.file }), { expiresIn: PRESIGN_TTL });
    const credential = await mint(id); // rotate expiry on each authorized download/check.
    console.log(JSON.stringify({ event: invite ? 'install' : 'update_check', participantId: id, target, version }));
    return reply(200, {
      version,
      target,
      url,
      sha256: entry.sha256,
      credential,
      participantId: id,
      feedbackEndpoint: FEEDBACK_ENDPOINT,
    });
  } catch (e: any) {
    console.error('handler error', e?.message || e);
    return reply(500, { error: 'server error' });
  }
};
