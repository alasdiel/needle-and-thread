// The Worker's rules, against `wrangler dev` (pnpm dev) or WORKER_URL: who may upload, read,
// comment, resolve, delete and revoke. The bodies are stand-ins; sealing is core's, tested there.
import { createHash, randomBytes } from 'node:crypto';

const base = process.env.WORKER_URL ?? 'http://localhost:8787';
let failures = 0;
const check = (what, ok, detail = '') => {
  if (!ok) failures++;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}${detail ? ` — ${detail}` : ''}`);
};
const b64 = (bytes) => Buffer.from(bytes).toString('base64url');
const fingerprint = (secret) => createHash('sha256').update(secret).digest('hex');

const id = b64(randomBytes(16));
const author = b64(randomBytes(32));
const token = b64(randomBytes(32));
const url = (path = '') => `${base}/s/${id}${path}`;
const call = (method, path, { key, comment, commenter, body, json } = {}) =>
  fetch(url(path), {
    method,
    headers: {
      ...(key && { Authorization: `Bearer ${key}` }),
      ...(comment && { 'X-Comment-Token': comment }),
      ...(commenter && { 'X-Commenter': commenter }),
      ...(json && { 'Content-Type': 'application/json' }),
    },
    body: json ? JSON.stringify(json) : body,
  });

// --- The text ---------------------------------------------------------------------------------
check('an unknown share is not found', (await call('GET')).status === 404);
check('a malformed id is not found', (await fetch(`${base}/s/nope`)).status === 404);
check('uploading needs the author key', (await call('PUT', '', { body: 'x' })).status === 401);
check('a new share needs its comment token’s hash', (await call('PUT', '', { key: author, body: 'x' })).status === 400);
check('the first upload makes the share', (await call('PUT', '', { key: author, commenter: fingerprint(token), body: 'sealed v1' })).status === 204);
check('anyone with the link reads it', (await (await call('GET')).text()) === 'sealed v1');
check('another key can’t replace it', (await call('PUT', '', { key: b64(randomBytes(32)), commenter: fingerprint(token), body: 'hijack' })).status === 403);
check('the author key can', (await call('PUT', '', { key: author, body: 'sealed v2' })).status === 204);
check('…and readers see the new version', (await (await call('GET')).text()) === 'sealed v2');
const cors = (await call('GET')).headers.get('access-control-allow-origin');
check('readers on another site may read it', cors === '*', cors);

// --- Comments ---------------------------------------------------------------------------------
check('commenting needs the link', (await call('POST', '/comments', { body: 'spam' })).status === 403);
check('…not just any token', (await call('POST', '/comments', { comment: 'guess', body: 'spam' })).status === 403);
const posted = await call('POST', '/comments', { comment: token, body: 'sealed comment' });
check('the comment token posts a comment', posted.status === 201);
const { id: first } = await posted.json();
const byAuthor = await call('POST', '/comments', { key: author, body: 'sealed reply' });
check('the author can comment too', byAuthor.status === 201);
let comments = await (await call('GET', '/comments')).json();
check('both are listed, oldest first', comments.length === 2 && comments[0].id === first, JSON.stringify(comments));
check('…bodies as they were sealed', Buffer.from(comments[0].body, 'base64url').toString() === 'sealed comment');
check('…and the author’s is marked', !comments[0].author && comments[1].author);
check('an empty comment is refused', (await call('POST', '/comments', { comment: token, body: '' })).status === 413);
check('a huge one too', (await call('POST', '/comments', { comment: token, body: 'x'.repeat(17000) })).status === 413);

const since = comments[1].updated;
check('only resolving needs the author key', (await call('PATCH', `/comments/${first}`, { comment: token, json: { resolved: true } })).status === 403);
check('the author resolves one', (await call('PATCH', `/comments/${first}`, { key: author, json: { resolved: true } })).status === 204);
comments = await (await call('GET', `/comments?since=${since}`)).json();
check('polling since a time brings only what changed', comments.length === 1 && comments[0].id === first && comments[0].resolved, JSON.stringify(comments));
check('a reader can’t delete', (await call('DELETE', `/comments/${first}`, { comment: token })).status === 403);
check('the author can', (await call('DELETE', `/comments/${first}`, { key: author })).status === 204);
comments = await (await call('GET', '/comments')).json();
const gone = comments.find((c) => c.id === first);
check('a deleted comment stays as an empty tombstone', gone?.deleted && gone.body === '', JSON.stringify(gone));

// --- Revoking ---------------------------------------------------------------------------------
check('only the author revokes', (await call('DELETE', '', { comment: token })).status === 403);
check('the author revokes', (await call('DELETE', '', { key: author })).status === 204);
check('then the text is gone', (await call('GET')).status === 404);
check('…and its comments', (await call('GET', '/comments')).status === 404);
check('…and nobody can comment', (await call('POST', '/comments', { comment: token, body: 'late' })).status === 404);

console.log(`\n${failures === 0 ? 'all passed' : `${failures} failed`} (worker)`);
process.exitCode = failures === 0 ? 0 : 1;
