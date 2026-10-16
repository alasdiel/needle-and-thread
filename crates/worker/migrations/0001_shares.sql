-- Shares and their comments (DESIGN §10). The text itself is in KV. Nothing here can be read
-- without a link's key: comment bodies are sealed, and secrets are kept only as their hashes.

CREATE TABLE IF NOT EXISTS shares (
  id TEXT PRIMARY KEY,
  -- SHA-256 of the author key, which uploads, revokes and moderates.
  author TEXT NOT NULL,
  -- SHA-256 of the comment token, which anyone with the link can derive.
  commenter TEXT NOT NULL,
  updated REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS comments (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  share TEXT NOT NULL,
  -- Sealed with the link's key, base64url.
  body TEXT NOT NULL,
  -- Posted with the author key.
  author INTEGER NOT NULL DEFAULT 0,
  resolved INTEGER NOT NULL DEFAULT 0,
  -- Kept as a row with no body, so readers polling with `since` learn it went.
  deleted INTEGER NOT NULL DEFAULT 0,
  created REAL NOT NULL,
  updated REAL NOT NULL
);

CREATE INDEX IF NOT EXISTS comments_by_share ON comments (share, updated);
