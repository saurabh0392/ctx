// Report intake. Two actions on one Function URL:
//   { action: "presign", images: [{ name, contentType }] }  -> presigned S3 POSTs for direct upload
//   { action: "submit",  report: {...}, imageKeys: [...] }   -> creates a GitHub issue with image links
//
// Every user-supplied string is untrusted (it later reaches a triage agent), so text is fenced and
// length-capped, content types are constrained, and the PAT never leaves the server (read from SSM).

import { SSMClient, GetParameterCommand } from '@aws-sdk/client-ssm';
import { S3Client } from '@aws-sdk/client-s3';
import { createPresignedPost } from '@aws-sdk/s3-presigned-post';
import { randomUUID } from 'crypto';

const REGION = process.env.AWS_REGION!;
const BUCKET = process.env.BUCKET!;
const REPO = process.env.GITHUB_REPO!;
const TOKEN_PARAM = process.env.SSM_TOKEN_PARAM!;
const MAX_IMAGES = parseInt(process.env.MAX_IMAGES || '25', 10);
const MAX_IMAGE_MB = parseInt(process.env.MAX_IMAGE_MB || '10', 10);

const ssm = new SSMClient({ region: REGION });
const s3 = new S3Client({ region: REGION });
let cachedToken: string | undefined;

// CORS is owned entirely by the Function URL config. Adding it here too produces duplicate
// Access-Control-Allow-Origin headers, which the browser rejects, so the handler stays out of it.
const json = (code: number, body: unknown) => ({ statusCode: code, headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });

const cap = (s: unknown, n: number) => String(s ?? '').slice(0, n);
const oneLine = (s: unknown, n: number) => cap(s, n).replace(/[\r\n]+/g, ' ').trim();
// Fence untrusted multi-line text into a code block, neutralizing any backticks so it cannot break out.
const fence = (s: unknown, n: number) => '```\n' + cap(s, n).replace(/```/g, "'''") + '\n```';

const LABELS: Record<string, string> = { bug: 'bug', coherence: 'coherence-regression', idea: 'enhancement' };

async function getToken(): Promise<string> {
  if (cachedToken) return cachedToken;
  const r = await ssm.send(new GetParameterCommand({ Name: TOKEN_PARAM, WithDecryption: true }));
  cachedToken = r.Parameter?.Value;
  if (!cachedToken) throw new Error('token not found in SSM');
  return cachedToken;
}

export const handler = async (event: any) => {
  let body: any;
  try { body = JSON.parse(event.body || '{}'); } catch { return json(400, { error: 'bad json' }); }

  try {
    if (body.action === 'presign') {
      const images: any[] = Array.isArray(body.images) ? body.images.slice(0, MAX_IMAGES) : [];
      const uploads = await Promise.all(images.map(async (img) => {
        const type = String(img.contentType || '');
        if (!type.startsWith('image/')) throw new Error('only images');
        const ext = (type.split('/')[1] || 'png').replace(/[^a-z0-9]/gi, '').slice(0, 5);
        const key = `images/${randomUUID()}.${ext}`;
        const presigned = await createPresignedPost(s3, {
          Bucket: BUCKET,
          Key: key,
          Conditions: [['content-length-range', 1, MAX_IMAGE_MB * 1024 * 1024], ['starts-with', '$Content-Type', 'image/']],
          Fields: { 'Content-Type': type },
          Expires: 300,
        });
        return { key, url: presigned.url, fields: presigned.fields };
      }));
      return json(200, { uploads });
    }

    if (body.action === 'submit') {
      const r = body.report || {};
      const title = oneLine(r.title, 140) || '(no title)';
      const kind = String(r.kind || 'bug');
      const keys: string[] = (Array.isArray(body.imageKeys) ? body.imageKeys : []).slice(0, MAX_IMAGES)
        .filter((k: any) => typeof k === 'string' && k.startsWith('images/') && /^[a-z0-9/_.-]+$/i.test(k));

      let md = cap(r.description, 4000) + '\n';
      if (r.example) md += `\n**Example**\n${fence(r.example, 6000)}\n`;
      if (r.bundle) md += `\n<details><summary>Diagnostic snapshot (counts only)</summary>\n\n${fence(JSON.stringify(r.bundle, null, 2), 8000)}\n</details>\n`;
      if (keys.length) md += `\n**Screenshots**\n` + keys.map((k) => `![screenshot](https://${BUCKET}.s3.${REGION}.amazonaws.com/${k})`).join('\n') + '\n';
      md += `\n<sub>Filed via the ctx dashboard report modal.</sub>`;

      const token = await getToken();
      const res = await fetch(`https://api.github.com/repos/${REPO}/issues`, {
        method: 'POST',
        headers: { authorization: `Bearer ${token}`, accept: 'application/vnd.github+json', 'content-type': 'application/json', 'user-agent': 'ctx-report-intake' },
        body: JSON.stringify({ title, body: md, labels: ['alpha-report', LABELS[kind] || 'bug'] }),
      });
      if (!res.ok) return json(502, { error: `github ${res.status}`, detail: cap(await res.text(), 300) });
      const issue = await res.json() as any;
      return json(200, { url: issue.html_url, number: issue.number });
    }

    return json(400, { error: 'unknown action' });
  } catch (e: any) {
    return json(500, { error: cap(e?.message || 'error', 200) });
  }
};
