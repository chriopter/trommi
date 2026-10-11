//! The admin page: on its own listener, behind a password, read-only, and showing only what the hub can see.

mod common;

use common::*;
use serde_json::json;
use trommi_hub::util::{b64, random, short};

fn text(reply: &Reply) -> String {
    String::from_utf8_lossy(&reply.body).into_owned()
}

/// The page's own origin, as a browser names it on a POST from the page (the test client sends `host: 127.0.0.1`).
fn from_page() -> Vec<(&'static str, String)> {
    vec![
        ("origin", "http://127.0.0.1".to_string()),
        (
            "content-type",
            "application/x-www-form-urlencoded".to_string(),
        ),
    ]
}

/// The sign-in form sent as a browser sends it.
fn sign_in(port: u16, password: &str) -> Reply {
    let body = format!(
        "username=admin&password={}",
        password
            .bytes()
            .map(|b| match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => (b as char).to_string(),
                b' ' => "+".to_string(),
                b => format!("%{b:02X}"),
            })
            .collect::<String>()
    );
    request(port, "POST", "/login", &from_page(), body.as_bytes())
}

/// The cookie header a browser sends back after this answer.
fn session_of(reply: &Reply) -> Vec<(&'static str, String)> {
    let set = reply.header("set-cookie").expect("a cookie");
    vec![("cookie", set.split(';').next().unwrap().to_string())]
}

fn get(port: u16, headers: &[(&str, String)]) -> Reply {
    request(port, "GET", "/", headers, b"")
}

