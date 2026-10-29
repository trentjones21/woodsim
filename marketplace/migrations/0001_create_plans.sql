-- One row a plan put up. Its recipe and page are in R2 under plans/<id>/.
CREATE TABLE plans (
  -- Ten letters and digits, the plan's link: /p/<id>.
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  -- When it went up, as an ISO 8601 time in UTC.
  created_at TEXT NOT NULL,
  -- Which way of writing wood its recipe is in (woodsim's Recipe::VERSION when it went up).
  recipe_version INTEGER NOT NULL,
  -- The SHA-256 of the recipe as stored, in hex.
  recipe_sha256 TEXT NOT NULL,
  -- The SHA-256 of the token that takes it down, in hex; the token itself is only given out once.
  edit_token_sha256 TEXT NOT NULL,
  -- Who put it up, once there are accounts.
  owner_id TEXT
);

CREATE INDEX plans_by_created_at ON plans (created_at DESC);
