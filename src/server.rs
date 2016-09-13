#![allow(unused_must_use)]

use std::iter;
use std::fmt::Write;
use std::time::{Duration, Instant};
use std::collections::{VecDeque, HashMap};
use hyper::server::{Server, Request, Response};
use log::LogLevel;
use time;

use base::error::Result;
use base::logger;
use arbitrer::{self, State, MarkedOffer};

pub fn run() {
    Server::http("0.0.0.0:3000").unwrap()
        .handle_threads(move |_: Request, res: Response| {
            if let Err(error) = handle(res) {
                error!("{}", error);
            }
        }, 1).unwrap();
}

fn handle(res: Response) -> Result<()> {
    let now = Instant::now();
    let mut buffer = String::new();

    render_header(&mut buffer);

    {
        let messages = logger::acquire_messages();
        render_messages(&mut buffer, &*messages);
    }

    {
        let state = arbitrer::acquire_state();
        render_bookies(&mut buffer, &*state);
        render_events(&mut buffer, &*state);
    }

    render_footer(&mut buffer, now.elapsed());

    try!(res.send(buffer.as_bytes()));

    Ok(())
}

fn render_header(b: &mut String) {
    b.push_str(r#"
<!DOCTYPE html>
<meta charset="utf-8">
<title>Aladdin</title>
<script src="http://ndossougbe.github.io/strapdown/dist/strapdown.js" defer></script>
<style>
    td[align="right"] { text-align: right !important }
    td[align="center"] { text-align: center !important }
</style>
<xmp style="display:none;">
    "#);
}

fn render_messages(b: &mut String, messages: &VecDeque<logger::Message>) {
    if messages.is_empty() {
        return;
    }

    writeln!(b, r#"
# Messages

<ul class="list-group">
    "#);

    for message in messages.iter() {
        writeln!(b, r#"<li class="list-group-item {class}">
                           <span class="badge">{date}</span>
                           `{module}` {data}
                       </li>
                "#,
                 class = match message.level {
                     LogLevel::Error => "list-group-item-danger",
                     LogLevel::Warn => "list-group-item-warning",
                     _ => ""
                 },
                 date = format_date(message.date, "%d/%m %R"),
                 module = message.module,
                 data = message.data);
    }

    writeln!(b, r#"</ul>"#);
}

fn render_bookies(b: &mut String, state: &State) {
    write!(b, "
# Bookies

| Host | Balance | Active |
| ---- | -------:|:------:|
    ");

    for bookie in &state.bookies {
        writeln!(b, "|{host}|{balance}|{active}|",
                 host = bookie.bookie.host,
                 balance = bookie.balance,
                 active = if bookie.active { '✓' } else { ' ' });
    }
}

fn render_events(b: &mut String, state: &State) {
    if state.events.is_empty() {
        return;
    }

    writeln!(b, "# Events");

    let mut groups = HashMap::new();

    for (offer, event) in &state.events {
        let vec = groups.entry(offer.kind.clone()).or_insert_with(Vec::new);
        vec.push(event);
    }

    for (kind, events) in groups {
        writeln!(b, "## {:?}", kind);

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
                    write!(b, "{outcome} `{odds}`|",
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