#[test]
fn the_admin_page_is_behind_its_password_and_shows_only_what_the_hub_sees() {
    // without a configured hash there is no page, whatever is asked
    let bare = TestHub::start();
    let port = bare.admin();
    assert_eq!(get(port, &[]).status, 404);
    assert_eq!(sign_in(port, "anything at all").status, 404);

    let hash = trommi_hub::admin::hash_password("correct horse battery");
    assert!(hash.starts_with("$argon2id$"));
    let mut w = World::on(TestHub::start_with(&[("HUB_ADMIN_PASSWORD_HASH", &hash)]));
    let hub = &w.hub;
    let admin = hub.admin();
    // a room with an account, an agent, a session and something written in it
    w.ada.post(hub, "/v1/account", &json!({
        "email": "ada@example.org",
        "kit": { "auth_key": b64(&random::<32>()), "sealed_copy": b64(&[&[2u8][..], &[7u8; 60]].concat()) },
        "password": { "auth_key": b64(&random::<32>()), "sealed_copy": b64(&[&[2u8][..], &[8u8; 60]].concat()), "kdf": { "alg": "argon2id", "v": 1, "m": 65536, "t": 3, "p": 1 } },
    })).ok();
    let mut bea = w.add_human();
    let mut agent = w.enrol_agent();
    w.catch_up(&mut bea, &w.room.clone());
    let (session, group) = w.found_main(&mut [&mut bea], Some(&mut agent));
    bea.send(
        &w.hub,
        &group,
        &chat(&session, agent.id(), "the words nobody but the room reads"),
    )
    .ok();
    let hub = &w.hub;

    // the public port knows nothing of it
    for path in ["/", "/login", "/admin"] {
        assert_eq!(request(hub.port, "GET", path, &[], b"").status, 404);
    }
    assert_eq!(sign_in(hub.port, "correct horse battery").status, 404);

    // without a session: a sign-in form a password manager fills, no Basic challenge, nothing of the hub
    let asked = get(admin, &[]);
    assert_eq!(asked.status, 200);
    assert_eq!(asked.header("www-authenticate"), None);
    assert_eq!(asked.header("set-cookie"), None);
    let form = text(&asked);
    assert!(form.contains("<form method=\"post\" action=\"/login\">"));
    assert!(form.contains("name=\"username\" value=\"admin\" autocomplete=\"username\""));
    assert!(form.contains("type=\"password\" name=\"password\" autocomplete=\"current-password\""));
    assert!(!form.contains("Rooms") && !form.contains("Sign out"));
    assert_eq!(asked.header("cache-control"), Some("no-store"));
    // (under `no-referrer` a browser would send its own sign-in with `Origin: null`)
    assert_eq!(asked.header("referrer-policy"), Some("same-origin"));
    let csp = asked.header("content-security-policy").unwrap();
    assert!(csp.contains("default-src 'none'") && csp.contains("form-action 'self'"));
    // scripts only from the hub's own files: no inline script, no handler, nothing from elsewhere
    assert!(csp.contains("script-src 'self';"));
    assert!(!csp.contains("unsafe-eval") && csp.matches("unsafe-inline").count() == 1 && csp.contains("style-src 'unsafe-inline'"));
    let js = request(admin, "GET", "/admin.js", &[], b"");
    assert_eq!(js.status, 200);
    assert_eq!(js.header("content-type"), Some("text/javascript; charset=utf-8"));
    assert_eq!(js.header("x-content-type-options"), Some("nosniff"));
    let js = text(&js);
    assert!(js.contains("'dblclick'") && js.contains("closest('.email')") && js.contains("toggle('shown')"));
    // "Show all" shows every address on the page at once, and blurs them all again
    assert!(js.contains("closest('.showall')") && js.contains("querySelectorAll('.email')") && js.contains("toggle('shown', on)"));
    assert!(!js.contains("fetch") && !js.contains("http"));
    // a cookie the hub did not give, or one of no use, is no session
    for value in [b64(&[5u8; 32]), "x".into(), String::new()] {
        let forged = [("cookie", format!("{}={value}", trommi_hub::admin::COOKIE))];
        assert!(!text(&get(admin, &forged)).contains("Rooms"));
    }
    // a Basic credential is nothing to this page
    let basic = [(
        "authorization",
        format!("Basic {}", "YWRtaW46Y29ycmVjdCBob3JzZSBiYXR0ZXJ5"),
    )];
    assert!(!text(&get(admin, &basic)).contains("Rooms"));

    // a sign-in another site starts is refused before the password is looked at, and does not count as wrong
    let foreign = [
        vec![("origin", "https://evil.example".to_string())],
        vec![("origin", "http://127.0.0.1.evil.example".to_string())],
        vec![("origin", "null".to_string())],
        vec![],
        vec![
            ("origin", "http://127.0.0.1".to_string()),
            ("sec-fetch-site", "cross-site".to_string()),
        ],
    ];
    for headers in &foreign {
        let refused = request(
            admin,
            "POST",
            "/login",
            headers,
            b"username=admin&password=correct+horse+battery",
        );
        assert_eq!(refused.status, 403, "{headers:?}");
        assert_eq!(refused.header("set-cookie"), None);
    }
    // behind a TLS proxy that names the host it was asked for
    let proxied = request(
        admin,
        "POST",
        "/login",
        &[
            ("origin", "https://hub.example.ts.net:8443".to_string()),
            ("x-forwarded-host", "hub.example.ts.net:8443".to_string()),
            ("sec-fetch-site", "same-origin".to_string()),
        ],
        b"username=admin&password=wrong",
    );
    assert_eq!(proxied.status, 401, "taken, and the password is wrong");
    hub.clock(1000);

    // wrong passwords: each makes the page wait longer before it takes another, the right one included
    let wrong = sign_in(admin, "wrong");
    assert_eq!(wrong.status, 401);
    assert!(text(&wrong).contains("Wrong password.") && text(&wrong).contains("current-password"));
    assert_eq!(wrong.header("set-cookie"), None);
    let waits = slowed(hub, 8, || sign_in(admin, "wrong"));
    assert!(
        waits.windows(2).all(|w| w[0] <= w[1]) && *waits.last().unwrap() <= 8,
        "{waits:?}"
    );
    let early = sign_in(admin, "correct horse battery");
    assert_eq!(early.status, 429);
    assert!(text(&early).contains("Too many attempts"));
    hub.clock(early.header("retry-after").unwrap().parse::<i64>().unwrap() * 1000);

    // the right password: a session cookie, and back to the page
    let signed = sign_in(admin, "correct horse battery");
    assert_eq!(signed.status, 303);
    assert_eq!(signed.header("location"), Some("/"));
    let cookie = signed.header("set-cookie").unwrap();
    for part in [
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Path=/",
        "Max-Age=43200",
    ] {
        assert!(cookie.split("; ").any(|p| p == part), "{part}: {cookie}");
    }
    assert!(cookie.starts_with(&format!("{}=", trommi_hub::admin::COOKIE)));
    let value = cookie.split(';').next().unwrap().split_once('=').unwrap().1;
    assert_eq!(trommi_hub::util::unb64(value).unwrap().len(), 32);
    let session = session_of(&signed);
    // each sign-in is a session of its own
    hub.clock(1000);
    let other = session_of(&sign_in(admin, "correct horse battery"));
    assert_ne!(other, session);

    // three pages behind the session: the overview, the accounts, the tables; each names the others
    let page = get(admin, &session);
    assert_eq!(page.status, 200);
    let html = text(&page);
    assert!(html.contains("<h1>Overview</h1>") && html.contains("version <code>dev</code>"));
    for nav in [
        "href=\"/\" class=\"on\"",
        "href=\"/accounts\"",
        "href=\"/tables\"",
    ] {
        assert!(html.contains(nav), "{nav}");
    }
    // the server's health and the hub's counts
    for tile in [
        "CPU",
        "Memory",
        "Disk",
        "Uptime",
        "Hub memory",
        "Database",
        "Sessions",
        "Streams",
        "Push",
    ] {
        assert!(
            html.contains(&format!("<div class=\"k\">{tile}</div>")),
            "{tile}"
        );
    }
    assert!(html.contains("<div class=\"k\">Rooms</div><div class=\"v\">1</div>"));
    assert!(html.contains("<div class=\"k\">Accounts</div><div class=\"v\">1</div>"));
    assert!(html.contains("<div class=\"k\">Sessions</div><div class=\"v\">1<small>live</small>"));
    assert!(html.contains("<a href=\"/tables/envelopes\">"));

    let accounts = request(admin, "GET", "/accounts", &session, b"");
    assert_eq!(accounts.status, 200);
    let accounts = text(&accounts);
    assert!(
        accounts.contains("<h1>Accounts</h1>")
            && accounts.contains("href=\"/accounts\" class=\"on\"")
    );
    // the e-mail address in full but blurred, shown by a double-click
    assert!(accounts.contains("<code class=\"email\" title=\"Double-click to show\">ada@example.org</code>"));
    // and a "Show all" at the top, not pressed: each address stays blurred until asked for
    assert_eq!(accounts.matches(trommi_hub::admin_view::SHOW_ALL).count(), 1);
    assert!(trommi_hub::admin_view::SHOW_ALL.contains("aria-pressed=\"false\">Show all<"));
    assert_eq!(
        trommi_hub::admin_view::blurred_email("a<b@example.com"),
        "<code class=\"email\" title=\"Double-click to show\">a&lt;b@example.com</code>"
    );
    // two human devices, one agent device, one live session, one envelope
    assert!(accounts.contains(&short(&w.room)));
    let row = accounts
        .split("<tr>")
        .find(|row| row.contains(&short(&w.room)))
        .unwrap();
    let numbers: Vec<&str> = row
        .split("<td class=\"n\">")
        .skip(1)
        .map(|cell| cell.split("</td>").next().unwrap())
        .collect();
    assert_eq!(&numbers[..5], ["2", "1", "0", "1", "0"], "{row}");
    assert_eq!(numbers[6], "1", "one envelope: {row}");

    let tables = request(admin, "GET", "/tables", &session, b"");
    assert_eq!(tables.status, 200);
    let tables = text(&tables);
    assert!(tables.contains("href=\"/tables\" class=\"on\""));
    // the side list: every table, each a link to its rows
    for (_, table, _, _) in trommi_hub::admin_view::TABLES {
        assert!(
            tables.contains(&format!(
                "<a href=\"/tables/{table}\"><code>{table}</code></a>"
            )),
            "{table}"
        );
        assert!(
            tables.contains(&format!("{table}</span><span class=\"count\">")),
            "{table} in the side list"
        );
    }
    assert_eq!(
        tables.matches("<a class=\"tbl").count(),
        trommi_hub::admin_view::TABLES.len()
    );
    assert!(
        !tables.contains("Not classified yet"),
        "every table and every column of the schema is in the page's lists"
    );
    // one table: its rows, credentials masked
    let devices = text(&request(admin, "GET", "/tables/devices", &session, b""));
    assert!(
        devices.contains("<a class=\"tbl on\" href=\"/tables/devices\"")
            && devices.contains("<code class=\"big\">devices</code>")
    );
    assert!(devices.contains("<td>human</td>") && devices.contains("<td>agent</td>"));
    // a name not on the list is no table, and never reaches the database
    for bad in ["sqlite_master", "rooms;DROP", "", "x"] {
        let path = format!("/tables/{bad}");
        assert_eq!(
            request(admin, "GET", &path, &session, b"").status,
            404,
            "{path}"
        );
    }
    // without a session the pages are the sign-in
    for path in ["/accounts", "/tables", "/tables/rooms", "/tables/rooms/1"] {
        let bare = request(admin, "GET", path, &[], b"");
        assert_eq!(bare.status, 303, "{path}");
        assert_eq!(bare.header("location"), Some("/"));
        assert_eq!(bare.header("set-cookie"), None);
        assert!(!text(&bare).contains("Sign out"));
    }

    // nothing of what was written, no key, no hash, no address in full on any page, nor in any table's rows
    let mut pages = vec![html.clone(), accounts.clone(), tables.clone()];
    for (_, table, _, _) in trommi_hub::admin_view::TABLES {
        pages.push(text(&request(
            admin,
            "GET",
            &format!("/tables/{table}"),
            &session,
            b"",
        )));
    }
    // (the overview and the accounts show ids shortened; a table's rows name each row by its whole key)
    for page in &pages[..3] {
        assert!(!page.contains(&trommi_hub::util::hex(&w.ada.id())));
        assert!(!page.contains(&trommi_hub::util::hex(&w.room)));
    }
    for page in &pages {
        assert!(!page.contains("nobody but the room"));
        // an address only ever blurred
        assert_eq!(
            page.matches("example.org").count(),
            page.matches("<code class=\"email\" title=\"Double-click to show\">ada@example.org</code>").count()
        );
        // the one script is the hub's own file
        assert_eq!(page.matches("<script").count(), 1);
        assert!(page.contains("<script src=\"/admin.js\" defer></script>"));
        assert!(!page.contains(" ondblclick") && !page.contains(" onclick"));
        assert!(!page.contains(&b64(&w.ada.id())) && !page.contains("argon2id$"));
        // the one form that sends something is the sign-out (a table's filter is a GET)
        assert_eq!(page.matches("<form method=\"post\"").count(), 1);
        assert!(page.contains("<form method=\"post\" action=\"/logout\">"));
    }
    if let Ok(dir) = std::env::var("TROMMI_ADMIN_PAGES") {
        // (the pages as files, for screenshots)
        for (name, page) in [
            ("overview", &html),
            ("accounts", &accounts),
            ("tables", &tables),
            ("tables-devices", &devices),
        ] {
            std::fs::write(format!("{dir}/{name}.html"), page).unwrap();
        }
    }
    for (method, path) in [
        ("POST", "/"),
        ("DELETE", "/"),
        ("POST", "/tables"),
        ("GET", "/login"),
        ("GET", "/rooms"),
        ("GET", "/logout"),
    ] {
        assert_eq!(
            request(admin, method, path, &session, b"").status,
            404,
            "{method} {path}"
        );
    }
    // a session is taken without the slow hash, also while wrong passwords hold the page back
    assert_eq!(sign_in(admin, "correct horse batterx").status, 401);
    assert_eq!(sign_in(admin, "correct horse battery").status, 429);
    assert!(text(&get(admin, &session)).contains("<h1>Overview</h1>"));
    // the longest password `admin-hash` takes fits into the form the page reads; a longer one is wrong unchecked
    let long = "p".repeat(trommi_hub::admin::MAX_PASSWORD);
    let roomy = TestHub::start_with(&[(
        "HUB_ADMIN_PASSWORD_HASH",
        &trommi_hub::admin::hash_password(&long),
    )]);
    let roomy_admin = roomy.admin();
    assert_eq!(sign_in(roomy_admin, &long).status, 303);
    assert_eq!(sign_in(roomy_admin, &format!("{long}p")).status, 401);
    // a body over the limit is no form
    let huge = format!(
        "password={}",
        "%41".repeat(trommi_hub::admin::MAX_PASSWORD * 2)
    );
    assert_eq!(
        request(roomy_admin, "POST", "/login", &from_page(), huge.as_bytes()).status,
        400
    );
    // the page is put together once for all who ask within a few seconds
    assert_eq!(text(&get(admin, &session)), html);
    // and no longer: five seconds on, the page is put together anew
    hub.clock(5001);
    let later = get(admin, &session);
    assert_eq!(later.status, 200);
    assert!(text(&later).contains("<h1>Overview</h1>"));

    // signing out: only from the page itself; then the session is gone, the other stays
    let mut cross = session.clone();
    cross.push(("origin", "https://evil.example".to_string()));
    assert_eq!(request(admin, "POST", "/logout", &cross, b"").status, 403);
    assert!(text(&get(admin, &session)).contains("Sign out"));
    let mut out = session.clone();
    out.extend(from_page());
    let left = request(admin, "POST", "/logout", &out, b"");
    assert_eq!(left.status, 303);
    assert_eq!(left.header("location"), Some("/"));
    assert!(left.header("set-cookie").unwrap().contains("Max-Age=0"));
    let after = get(admin, &session);
    assert!(!text(&after).contains("Rooms") && text(&after).contains("current-password"));
    assert!(text(&get(admin, &other)).contains("Sign out"));
    // a session ends after twelve hours
    hub.clock(12 * 3_600_000 + 1000);
    assert!(text(&get(admin, &other)).contains("current-password"));

    // few connections, none for long: the seventeenth is closed at once, and the hub's own port is not touched
    let held: Vec<std::net::TcpStream> = (0..trommi_hub::admin::MAX_CONNECTIONS + 4)
        .map(|_| std::net::TcpStream::connect(("127.0.0.1", admin)).unwrap())
        .collect();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let closed = held
        .iter()
        .filter(|socket| {
            use std::io::Read;
            socket
                .set_read_timeout(Some(std::time::Duration::from_millis(50)))
                .unwrap();
            matches!((&**socket).read(&mut [0u8; 1]), Ok(0))
        })
        .count();
    assert!(
        closed >= 4,
        "{closed} of the connections over the limit were closed"
    );
    assert_eq!(hub.get("/healthz").status, 200);
}

