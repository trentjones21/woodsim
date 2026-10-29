// The marketplace's Worker. Plans go up as the game writes wood out, a woodsim recipe, and the
// Worker draws them up itself with the game's own Rust, so what's shown is always what the
// recipe makes and never HTML someone sent. Each plan's recipe and page are kept in R2, and
// its row in D1.
//
//   GET    /                the list of plans, newest first
//   POST   /api/plans       put plans up: a form with `title` and `recipe`, and the upload
//                           token as a bearer token; answers with the plan's link and the
//                           token that takes it down
//   GET    /p/<id>          a plan's page
//   DELETE /api/plans/<id>  take a plan down, with its token as a bearer token

import { drawUp } from "./plans";
import { type Listed, listPage, notFound, planPage } from "./pages";

/** The most a recipe can be; the largest the game has written is a few kilobytes. */
const MOST_RECIPE = 1 << 20;
/** The most an upload can be: a recipe, its title, and the form around them. */
const MOST_UPLOAD = MOST_RECIPE + (64 << 10);
const MOST_TITLE = 120;
/** A plan's id: this many of these, the letters and digits that can't be mistaken for others. */
const ID_LENGTH = 10;
const ID_LETTERS = "23456789abcdefghjkmnpqrstuvwxyz";
const ID = new RegExp(`^[${ID_LETTERS}]{${ID_LENGTH}}$`);

export default {
  async fetch(request, env): Promise<Response> {
    const url = new URL(request.url);
    const [, first, second, third, ...rest] = url.pathname.split("/");
    try {
      if (url.pathname === "/") {
        return reading(request) ? await list(env) : notAllowed("GET, HEAD");
      }
      if (first === "api" && second === "plans" && third === undefined) {
        return request.method === "POST" ? await putUp(request, env, url) : notAllowed("POST");
      }
      if (first === "api" && second === "plans" && ID.test(third) && rest.length === 0) {
        return request.method === "DELETE" ? await takeDown(request, env, third) : notAllowed("DELETE");
      }
      if (first === "p" && ID.test(second) && third === undefined) {
        return reading(request) ? await show(env, second) : notAllowed("GET, HEAD");
      }
      return notFound();
    } catch (error) {
      console.error(
        JSON.stringify({ message: "request failed", path: url.pathname, error: String(error) }),
      );
      return Response.json({ error: "Something went wrong on the marketplace's end." }, { status: 500 });
    }
  },
} satisfies ExportedHandler<Env>;

async function list(env: Env): Promise<Response> {
  const { results } = await env.DB.prepare(
    "SELECT id, title, created_at FROM plans ORDER BY created_at DESC LIMIT 100",
  ).all<Listed>();
  return listPage(results);
}

async function show(env: Env, id: string): Promise<Response> {
  const plan = await env.DB.prepare("SELECT title FROM plans WHERE id = ?")
    .bind(id)
    .first<{ title: string }>();
  // A plan still going up has its row but not yet its page.
  const page = plan && (await env.PLANS.get(`plans/${id}/plan.html`));
  return page ? planPage(page.body, plan.title) : notFound();
}

