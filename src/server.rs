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

use constants::{PORT, COMBO_COUNT};
use base::error::Result;
use base::logger;
use base::currency::Currency;
use arbitrer::{self, Bookie, BookieStage, Table, MarkedOffer};
use combo::{self, Combo};

lazy_static! {
    static ref START_DATE: u32 = time::get_time().sec as u32;
}

pub fn run() {
    *START_DATE;

    let mut server = Server::http(("0.0.0.0", PORT)).unwrap();

    server.keep_alive(None);
    server.set_read_timeout(Some(Duration::new(2, 0)));
    server.set_write_timeout(Some(Duration::new(5, 0)));
    server.handle_threads(handle, 1).unwrap();
}

fn handle(req: Request, res: Response) {
    debug!("{} {}", req.method, req.uri);

    let result = match req.uri {
        AbsolutePath(ref path) => match (&req.method, &path[..]) {
            (&Get, "/") => send_index(res),
            _ => send_404(res)
        },
        _ => send_404(res)
    };

    if let Err(error) = result {
        warn!("During response: {}", error);
    }
}

fn send_404(mut res: Response) -> Result<()> {
    *res.status_mut() = NotFound;
    Ok(())
}

fn send_index(res: Response) -> Result<()> {
    let now = Instant::now();
    let mut buffer = String::new();

    render_header(&mut buffer);

    {
        let history = logger::acquire_history();
        render_history(&mut buffer, &*history);
    }

    render_bookies(&mut buffer, &arbitrer::BOOKIES);

    let combos = combo::load_recent(COMBO_COUNT);
    render_combos(&mut buffer, &combos);

    render_table(&mut buffer, &arbitrer::TABLE);

    render_footer(&mut buffer, now.elapsed());

    res.send(buffer.as_bytes()).map_err(From::from)
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

| Host | Balance | Stage | Offers |
| ---- | -------:|:-----:| ------:|
    ");

    for bookie in bookies {
        let stage = match bookie.stage() {
            BookieStage::Initial => "".into(),
            BookieStage::Preparing => "⌚".into(),
            BookieStage::Running => "✓".into(),
            BookieStage::Aborted => "✗".into(),
            BookieStage::Sleeping(wakeup) => {
                let now = time::get_time().sec as u32;
                let delay = (wakeup - now) / 60;

                format!("{:02}:{:02}", delay / 60, delay % 60)
            }
        };

        writeln!(b, "|{host}|{balance}|{stage}|{offers}|",
                 host = bookie.host,
                 balance = bookie.balance(),
                 stage = stage,
                 offers = bookie.offer_count());
    }
}

fn render_combos(b: &mut String, combos: &[Combo]) {
    if combos.is_empty() {
        return;
    }

    writeln!(b, "# Recent combos");

    for combo in combos {
        let approx_expiry = combo.bets[0].expiry;

        writeln!(b, "|`[{date}]`|{game} {kind}|`{start_date}`|`{start_time}`|{sum}|",
                 date = format_date(combo.date, "%d/%m %R"),
                 game = combo.game,
                 kind = ""/*combo.kind*/,    // TODO(loyd): enable after nested.
                 start_date = format_date(approx_expiry, "%d/%m"),
                 start_time = format_date(approx_expiry, "%R"),
                 sum = combo.bets.iter().fold(Currency(0), |sum, bet| sum + bet.stake));

        writeln!(b, "|-|-|-:|:-:|-:|");

        for bet in &combo.bets {
            writeln!(b, "|{title} `{coef:.2}`|{host}|{stake}|{placed}|{profit:+.1}%|",
                     title = if let Some(ref s) = bet.title { s } else { "*draw*" },
                     coef = bet.coef,
                     host = bet.host,
                     stake = bet.stake,
                     placed = if bet.placed { ' ' } else { '✘' },
                     profit = bet.profit * 100.);
        }

        writeln!(b, "");
    }
}

fn render_table(b: &mut String, table: &Table) {
    let mut groups = HashMap::new();

    for market in table.iter() {
        let pair = (market[0].1.game.clone(), market[0].1.kind.clone());
        let vec = groups.entry(pair).or_insert_with(Vec::new);
        vec.push(market.to_vec());
    }

    if groups.is_empty() {
        return;
    }

    writeln!(b, "# Markets");

    for ((game, _kind), mut markets) in groups {
        //writeln!(b, "## {:?} [{:?}]", game, kind);  // TODO(loyd): enable after nested.
        writeln!(b, "## {:?}", game);

        markets.sort_by_key(|market| market[0].1.date);

        for market in markets {
            let outcome_count = market[0].1.outcomes.len();

            writeln!(b, "{}", iter::repeat('|').take(outcome_count + 4).collect::<String>());
            writeln!(b, "|{}", iter::repeat("---|").take(outcome_count + 3).collect::<String>());

            for &MarkedOffer(bookie, ref offer) in &*market {
                write!(b, "|`{date}`|{host}|#{oid}|",
                       date = format_date(offer.date, "%d/%m %R"),
                       host = bookie.host,
                       oid = offer.oid);

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
    writeln!(b, "> Rendered in `{}ms`\n", ms);
    writeln!(b, "> Started at `{}`", format_date(*START_DATE, "%d/%m %R"));
    write!(b, "</xmp>");
}

fn format_date(date: u32, format: &str) -> String {
    let tm = time::at_utc(time::Timespec::new(date as i64, 0)).to_local();
    time::strftime(format, &tm).unwrap()
}
