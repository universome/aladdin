#![allow(unused_must_use)]

use std::iter;
use std::fmt::Write;
use std::time::{Duration, Instant};
use std::collections::{VecDeque, HashMap};
use hyper::{Get, NotFound};
use hyper::server::{Server, Request, Response};
use hyper::uri::RequestUri::AbsolutePath;
use log::LogLevel;
use time;

use base::logger;
use base::config::CONFIG;
use arbitrer::{self, Bookie, Events, MarkedOffer, OldCombo};

lazy_static! {
    static ref PORT: u16 = CONFIG.lookup("server.port")
        .unwrap().as_integer().unwrap() as u16;
}

pub fn run() {
    let mut server = Server::http(("0.0.0.0", *PORT)).unwrap();

    server.keep_alive(None);
    server.handle_threads(handle, 1).unwrap();
}

fn handle(req: Request, res: Response) {
    debug!("{} {}", req.method, req.uri);

    match req.uri {
        AbsolutePath(ref path) => match (&req.method, &path[..]) {
            (&Get, "/") => send_index(res),
            _ => send_404(res)
        },
        _ => send_404(res)
    };
}

fn send_404(mut res: Response) {
    *res.status_mut() = NotFound;
}

fn send_index(res: Response) {
    let now = Instant::now();
    let mut buffer = String::new();

    render_header(&mut buffer);

    {
        let history = logger::acquire_history();
        render_history(&mut buffer, &*history);
    }

    render_bookies(&mut buffer, &arbitrer::BOOKIES);

    {
        let combo_history = arbitrer::acquire_combo_history();
        render_combo_history(&mut buffer, &*combo_history);
    }

    {
        let events = arbitrer::acquire_events();
        render_events(&mut buffer, &*events);
    }

    render_footer(&mut buffer, now.elapsed());

    res.send(buffer.as_bytes());
}

fn render_header(b: &mut String) {
    b.push_str(r#"
<!DOCTYPE html>
<meta charset="utf-8">
<title>Aladdin</title>
<script src="http://ndossougbe.github.io/strapdown/dist/strapdown.js" defer></script>
<xmp style="display:none;" toc>
    "#);
}

fn render_history(b: &mut String, history: &VecDeque<logger::Message>) {
    if history.is_empty() {
        return;
    }

    writeln!(b, r#"
# Messages

<ul class="list-group">
    "#);

    for message in history.iter() {
        writeln!(b, r#"<li class="list-group-item {class}">
                           <span class="badge">{date}</span>"#,
                 class = match message.level {
                     LogLevel::Error => "list-group-item-danger",
                     LogLevel::Warn => "list-group-item-warning",
                     _ => ""
                 },
                 date = format_date(message.date, "%d/%m %R"));

        if message.count > 1 {
            writeln!(b, r#"<span class="badge">{count}</span>"#,
                     count = message.count);
        }

        writeln!(b, r#"`{module}` {data}</li>"#,
                 module = message.module,
                 data = message.data);
    }

    writeln!(b, r#"</ul>"#);
}

fn render_bookies(b: &mut String, bookies: &[Bookie]) {
    write!(b, "
# Bookies

| Host | Balance | Active |
| ---- | -------:|:------:|
    ");

    for bookie in bookies {
        writeln!(b, "|{host}|{balance}|{active}|",
                 host = bookie.host,
                 balance = bookie.balance(),
                 active = if bookie.active() { '✓' } else { ' ' });
    }
}

fn render_combo_history(b: &mut String, combo_history: &VecDeque<OldCombo>) {
    if combo_history.is_empty() {
        return;
    }

    writeln!(b, "# Recent combos");

    for combo in combo_history {
        writeln!(b, "|`[{date}]`|`{start}`|{kind:?}|||",
                 date = format_date(combo.date, "%d/%m %R"),
                 start = format_date(combo.head.date, "%d/%m %R"),
                 kind = combo.head.kind);

        writeln!(b, "|-|-|:-:|-|-|");

        for bet in &combo.bets {
            writeln!(b, "|{team} `{odds:.2}`|{host}|{size}|{profit:+.1}%|",
                     team = bet.outcome.0,
                     odds = bet.outcome.1,
                     host = bet.bookie.host,
                     size = bet.size,
                     profit = bet.profit * 100.);
        }

        writeln!(b, "");
    }
}

fn render_events(b: &mut String, events: &Events) {
    if events.is_empty() {
        return;
    }

    writeln!(b, "# Events");

    let mut groups = HashMap::new();

    for (offer, event) in events {
        let vec = groups.entry(offer.kind.clone()).or_insert_with(Vec::new);
        vec.push(event);
    }

    for (kind, mut events) in groups {
        writeln!(b, "## {:?}", kind);

        events.sort_by_key(|event| event[0].1.date);

        for event in events {
            let outcome_count = event[0].1.outcomes.len();

            writeln!(b, "{}", iter::repeat('|').take(outcome_count + 4).collect::<String>());
            writeln!(b, "|{}", iter::repeat("---|").take(outcome_count + 3).collect::<String>());

            for &MarkedOffer(bookie, ref offer) in event {
                write!(b, "|`{date}`|{host}|#{inner_id}|",
                       date = format_date(offer.date, "%d/%m %R"),
                       host = bookie.host,
                       inner_id = offer.inner_id);

                for outcome in &offer.outcomes {
                    write!(b, "{outcome} `{odds:.2}`|",
                           outcome = outcome.0,
                           odds = outcome.1);
                }

                writeln!(b, "");
            }

            writeln!(b, "");
        }
    }
}

fn render_footer(b: &mut String, spent: Duration) {
    let ms = spent.as_secs() as u32 * 1_000 + spent.subsec_nanos() / 1_000_000;
    writeln!(b, "---");
    writeln!(b, "> Rendered in `{}ms`", ms);
    write!(b, "</xmp>");
}

fn format_date(date: u32, format: &str) -> String {
    let tm = time::at_utc(time::Timespec::new(date as i64, 0)).to_local();
    time::strftime(format, &tm).unwrap()
}
