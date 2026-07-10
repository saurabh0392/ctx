// ctx distribution endpoint (Lambda Function URL).
//
//   GET  /install.sh   -> the installer script, public, with its own endpoint templated in
//   GET  /install.ps1  -> the Windows installer, same
//   POST /             -> { token, target } validated against the SSM allowlist; returns a short-lived
//                         presigned download URL and the sha256 for that target, or 403 / 404
//
// The binaries are never on a public URL. A leaked presigned URL is one binary for a few minutes.

import { SSMClient, GetParameterCommand } from '@aws-sdk/client-ssm';
import { S3Client, GetObjectCommand } from '@aws-sdk/client-s3';
import { getSignedUrl } from '@aws-sdk/s3-request-presigner';

const BUCKET = process.env.BUCKET!;
const SSM_TOKENS_PARAM = process.env.SSM_TOKENS_PARAM!;
const PRESIGN_TTL = parseInt(process.env.PRESIGN_TTL || '300', 10);

const ssm = new SSMClient({});
const s3 = new S3Client({});

const SUPPORTED = new Set([
  'aarch64-apple-darwin',
  'x86_64-apple-darwin',
  'x86_64-unknown-linux-gnu',
]);

function reply(status: number, body: string, contentType = 'application/json') {
  return { statusCode: status, headers: { 'content-type': contentType }, body };
}

async function s3Text(key: string): Promise<string> {
  const out = await s3.send(new GetObjectCommand({ Bucket: BUCKET, Key: key }));
  return await out.Body!.transformToString();
}

// Newline list, each line "token" or "token = label"; blank lines and # comments ignored.
async function allowedTokens(): Promise<Set<string>> {
  let raw = '';
  try {
    const out = await ssm.send(new GetParameterCommand({ Name: SSM_TOKENS_PARAM, WithDecryption: true }));
    raw = out.Parameter?.Value || '';
  } catch {
    // No allowlist configured yet: treat as empty, so every token is rejected (403), never a 500.
    return new Set<string>();
  }
  const set = new Set<string>();
  for (const line of raw.split(/\r?\n/)) {
    const t = line.trim();
    if (!t || t.startsWith('#')) continue;
    set.add(t.split(/[\s=]/)[0]);
  }
  return set;
}

function tokenValid(token: string, allowed: Set<string>): boolean {
  // Length-independent membership: never branch on a prefix match.
  let ok = false;
  for (const a of allowed) if (a.length === token.length && a === token) ok = true;
  return ok && token.length > 0;
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
      try {
        script = await s3Text(name);
      } catch {
        return reply(404, `${name} not published yet`, 'text/plain');
      }
      script = script.split('__CTX_ENDPOINT__').join(endpoint);
      const ct = name.endsWith('.ps1') ? 'text/plain' : 'text/x-shellscript';
      return reply(200, script, ct);
    }

    if (method === 'POST') {
      let body: any = {};
      try {
        const raw = event.isBase64Encoded ? Buffer.from(event.body || '', 'base64').toString() : event.body || '{}';
        body = JSON.parse(raw);
      } catch {
        return reply(400, JSON.stringify({ error: 'bad json' }));
      }
      const token = String(body.token || '').trim();
      const target = String(body.target || '').trim();

      const allowed = await allowedTokens();
      if (!tokenValid(token, allowed)) return reply(403, JSON.stringify({ error: 'invalid or revoked token' }));
      if (!SUPPORTED.has(target)) return reply(404, JSON.stringify({ error: `unsupported target: ${target}` }));

      let manifest: any;
      try {
        manifest = JSON.parse(await s3Text('manifest/latest.json'));
      } catch {
        return reply(503, JSON.stringify({ error: 'no release published yet' }));
      }
      const entry = manifest?.targets?.[target];
      if (!entry?.file) return reply(404, JSON.stringify({ error: `no build for ${target} in ${manifest.version}` }));

      const url = await getSignedUrl(
        s3,
        new GetObjectCommand({ Bucket: BUCKET, Key: entry.file }),
        { expiresIn: PRESIGN_TTL },
      );
      // A server-side install line lands in CloudWatch. It counts our own installs; it is not user telemetry.
      console.log(JSON.stringify({ event: 'install', target, version: manifest.version }));
      return reply(200, JSON.stringify({ version: manifest.version, target, url, sha256: entry.sha256 }));
    }

    return reply(405, JSON.stringify({ error: 'method not allowed' }));
  } catch (e: any) {
    console.error('handler error', e);
    return reply(500, JSON.stringify({ error: 'server error' }));
  }
};
