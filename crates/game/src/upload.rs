//! Putting plans up on the marketplace (`marketplace/`), to share at a link. Share at the bench
//! asks what to call the glue-up Plans would draw up (`Sharing::Naming`: a card with a title to
//! type), and putting it up sends that title and its recipe, every board's whole history, to
//! the marketplace from a thread of its own, so the game keeps going (`Sharing::PuttingUp`).
//! The marketplace draws the plans up itself, from the recipe. Once they're up, the link goes
//! on the clipboard and opens in the browser, and it's kept with the token that takes the plans
//! down again, as the next free `uploads/upload-N.txt`.
//!
//! The marketplace is the deployed one unless `WOODSHOP_MARKETPLACE` says another
//! (`http://localhost:8787` for `wrangler dev`), and until there are accounts it takes an
//! upload token: `WOODSHOP_UPLOAD_TOKEN`, or the one in `marketplace/.upload-token`.

use std::{
    env, fs, io, mem,
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use bevy::{prelude::*, text::EditableText};
use woodsim::{Assembly, Recipe, Wood};

use crate::{
    bench,
    controls::{Action, Actions},
    copy::{Notice, to_clipboard},
    hand::describe_glue_up,
    join::BenchStation,
};

/// The marketplace as it's deployed.
const DEPLOYED: &str = "https://woodshop-marketplace.cloudflareos-7a7053.workers.dev";
/// Where the marketplace's upload token is kept, from where the game runs.
const TOKEN_FILE: &str = "marketplace/.upload-token";
/// Where the links to plans put up are kept, from where the game runs.
const FOLDER: &str = "uploads";
/// The longest a title can be, as the marketplace takes them.
pub const MOST_TITLE: usize = 120;
/// How long putting plans up can take before it's given up on.
#[cfg(not(target_arch = "wasm32"))]
const TIMEOUT: Duration = Duration::from_secs(60);
/// How long the status says how it went, wherever you are, in seconds.
const NOTICE_SECONDS: f64 = 8.0;

pub struct UploadPlugin;

impl Plugin for UploadPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sharing>()
            .add_systems(Update, (start_putting_up, finish_putting_up).chain());
    }
}

/// A marketplace to put plans up on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marketplace {
    /// Where it is, with no `/` on the end.
    pub url: String,
    /// The token it takes to put plans up, if there's one to give it.
    pub token: Option<String>,
}

impl Marketplace {
    /// The marketplace `WOODSHOP_MARKETPLACE` says, or the deployed one, with the token
    /// `WOODSHOP_UPLOAD_TOKEN` says, or `marketplace/.upload-token` holds.
    pub fn from_env() -> Marketplace {
        let url = env::var("WOODSHOP_MARKETPLACE").unwrap_or_else(|_| DEPLOYED.to_string());
        let token = env::var("WOODSHOP_UPLOAD_TOKEN")
            .ok()
            .or_else(|| fs::read_to_string(TOKEN_FILE).ok())
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty());
        Marketplace {
            url: url.trim_end_matches('/').to_string(),
            token,
        }
    }

    /// Puts up plans called `title` for the wood `recipe` writes out, and says where they are,
    /// or why they aren't up, in a sentence.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn put_up(&self, title: &str, recipe: &str) -> Result<PutUp, String> {
        let Some(token) = &self.token else {
            return Err(format!(
                "Putting plans up takes the marketplace's upload token, in \
                 WOODSHOP_UPLOAD_TOKEN or {TOKEN_FILE}."
            ));
        };
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .build()
            .into();
        let mut response = agent
            .post(format!("{}/api/plans", self.url))
            .header("Authorization", format!("Bearer {token}"))
            .send_form([("title", title), ("recipe", recipe)])
            .map_err(|why| format!("Couldn't reach the marketplace at {}: {why}.", self.url))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|why| format!("The marketplace's answer didn't come through: {why}."))?;
        answer(status, &body)
    }

    /// On the web, plans go up from the game, not from a page.
    #[cfg(target_arch = "wasm32")]
    pub fn put_up(&self, _title: &str, _recipe: &str) -> Result<PutUp, String> {
        Err("Plans go up from the game.".to_string())
    }
}

