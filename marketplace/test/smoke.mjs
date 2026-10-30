// Checks a running marketplace end to end (`npm run dev` first, then `npm run smoke`): uploads
// are refused without the token or with a recipe that doesn't read, a pasted glue-up goes up
// and its page comes out under its title, escaped, with no way to run script, it's on the
// list, and it comes down only with its own token.
//
// MARKETPLACE_URL says where (http://localhost:8787 by default), and UPLOAD_TOKEN the token
// (from .dev.vars by default).

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const base = process.env.MARKETPLACE_URL ?? "http://localhost:8787";
const token =
  process.env.UPLOAD_TOKEN ??
  readFileSync(new URL("../.dev.vars", import.meta.url), "utf8").match(/^UPLOAD_TOKEN=(.+)$/m)[1];
const recipe = readFileSync(
  new URL("../../crates/woodsim/tests/pasted/microns-bridge.txt", import.meta.url),
  "utf8",
);

function upload(fields, bearer = token) {
  const form = new FormData();
  for (const [name, value] of Object.entries(fields)) {
    form.append(name, value);
  }
  return fetch(`${base}/api/plans`, {
    method: "POST",
    body: form,
    headers: bearer ? { authorization: `Bearer ${bearer}` } : {},
  });
}

let response = await upload({ title: "No token", recipe }, null);
assert.equal(response.status, 401, "an upload without the token");
response = await upload({ title: "Wrong token", recipe }, "not-the-token");
assert.equal(response.status, 401, "an upload with the wrong token");
response = await upload({ recipe });
assert.equal(response.status, 400, "an upload without a title");
response = await upload({ title: "Not wood", recipe: "stock s0 1 2 3\nboard s9\n" });
assert.equal(response.status, 422, "a recipe that doesn't read");
assert.match((await response.json()).error, /line 2: no board `s9`/);

const title = `Bridge <script>alert("hi")</script> & co`;
response = await upload({ title, recipe: new Blob([recipe], { type: "text/plain" }) });
assert.equal(response.status, 201, "a pasted glue-up");
const plan = await response.json();
assert.match(plan.url, /\/p\/[a-z0-9]{10}$/);

response = await fetch(plan.url);
assert.equal(response.status, 200, "its page");
assert.match(response.headers.get("content-security-policy"), /default-src 'none'/);
const page = await response.text();
// Quotes are left as they are in an element's text, where they can't end anything.
const escaped = 'Bridge &lt;script&gt;alert("hi")&lt;/script&gt; &amp; co';
assert.ok(page.includes(`<title>${escaped}</title>`), "its title, escaped");
assert.ok(page.includes(`<h1>${escaped}</h1>`), "its heading, escaped");
assert.ok(!page.includes("<script"), "no script on the page");
assert.ok(page.includes("<h2>Cut list</h2>"), "the plans themselves");

const list = await (await fetch(`${base}/`)).text();
assert.ok(list.includes(`href="/p/${plan.id}"`), "it's on the list");

const takeDown = (bearer) =>
  fetch(`${base}/api/plans/${plan.id}`, {
    method: "DELETE",
    headers: { authorization: `Bearer ${bearer}` },
  });
assert.equal((await takeDown(token)).status, 403, "taking it down with the upload token");
assert.equal((await takeDown(plan.edit_token)).status, 204, "taking it down with its own token");
assert.equal((await fetch(plan.url)).status, 404, "its page once it's down");
assert.equal((await takeDown(plan.edit_token)).status, 404, "taking it down again");

// The viewer's page, and its module from R2, compressed, once it's been put there.
response = await fetch(`${base}/viewer/`);
assert.equal(response.status, 200, "the viewer's page");
assert.match(await response.text(), /<canvas id="viewer">/);
response = await fetch(`${base}/viewer/viewer_bg.wasm`, { method: "HEAD" });
if (response.status === 404) {
  console.log("The viewer's module isn't in R2 yet (npm run viewer, then viewer:local or viewer:remote).");
} else {
  assert.equal(response.status, 200, "the viewer's module");
  assert.equal(response.headers.get("content-type"), "application/wasm");
}

console.log(`The marketplace at ${base} works.`);
