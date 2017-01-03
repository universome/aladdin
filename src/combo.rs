use parking_lot::Mutex;
use rusqlite::{Connection, Row};

use constants::DATABASE;
use base::currency::Currency;

#[derive(Debug)]
pub struct Combo {
    pub date: u32,
    pub game: String,
    pub kind: String,
    pub bets: Vec<Bet>
}

#[derive(Debug)]
pub struct Bet {
    pub host: String,
    pub id: u64,
    pub title: Option<String>,
    pub expiry: u32,
    pub coef: f64,
    pub stake: Currency,
    pub profit: f64,
    pub placed: bool
}

lazy_static! {
    static ref DB: Mutex<Connection> = {
        let db = Connection::open(DATABASE).unwrap();

        db.execute(&format!("CREATE TABLE IF NOT EXISTS {}", BET_SCHEMA), &[]).unwrap();
        db.execute(&format!("CREATE TABLE IF NOT EXISTS {}", COMBO_SCHEMA), &[]).unwrap();

        Mutex::new(db)
    };
}

const BET_SCHEMA: &str = "bet(
    host    TEXT    NOT NULL,
    id      INTEGER NOT NULL,
    title   TEXT,
    expiry  INTEGER NOT NULL,
    coef    REAL    NOT NULL,
    stake   REAL    NOT NULL,
    profit  REAL    NOT NULL,
    placed  BOOLEAN NOT NULL
)";

const COMBO_SCHEMA: &str = "combo(
    date    INTEGER NOT NULL,
    game    TEXT    NOT NULL,
    kind    TEXT    NOT NULL,
    bet_1   INTEGER NOT NULL,
    bet_2   INTEGER NOT NULL,
    bet_3   INTEGER
)";

pub fn contains(host: &str, id: u64) -> bool {
    let db = DB.lock();
    let mut stmt = db.prepare_cached("SELECT id FROM bet WHERE host = ? AND id = ?").unwrap();

    stmt.exists(&[&host, &(id as i64)]).unwrap()
}

pub fn save(combo: Combo) {
    // TODO(loyd): use cache.
    const INSERT_BET: &str = "INSERT INTO bet(host, id, title, expiry, coef, stake, profit, placed)
                              VALUES (:host, :id, :title, :expiry, :coef, :stake, :profit, :placed)";

    const INSERT_COMBO: &str = "INSERT INTO combo(date, game, kind, bet_1, bet_2, bet_3)
                                VALUES (:date, :game, :kind, :bet_1, :bet_2, :bet_3)";

    let mut db = DB.lock();
    let tx = db.transaction().unwrap();

    let row_ids = combo.bets.iter().map(|bet| {
        let stake: f64 = bet.stake.into();

        tx.execute_named(INSERT_BET, &[
            (":host", &bet.host),
            (":id", &(bet.id as i64)),
            (":title", &bet.title),
            (":expiry", &(bet.expiry as i64)),
            (":coef", &bet.coef),
            (":stake", &stake),
            (":profit", &bet.profit),
            (":placed", &bet.placed)
        ]).unwrap();

        tx.last_insert_rowid()
    }).collect::<Vec<_>>();

    tx.execute_named(INSERT_COMBO, &[
        (":date", &(combo.date as i64)),
        (":game", &combo.game),
        (":kind", &combo.kind),
        (":bet_1", &row_ids[0]),
        (":bet_2", &row_ids[1]),
        (":bet_3", &row_ids.get(2).map(|x| *x))
    ]).unwrap();

    tx.commit().unwrap();
}

pub fn mark_as_placed(host: &str, id: u64, title: Option<&str>) {
    let db = DB.lock();

    let mut stmt = db.prepare_cached("UPDATE bet SET placed = 1
                                      WHERE host = ? AND id = ? AND ifnull(title, '') = ?").unwrap();

    let updated = stmt.execute(&[&host, &(id as i64), &title.unwrap_or("")]).unwrap();

    debug_assert_eq!(updated, 1);
}

impl<'a, 'b> From<Row<'a, 'b>> for Combo {
    fn from(row: Row) -> Combo {
        // XXX(loyd): this code relies on column ordering.
        let bets = (0..3)
            .take_while(|i| *i < 2 || row.get::<_, Option<i64>>(3 + i).is_some())
            .map(|i| {
                let o = 6 + i * 8;

                Bet {
                    host:   row.get(o),
                    id:     row.get::<_, i64>(o + 1) as u64,
                    title:  row.get(o + 2),
                    expiry: row.get::<_, i64>(o + 3) as u32,
                    coef:   row.get(o + 4),
                    stake:  Currency::from(row.get::<_, f64>(o + 5)),
                    profit: row.get(o + 6),
                    placed: row.get(o + 7)
                }
            })
            .collect();

        Combo {
            date: row.get::<_, i64>("date") as u32,
            game: row.get("game"),
            kind: row.get("kind"),
            bets: bets
        }
    }
}

pub fn load_recent(count: u32) -> Vec<Combo> {
    let db = DB.lock();

    let mut stmt = db.prepare_cached("
        SELECT * FROM combo
            INNER JOIN bet b1 ON bet_1 = b1.rowid
            INNER JOIN bet b2 ON bet_2 = b2.rowid
            LEFT  JOIN bet b3 ON bet_3 = b3.rowid
        ORDER BY combo.rowid DESC
        LIMIT ?
    ").unwrap();

    let mut rows = stmt.query(&[&(count as i64)]).unwrap();
    let mut combos = Vec::new();

    while let Some(row) = rows.next() {
        combos.push(Combo::from(row.unwrap()))
    }

    combos
}
