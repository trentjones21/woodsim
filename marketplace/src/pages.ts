// The marketplace's pages: the list of plans, and a plan's page as the game drew it up, under
// the title it was put up with.

/** A plan as the list shows it. */
export interface Listed {
  id: string;
  title: string;
  created_at: string;
}

/** Pages carry no script: a plan page is the game's own drawing, and titles are people's. */
const HEADERS = {
  "content-type": "text/html; charset=utf-8",
  "content-security-policy":
    "default-src 'none'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
  "x-content-type-options": "nosniff",
  "referrer-policy": "strict-origin-when-cross-origin",
};

const STYLE = `
:root { color: #1d2026; }
body { font: 14px/1.45 -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif; background: #fff;
  max-width: 7.5in; margin: 32px auto; padding: 0 16px; }
h1 { font-size: 26px; margin: 0 0 4px; }
.sub { color: #555; margin: 0 0 24px; }
ul { list-style: none; padding: 0; margin: 0; }
li { display: flex; justify-content: space-between; gap: 16px; padding: 10px 0;
  border-bottom: 1px solid #e4e4e4; }
a { color: #1d2026; font-weight: 600; }
time { color: #555; flex: none; }
`;

/** The bar over a plan page that leads back to the list. It doesn't print. */
const SITE_BAR = `<nav class="site"><a href="/">Woodshop plans</a></nav>`;
const SITE_BAR_STYLE = `<style>
nav.site { margin: -16px 0 20px; padding-bottom: 8px; border-bottom: 1px solid #e4e4e4; }
nav.site a { color: #555; font-weight: 600; text-decoration: none; }
@media print { nav.site { display: none; } }
</style>`;

/** Every plan put up, newest first. */
export function listPage(plans: Listed[]): Response {
  const items = plans.length
    ? `<ul>${plans
        .map(
          (plan) =>
            `<li><a href="/p/${plan.id}">${escape(plan.title)}</a>` +
            `<time datetime="${escape(plan.created_at)}">${plan.created_at.slice(0, 10)}</time></li>`,
        )
        .join("")}</ul>`
    : `<p>Nothing's been put up yet.</p>`;
  const page = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Woodshop plans</title>
<style>${STYLE}</style>
</head>
<body>
<h1>Woodshop plans</h1>
<p class="sub">Plans drawn up in Woodshop Simulator, from every cut that made them.</p>
${items}
</body>
</html>`;
  return new Response(page, { headers: { ...HEADERS, "cache-control": "no-cache" } });
}

/**
 * A plan's page, `page` as the game drew it up, under `title`: the page's own title and
 * heading, and what a link to it shows, with a bar back to the list.
 */
export function planPage(page: ReadableStream, title: string): Response {
  const response = new Response(page, {
    headers: { ...HEADERS, "cache-control": "public, max-age=60" },
  });
  return new HTMLRewriter()
    .on("head", {
      element(head) {
        head.append(
          `<meta name="viewport" content="width=device-width, initial-scale=1">` +
            `<meta property="og:title" content="${escape(title)}">` +
            `<meta property="og:type" content="article">` +
            SITE_BAR_STYLE,
          { html: true },
        );
      },
    })
    .on("title", {
      element(element) {
        element.setInnerContent(title);
      },
    })
    .on("body", {
      element(body) {
        body.prepend(SITE_BAR, { html: true });
      },
    })
    .on("body > header > h1", {
      element(heading) {
        heading.setInnerContent(title);
      },
    })
    .transform(response);
}

export function notFound(): Response {
  return new Response("There's no such page.", {
    status: 404,
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
}

/** `text` as it shows in HTML, in an element or an attribute. */
function escape(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