/// Plans put up: where they are, and the token that takes them down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PutUp {
    pub url: String,
    pub token: String,
}

/// What the marketplace's answer says: where the plans are, or why they aren't up.
#[cfg(not(target_arch = "wasm32"))]
fn answer(status: u16, body: &str) -> Result<PutUp, String> {
    let json: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let field = |name: &str| {
        json.as_ref()
            .and_then(|json| json.get(name))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    match (status, field("url"), field("edit_token")) {
        (201, Some(url), Some(token)) => Ok(PutUp { url, token }),
        _ => {
            Err(field("error")
                .unwrap_or_else(|| format!("The marketplace answered with a {status}.")))
        }
    }
}

/// Keeps where plans called `title` went up on `marketplace`, and the token that takes them
/// down, as the next free `upload-N.txt` in `folder`, making it if it isn't there.
fn keep(put_up: &PutUp, title: &str, marketplace: &str, folder: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let path = (1..)
        .map(|n| folder.join(format!("upload-{n}.txt")))
        .find(|path| !path.exists())
        .expect("a free name");
    let id = put_up.url.rsplit('/').next().unwrap_or_default();
    fs::write(
        &path,
        format!(
            "# {title}, put up at {url}\n\
             # To take it down: curl -X DELETE -H \"Authorization: Bearer $(sed -n 's/^token //p' {path})\" \
             {marketplace}/api/plans/{id}\n\
             url {url}\n\
             token {token}\n",
            url = put_up.url,
            token = put_up.token,
            path = path.display(),
        ),
    )?;
    Ok(path)
}

/// Plans being put up on the marketplace, from the bench.
#[derive(Resource, Default)]
pub enum Sharing {
    #[default]
    Idle,
    /// The card's open, for a title to be typed: what's going up, and the title it starts with.
    Naming {
        glue_up: Assembly,
        suggested: String,
    },
    /// On its way, under `title`.
    PuttingUp {
        title: String,
        job: JoinHandle<Result<PutUp, String>>,
    },
}

/// The title the card starts with for `glue_up`, to type over: what it is, as the bench says.
fn suggested(glue_up: &Assembly) -> String {
    let what = match glue_up.members() {
        [member] => plans::describe_size(&member.board),
        _ => describe_glue_up(glue_up),
    };
    let mut letters = what.chars();
    letters.next().map_or(what.clone(), |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

impl Sharing {
    /// Opens the card to name `glue_up`'s plans before they go up, unless some are already on
    /// their way.
    pub fn name(&mut self, glue_up: Assembly) {
        if !self.putting_up() {
            let suggested = suggested(&glue_up);
            *self = Sharing::Naming { glue_up, suggested };
        }
    }

    /// The title the card starts with, while it's open.
    pub fn naming(&self) -> Option<&str> {
        match self {
            Sharing::Naming { suggested, .. } => Some(suggested),
            _ => None,
        }
    }

    pub fn putting_up(&self) -> bool {
        matches!(self, Sharing::PuttingUp { .. })
    }

    /// Sends the plans being named up under `title` (or the title the card started with, if
    /// it's been left empty) to `marketplace`, from a thread of their own, and says so.
    fn put_up(&mut self, title: &str, marketplace: Marketplace) -> Option<String> {
        let Sharing::Naming { glue_up, suggested } = mem::take(self) else {
            return None;
        };
        let title = match title.split_whitespace().collect::<Vec<_>>().join(" ") {
            typed if typed.is_empty() => suggested,
            typed => typed,
        };
        let recipe = Recipe {
            wood: Wood::GlueUp(glue_up),
            saw: None,
        }
        .write();
        let going = "Putting the plans up on the marketplace.".to_string();
        let named = title.clone();
        let job = thread::spawn(move || marketplace.put_up(&named, &recipe));
        *self = Sharing::PuttingUp { title, job };
        Some(going)
    }

    /// If the plans on their way have gone up, or failed to, what came of it: their title, and
    /// where they are or why they aren't up.
    fn finished(&mut self) -> Option<(String, Result<PutUp, String>)> {
        if !matches!(self, Sharing::PuttingUp { job, .. } if job.is_finished()) {
            return None;
        }
        let Sharing::PuttingUp { title, job } = mem::take(self) else {
            unreachable!("checked just now");
        };
        let came = job
            .join()
            .unwrap_or_else(|_| Err("Putting them up went wrong.".to_string()));
        Some((title, came))
    }
}

/// The text box on the card the title's typed into.
#[derive(Component)]
pub struct TitleBox;

/// Starts putting the named plans up when asked, with the title typed, or closes the card.
fn start_putting_up(
    actions: Res<Actions>,
    titles: Query<&EditableText, With<TitleBox>>,
    mut sharing: ResMut<Sharing>,
    mut bench: ResMut<BenchStation>,
) {
    for action in actions.started() {
        match action {
            Action::PutUp => {
                let typed = titles
                    .iter()
                    .next()
                    .map(|title| title.value().to_string())
                    .unwrap_or_default();
                if let Some(going) = sharing.put_up(&typed, Marketplace::from_env()) {
                    bench.note(going);
                }
            }
            Action::StopSharing if sharing.naming().is_some() => *sharing = Sharing::Idle,
            _ => {}
        }
    }
}

/// Once plans have gone up, puts their link on the clipboard, opens it, keeps it with the
/// token that takes them down, and says so; or says why they didn't go up.
fn finish_putting_up(
    time: Res<Time>,
    mut sharing: ResMut<Sharing>,
    mut bench: ResMut<BenchStation>,
    mut notice: ResMut<Notice>,
) {
    let Some((title, came)) = sharing.finished() else {
        return;
    };
    let said = match came {
        Ok(put_up) => {
            let marketplace = Marketplace::from_env().url;
            let kept = keep(&put_up, &title, &marketplace, Path::new(FOLDER));
            let copied = to_clipboard(&put_up.url).is_ok();
            let _ = bench::open_in_browser(&put_up.url);
            let link = if copied {
                "The link's on the clipboard"
            } else {
                "Couldn't reach the clipboard"
            };
            match kept {
                Ok(path) => format!(
                    "The plans are up at {}. {link}, and the token that takes them down is in {}.",
                    put_up.url,
                    path.display()
                ),
                Err(why) => format!(
                    "The plans are up at {}. {link}, but the token that takes them down couldn't \
                     be kept ({why}): it's {}.",
                    put_up.url, put_up.token
                ),
            }
        }
        Err(why) => format!("The plans didn't go up. {why}"),
    };
    bench.note(said.clone());
    notice.0 = Some((said, time.elapsed_secs_f64() + NOTICE_SECONDS));
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
    };

    use woodsim::{Board, Length};

    use super::*;

    fn board() -> Assembly {
        Assembly::of(Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        ))
    }

    /// A marketplace on this machine that answers one request with `status` and `body`, and
    /// hands back the request it got: its first line, headers, and body.
    fn marketplace(status: u16, body: &'static str) -> (Marketplace, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let served = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut request = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                request.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body_read = vec![0; length];
            reader.read_exact(&mut body_read).unwrap();
            request.push_str(&String::from_utf8(body_read).unwrap());
            let answer = format!(
                "HTTP/1.1 {status} Whatever\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            reader.get_mut().write_all(answer.as_bytes()).unwrap();
            request
        });
        let marketplace = Marketplace {
            url,
            token: Some("the-token".to_string()),
        };
        (marketplace, served)
    }

    #[test]
    fn plans_go_up_as_a_form_with_the_token() {
        let (marketplace, served) = marketplace(
            201,
            r#"{"id":"abcdefghjk","url":"https://example.test/p/abcdefghjk","edit_token":"down"}"#,
        );
        let recipe = Recipe {
            wood: Wood::GlueUp(board()),
            saw: None,
        }
        .write();
        let put_up = marketplace.put_up("A shelf & more", &recipe).unwrap();
        assert_eq!(
            put_up,
            PutUp {
                url: "https://example.test/p/abcdefghjk".to_string(),
                token: "down".to_string(),
            }
        );
        let request = served.join().unwrap();
        assert!(
            request.starts_with("POST /api/plans HTTP/1.1\r\n"),
            "{request}"
        );
        let lower = request.to_ascii_lowercase();
        assert!(
            lower.contains("authorization: bearer the-token\r\n"),
            "{request}"
        );
        assert!(
            lower.contains("content-type: application/x-www-form-urlencoded"),
            "{request}"
        );
        let sent = format!(
            "title=A+shelf+%26+more&recipe=version+{}%0A",
            Recipe::VERSION
        );
        assert!(request.contains(&sent), "{request}");
    }

    #[test]
    fn the_marketplace_says_why_plans_dont_go_up() {
        let (marketplace, served) = marketplace(422, r#"{"error":"The recipe doesn't read."}"#);
        assert_eq!(
            marketplace.put_up("Shelf", "nonsense"),
            Err("The recipe doesn't read.".to_string())
        );
        served.join().unwrap();
        assert_eq!(
            answer(502, "<html>Bad gateway</html>"),
            Err("The marketplace answered with a 502.".to_string())
        );
        let without = Marketplace {
            token: None,
            ..Marketplace::from_env()
        };
        assert!(
            without
                .put_up("Shelf", "")
                .unwrap_err()
                .contains("upload token")
        );
    }

    /// Over the network, to the deployed marketplace, which turns a wrong token away before
    /// anything goes up: `cargo test -p game -- --ignored deployed`.
    #[test]
    #[ignore = "reaches the deployed marketplace"]
    fn the_deployed_marketplace_turns_a_wrong_token_away() {
        let marketplace = Marketplace {
            url: DEPLOYED.to_string(),
            token: Some("not-the-token".to_string()),
        };
        assert_eq!(
            marketplace.put_up("Shelf", "board s0\n"),
            Err("Putting plans up takes the marketplace's upload token.".to_string())
        );
    }

    #[test]
    fn plans_are_named_then_put_up_under_what_was_typed() {
        let mut sharing = Sharing::default();
        sharing.name(board());
        assert_eq!(sharing.naming(), Some("3/4\" x 3-1/2\" x 24\""));
        let (marketplace, served) = marketplace(
            201,
            r#"{"id":"abcdefghjk","url":"https://example.test/p/abcdefghjk","edit_token":"down"}"#,
        );
        let going = sharing.put_up("  A   shelf \n", marketplace);
        assert_eq!(
            going.as_deref(),
            Some("Putting the plans up on the marketplace.")
        );
        assert!(sharing.putting_up());
        // Another can't be named while one's on its way.
        sharing.name(board());
        assert_eq!(sharing.naming(), None);
        assert!(served.join().unwrap().contains("title=A+shelf&"));
        let (title, came) = loop {
            if let Some(finished) = sharing.finished() {
                break finished;
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(title, "A shelf");
        assert_eq!(came.unwrap().url, "https://example.test/p/abcdefghjk");
        assert!(matches!(sharing, Sharing::Idle));
    }

    #[test]
    fn a_title_left_empty_goes_up_as_it_started() {
        let mut sharing = Sharing::default();
        sharing.name(board());
        let (marketplace, served) = marketplace(201, r#"{"error":"whatever"}"#);
        sharing.put_up("   ", marketplace);
        assert!(
            served
                .join()
                .unwrap()
                .contains("title=3%2F4%22+x+3-1%2F2%22+x+24%22&")
        );
    }

    #[test]
    fn the_link_is_kept_with_the_token_that_takes_it_down() {
        let folder = std::env::temp_dir().join(format!("woodshop-uploads-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let put_up = PutUp {
            url: "https://example.test/p/abcdefghjk".to_string(),
            token: "down".to_string(),
        };
        let first = keep(&put_up, "A shelf", "https://example.test", &folder).unwrap();
        let second = keep(&put_up, "A shelf", "https://example.test", &folder).unwrap();
        assert_eq!(first.file_name().unwrap(), "upload-1.txt");
        assert_eq!(second.file_name().unwrap(), "upload-2.txt");
        let kept = fs::read_to_string(&first).unwrap();
        assert!(kept.starts_with("# A shelf, put up at https://example.test/p/abcdefghjk\n"));
        assert!(
            kept.contains("https://example.test/api/plans/abcdefghjk\n"),
            "{kept}"
        );
        assert!(kept.ends_with("url https://example.test/p/abcdefghjk\ntoken down\n"));
        fs::remove_dir_all(&folder).unwrap();
    }
}
