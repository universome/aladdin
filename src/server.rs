#![allow(unused_must_use)]

use std::fmt::Write;
use std::time::{Duration, Instant};
use hyper::server::{Server, Request, Response};
use time;

use base::error::Result;
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
        let state = arbitrer::acquire_state();
        render_bookies(&mut buffer, &*state);
        render_events(&mut buffer, &*state);
    }

    render_footer(&mut buffer, now.elapsed());

    try!(res.send(buffer.as_bytes()));

    Ok(())
}

fn render_header(b: &mut String) {
    write!(b, r#"
<!DOCTYPE html>
<meta charset="utf-8">
<title>Aladdin</title>
<script src="http://strapdownjs.com/v/0.2/strapdown.js" defer></script>
<xmp style="display:none;">
    "#);
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
    writeln!(b, "# Events");

    for event in state.events.values() {
        writeln!(b, "|`{date}`|{kind:?}|",
                 date = format_date(event[0].1.date, "%d/%m"),
                 kind = event[0].1.kind);

        writeln!(b, "| --- | --- |:---:|");

        for &MarkedOffer(bookie, ref offer) in event {
            write!(b, "|`{date}`|{host}|#{inner_id}|",
                   date = format_date(offer.date, "%R"),
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
