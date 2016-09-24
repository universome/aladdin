use std::sync::Mutex;
use rusqlite::Connection;

use base::config::CONFIG;
use base::currency::Currency;

pub struct Combo {
    pub date: u32,
    pub kind: String,
    pub bets: Vec<Bet>
}

pub struct Bet {
    pub host: String,
    pub id: u64,
    pub title: String,
    pub expiry: u32,
    pub coef: f64,
    pub size: Currency,
    pub profit: f64
}

lazy_static! {
    static ref DB: Mutex<Connection> = {
        let path = CONFIG.lookup("arbitrer.database").unwrap().as_str().unwrap();
        let db = Connection::open(path).unwrap();

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
    size    REAL    NOT NULL,
    profit  REAL    NOT NULL,

    PRIMARY KEY(host, id)
)";

const COMBO_SCHEMA: &str = "combo(
    date    INTEGER NOT NULL,
    kind    TEXT    NOT NULL,
    bet_1   INTEGER NOT NULL,
    bet_2   INTEGER NOT NULL,
    bet_3   INTEGER
)";

pub fn contains(host: &str, id: u64) -> bool {
    let db = DB.lock().unwrap();
    let mut stmt = db.prepare_cached("SELECT id FROM bet WHERE host = ? and id = ?").unwrap();

    stmt.exists(&[&host, &(id as i64)]).unwrap()
}

pub fn save(combo: Combo) {
    // TODO(loyd): use cache.
    const INSERT_BET: &str = "INSERT INTO bet(host, id, title, expiry, coef, size, profit)
                              VALUES (:host, :id, :title, :expiry, :coef, :size, :profit)";

    const INSERT_COMBO: &str = "INSERT INTO combo(date, kind, bet_1, outcome_2, outcome_3)
                                VALUES (:date, :kind, :outcome_1, :outcome_2, :outcome_3)";

    let mut db = DB.lock().unwrap();
    let tx = db.transaction().unwrap();

    let row_ids = combo.bets.iter().map(|bet| {
        let size: f64 = bet.size.into();

        tx.execute_named(INSERT_BET, &[
            (":host", &bet.host),
            (":id", &(bet.id as i64)),
            (":title", &bet.title),
            (":expiry", &(bet.expiry as i64)),
            (":coef", &bet.coef),
            (":size", &size),
            (":profit", &bet.profit)
        ]).unwrap();

        tx.last_insert_rowid()
    }).collect::<Vec<_>>();

    tx.execute_named(INSERT_COMBO, &[
        (":date", &(combo.date as i64)),
        (":kind", &combo.kind),
        (":bet_1", &row_ids[0]),
        (":bet_2", &row_ids[1]),
        (":bet_3", &row_ids.get(2).map(|x| *x))
    ]).unwrap();

    tx.commit().unwrap();
}