/// The `href` of the link with this label on a page, as a request path.
fn link(page: &str, label: &str) -> Option<String> {
    let end = page.find(&format!("\">{label}</a>"))?;
    let start = page[..end].rfind("href=\"")? + "href=\"".len();
    Some(page[start..end].replace("&amp;", "&"))
}

/// The keys of the rows listed on a table's page, in the order shown.
fn row_keys(page: &str, table: &str) -> Vec<String> {
    let mark = format!("<tr><td><a href=\"/tables/{table}/");
    page.split(&mark)
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().to_string())
        .collect()
}

#[test]
fn a_table_shows_its_rows_in_pages_with_its_credentials_masked() {
    let hash = trommi_hub::admin::hash_password("correct horse battery");
    let hub = TestHub::start_with(&[("HUB_ADMIN_PASSWORD_HASH", &hash)]);
    let admin = hub.admin();
    let session = session_of(&sign_in(admin, "correct horse battery"));
    let get = |path: &str| request(admin, "GET", path, &session, b"");
    let t0: i64 = 1_791_700_000_000;
    hub.app
        .db
        .write(|c| {
            // 120 push subscriptions: their endpoints and keys are credentials
            for i in 1..=120i64 {
                c.execute(
                    "INSERT INTO push_subscriptions (id, room_id, device, kind, endpoint, p256dh, auth, level, created_at)
                     VALUES (?1, ?2, ?3, 'web_push', ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        i,
                        vec![9u8; 32],
                        vec![i as u8; 32],
                        format!("https://push.example/secret-endpoint-{i}"),
                        vec![0xabu8; 65],
                        vec![0xcdu8; 16],
                        if i % 2 == 0 { "knocking" } else { "all" },
                        t0 + i * 1000
                    ],
                )?;
            }
            // an account: its e-mail masked, its login hashes never shown
            c.execute(
                "INSERT INTO accounts (account_id, email, created_at, updated_at, auth_salt, auth_hash, kdf, kit_salt, kit_hash, kit_copy, kit_form, user_handle)
                 VALUES (7, 'grace@example.net', ?1, ?1, ?2, ?3, '{\"alg\":\"argon2id\"}', ?2, ?3, ?4, 'email', ?5)",
                rusqlite::params![t0, vec![0x5au8; 16], vec![0xeeu8; 32], vec![0x77u8; 80], vec![1u8; 16]],
            )?;
            // a login throttle row: nothing in it is a credential, all of it shown
            c.execute(
                "INSERT INTO login_accounts (account, budget_start, budget_used, next_turn) VALUES (?1, ?2, 3, 4)",
                rusqlite::params![vec![0x42u8; 32], t0],
            )?;
            // 60 files (a table without rowid, keyed by room and file): equal times and missing ones, to page over
            for i in 0..60u8 {
                c.execute(
                    "INSERT INTO files (room_id, file_id, uploader, size, sha256, stored_at, referenced_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        vec![i % 3; 32],
                        vec![i; 16],
                        vec![1u8; 32],
                        1000 + i as i64,
                        vec![i; 32],
                        t0 + (i / 4) as i64,
                        if i % 5 == 0 { None } else { Some(t0 + (i % 7) as i64) }
                    ],
                )?;
            }
            Ok::<_, rusqlite::Error>(())
        })
        .unwrap();

    // the newest 50 first, the credentials masked, the times also as dates
    let first = get("/tables/push_subscriptions");
    assert_eq!(first.status, 200);
    let first = text(&first);
    let keys = row_keys(&first, "push_subscriptions");
    assert_eq!(keys.len(), 50);
    assert_eq!(keys[0], "120");
    assert_eq!(keys[49], "71");
    assert!(first.contains("120 rows"));
    assert!(!first.contains("secret-endpoint") && !first.contains("abababab") && !first.contains("cdcdcdcd"));
    assert!(first.contains("•••• (secret, 65 bytes)") && first.contains("•••• (secret, 16 bytes)"));
    assert!(first.contains("2026-10-") && first.contains(" UTC</span>"));
    assert!(first.contains("<td>knocking</td>"));
    // the columns folded at the top, then what the hub reads, then which columns are not in full, then the rows
    let at = |what: &str| first.find(what).unwrap_or_else(|| panic!("{what}"));
    assert!(at("<details class=\"card cols-card\">") < at("The hub reads:"));
    assert!(at("The hub reads:") < at("Not in full:") && at("Not in full:") < at("<table class=\"grid rows\">"));
    assert!(first.contains("Not in full: <code>endpoint, p256dh, auth, apns_key</code> (credentials, shown by their length only)."));
    assert!(first.contains("<li><span class=\"secret\" title=\"credential: not shown\">••</span><code>endpoint</code>"));
    // the side list: names and counts only, each a link to its rows
    assert!(!first.contains("<ul class=\"cols-list") && !first.contains("Open push_subscriptions"));
    // no e-mail address on the page, no "Show all"
    assert!(!first.contains("class=\"showall\""));
    assert!(first.contains("<a class=\"tbl on\" href=\"/tables/push_subscriptions\" aria-current=\"page\">"));
    for group in ["Accounts and sign-in", "Rooms and devices", "Groups (MLS)", "Keys and recovery", "Content", "Files and shares", "Push", "Hub upkeep"] {
        assert!(first.contains(&format!("<div class=\"grp\">{group}</div>")), "{group}");
    }
    let throttle = text(&get("/tables/login_accounts"));
    assert_eq!(row_keys(&throttle, "login_accounts").len(), 1);
    assert!(throttle.contains("Every column is shown in full.") && throttle.contains(&trommi_hub::util::hex(&[0x42u8; 16])));
    // a byte string by its length and first 16 bytes
    assert!(first.contains(&format!("32 bytes</span> <code>{}</code>", trommi_hub::util::hex(&[9u8; 16]))));
    assert_eq!(link(&first, "← Previous"), None);
    // on, and back
    let second = text(&get(&link(&first, "Next →").unwrap()));
    let keys2 = row_keys(&second, "push_subscriptions");
    assert_eq!((keys2[0].as_str(), keys2[49].as_str()), ("70", "21"));
    let third = text(&get(&link(&second, "Next →").unwrap()));
    let keys3 = row_keys(&third, "push_subscriptions");
    assert_eq!((keys3.len(), keys3[0].as_str(), keys3[19].as_str()), (20, "20", "1"));
    assert_eq!(link(&third, "Next →"), None);
    let back = text(&get(&link(&third, "← Previous").unwrap()));
    assert_eq!(row_keys(&back, "push_subscriptions"), keys2);
    let back = text(&get(&link(&back, "← Previous").unwrap()));
    assert_eq!(row_keys(&back, "push_subscriptions"), keys);

    // a filter: one column, one value
    let knocking = text(&get("/tables/push_subscriptions?f=level&v=knocking"));
    assert!(knocking.contains("60 rows where the filter holds"));
    assert!(!knocking.contains("<td>all</td>"));
    let keys = row_keys(&knocking, "push_subscriptions");
    assert_eq!((keys[0].as_str(), keys.len()), ("120", 50));
    let rest = text(&get(&link(&knocking, "Next →").unwrap()));
    assert_eq!(row_keys(&rest, "push_subscriptions").len(), 10);
    let one = text(&get(&format!("/tables/push_subscriptions?f=device&v={}", trommi_hub::util::hex(&[5u8; 32]))));
    assert_eq!(row_keys(&one, "push_subscriptions"), ["5"]);
    assert!(text(&get("/tables/push_subscriptions?f=id&v=x")).contains("id takes a whole number."));
    // a secret column can be neither searched nor sorted by
    let probe = text(&get("/tables/push_subscriptions?f=endpoint&v=https%3A%2F%2Fpush.example%2Fsecret-endpoint-5"));
    assert!(probe.contains("endpoint cannot be filtered by") && probe.contains("120 rows"));
    let sorted = text(&get("/tables/push_subscriptions?s=auth"));
    assert!(sorted.contains("Not sorted by auth"));
    assert_eq!(row_keys(&sorted, "push_subscriptions")[0], "120");
    assert!(!probe.contains("<option>endpoint</option>") && probe.contains("<option>level</option>"));
    // sorted by a column, the other way round
    let oldest = text(&get("/tables/push_subscriptions?s=created_at&d=asc"));
    assert_eq!(row_keys(&oldest, "push_subscriptions")[0], "1");
    assert!(oldest.contains("created_at ▲"));

    // one row, every value in full but the secrets
    let row = get("/tables/push_subscriptions/5");
    assert_eq!(row.status, 200);
    let row = text(&row);
    assert!(row.contains(&trommi_hub::util::hex(&[5u8; 32])) && row.contains("<span class=\"text\">all</span>"));
    assert!(!row.contains("secret-endpoint") && !row.contains("abababab") && row.contains("•••• (secret, 65 bytes)"));
    assert!(row.contains("rows with this value"));
    for missing in ["/tables/push_subscriptions/999", "/tables/push_subscriptions/x1", "/tables/push_subscriptions/1;2", "/tables/push_subscriptions/", "/tables/nope", "/tables/nope/1"] {
        assert_eq!(get(missing).status, 404, "{missing}");
    }

    // the account: the e-mail blurred, the login hashes and the KDF record never shown, the sealed copy by its length
    for path in ["/tables/accounts", "/tables/accounts/7"] {
        let page = text(&get(path));
        assert!(page.contains("<code class=\"email\" title=\"Double-click to show\">grace@example.net</code>"), "{path}");
        assert_eq!(page.matches(trommi_hub::admin_view::SHOW_ALL).count(), 1, "{path}");
        assert_eq!(page.matches("grace").count(), 1, "{path}");
        assert!(!page.contains("eeeeeeee") && !page.contains("5a5a5a5a") && !page.contains("argon2id"), "{path}");
        assert!(page.contains("80 bytes · <code>77777777"), "{path}");
    }

    // a table without rowid: every row once, page by page, in either order, sorted by a column with nulls
    let walk = |start: &str, label: &str| {
        let mut seen = Vec::new();
        let mut path = Some(start.to_string());
        while let Some(p) = path {
            let page = text(&get(&p));
            let mut keys = row_keys(&page, "files");
            if label == "← Previous" {
                keys.reverse();
            }
            seen.extend(keys);
            path = link(&page, label);
        }
        seen
    };
    for sort in ["", "?s=referenced_at&d=asc", "?s=referenced_at", "?s=_row&d=asc", "?s=size"] {
        let down = walk(&format!("/tables/files{sort}"), "Next →");
        assert_eq!(down.len(), 60, "{sort}");
        let mut unique = down.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 60, "{sort}");
        // and back from the last page
        let last_page = {
            let mut p = format!("/tables/files{sort}");
            while let Some(next) = link(&text(&get(&p)), "Next →") {
                p = next;
            }
            p
        };
        let mut up = walk(&last_page, "← Previous");
        up.reverse();
        assert_eq!(up, down, "{sort}");
    }
    if let Ok(dir) = std::env::var("TROMMI_ADMIN_PAGES") {
        // (the pages as files, for screenshots)
        std::fs::write(format!("{dir}/rows.html"), &first).unwrap();
        std::fs::write(format!("{dir}/row.html"), text(&get("/tables/push_subscriptions/5"))).unwrap();
    }
    let key = row_keys(&text(&get("/tables/files")), "files")[0].clone();
    assert!(key.starts_with('x') && key.contains(".x"));
    assert_eq!(get(&format!("/tables/files/{key}")).status, 200);
}