async function putUp(request: Request, env: Env, url: URL): Promise<Response> {
  if (!(await same(bearer(request), env.UPLOAD_TOKEN))) {
    return refuse(401, "Putting plans up takes the marketplace's upload token.");
  }
  const length = Number(request.headers.get("content-length"));
  if (!length) {
    return refuse(411, "Say how long the upload is, in Content-Length.");
  }
  if (length > MOST_UPLOAD) {
    return refuse(413, "That's too much to be a recipe.");
  }
  let form: FormData;
  try {
    form = await request.formData();
  } catch {
    return refuse(400, "Send the plans as a form (multipart/form-data) with a title and a recipe.");
  }
  const title = form.get("title");
  const recipe = form.get("recipe");
  const named = typeof title === "string" ? title.replace(/\s+/g, " ").trim() : "";
  if (!named) {
    return refuse(400, "Give the plans a title.");
  }
  if (named.length > MOST_TITLE) {
    return refuse(400, `Keep the title to ${MOST_TITLE} characters.`);
  }
  if (recipe === null) {
    return refuse(400, "Send the recipe the game wrote, as `recipe`.");
  }
  const text =
    typeof recipe === "string"
      ? new TextEncoder().encode(recipe)
      : new Uint8Array(await recipe.arrayBuffer());
  if (text.length > MOST_RECIPE) {
    return refuse(413, "That's too much to be a recipe.");
  }

  const drawn = drawUp(text);
  if ("why" in drawn) {
    return refuse(422, `The recipe doesn't read: ${drawn.why}.`);
  }

  // The row goes in first, so an id already taken is refused before anything's written over.
  const id = newId();
  const token = newToken();
  await env.DB.prepare(
    "INSERT INTO plans (id, title, created_at, recipe_version, recipe_sha256, edit_token_sha256) " +
      "VALUES (?, ?, ?, ?, ?, ?)",
  )
    .bind(
      id,
      named,
      new Date().toISOString(),
      drawn.recipeVersion,
      hex(await sha256(drawn.recipe)),
      hex(await sha256(token)),
    )
    .run();
  try {
    await Promise.all([
      env.PLANS.put(`plans/${id}/recipe.txt`, drawn.recipe, {
        httpMetadata: { contentType: "text/plain; charset=utf-8" },
      }),
      env.PLANS.put(`plans/${id}/plan.html`, drawn.page, {
        httpMetadata: { contentType: "text/html; charset=utf-8" },
      }),
    ]);
  } catch (error) {
    await env.DB.prepare("DELETE FROM plans WHERE id = ?").bind(id).run();
    throw error;
  }
  console.log(
    JSON.stringify({ message: "plans put up", id, recipe_bytes: text.length, page_bytes: drawn.page.length }),
  );
  return Response.json(
    { id, url: new URL(`/p/${id}`, url).href, edit_token: token },
    { status: 201 },
  );
}

async function takeDown(request: Request, env: Env, id: string): Promise<Response> {
  const plan = await env.DB.prepare("SELECT edit_token_sha256 FROM plans WHERE id = ?")
    .bind(id)
    .first<{ edit_token_sha256: string }>();
  if (!plan) {
    return refuse(404, "There's no such plan.");
  }
  const token = bearer(request);
  const matches =
    token !== null &&
    crypto.subtle.timingSafeEqual(await sha256(token), unhex(plan.edit_token_sha256));
  if (!matches) {
    return refuse(403, "Taking plans down takes the token they went up with.");
  }
  await env.DB.prepare("DELETE FROM plans WHERE id = ?").bind(id).run();
  await env.PLANS.delete([`plans/${id}/recipe.txt`, `plans/${id}/plan.html`]);
  console.log(JSON.stringify({ message: "plans taken down", id }));
  return new Response(null, { status: 204 });
}

function refuse(status: number, error: string): Response {
  return Response.json({ error }, { status });
}

/** Whether a request only reads a page: a GET, or a HEAD, which the runtime answers without the body. */
function reading(request: Request): boolean {
  return request.method === "GET" || request.method === "HEAD";
}

function notAllowed(method: string): Response {
  return new Response(null, { status: 405, headers: { allow: method } });
}

/** The bearer token a request carries, if any. */
function bearer(request: Request): string | null {
  const match = request.headers.get("authorization")?.match(/^Bearer (\S+)$/);
  return match ? match[1] : null;
}

/** Whether `given` is `expected`, without how long the check takes saying how close it came. */
async function same(given: string | null, expected: string | undefined): Promise<boolean> {
  if (given === null || !expected) {
    return false;
  }
  const [a, b] = await Promise.all([sha256(given), sha256(expected)]);
  return crypto.subtle.timingSafeEqual(a, b);
}

function newId(): string {
  // Bytes past the last whole run of the letters are skipped, so every letter's as likely.
  const most = 256 - (256 % ID_LETTERS.length);
  let id = "";
  while (id.length < ID_LENGTH) {
    for (const byte of crypto.getRandomValues(new Uint8Array(ID_LENGTH * 2))) {
      if (byte < most && id.length < ID_LENGTH) {
        id += ID_LETTERS[byte % ID_LETTERS.length];
      }
    }
  }
  return id;
}

/** 32 random bytes, in base64url. */
function newToken(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(32));
  return btoa(String.fromCharCode(...bytes))
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replace(/=+$/, "");
}

async function sha256(text: string): Promise<ArrayBuffer> {
  return crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
}

function hex(bytes: ArrayBuffer): string {
  return [...new Uint8Array(bytes)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function unhex(text: string): Uint8Array {
  return new Uint8Array(text.match(/../g)?.map((pair) => parseInt(pair, 16)) ?? []);
}
