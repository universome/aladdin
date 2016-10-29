use std::env;
use std::sync::{RwLock, RwLockReadGuard};
use std::collections::VecDeque;
use log::{self, Log, LogRecord, LogLevel, LogMetadata, SetLoggerError};
use env_logger::{LogBuilder as EnvLogBuilder, Logger as EnvLogger};
use time;

use constants::HISTORY_SIZE;

struct Logger(EnvLogger);

impl Log for Logger {
    fn enabled(&self, metadata: &LogMetadata) -> bool {
        self.0.enabled(metadata)
    }

    fn log(&self, record: &LogRecord) {
        self.0.log(record);

        if self.enabled(record.metadata()) && record.level() <= LogLevel::Warn {
            save_to_history(Message {
                level: record.level(),
                module: trim_target(record.target()).to_string(),
                date: time::get_time().sec as u32,
                data: format!("{}", record.args()),
                count: 1
            });
        }
    }
}

macro_rules! stylish {
    ($style:expr) => (concat!("\x1b[", $style, "m"))
}

fn format(record: &LogRecord) -> String {
    let (shortcut, style) = match record.level() {
        LogLevel::Error => ("E", stylish!("31")),
        LogLevel::Warn  => ("W", stylish!("33")),
        LogLevel::Info  => ("I", stylish!("37")),
        LogLevel::Debug => ("D", stylish!("35")),
        LogLevel::Trace => ("T", stylish!("34"))
    };

    let timestamp = time::now();

    format!("{st_t}{timestamp}{st_r} {target:12} [{st_t}{shortcut}{st_r}] {st_m}{message}{st_r}",
            st_t = style,
            st_m = style,
            st_r = stylish!("0"),
            timestamp = timestamp.strftime("%R").unwrap(),
            target = trim_target(record.target()),
            shortcut = shortcut,
            message = record.args())
}

fn trim_target(target: &str) -> &str {
    if target.starts_with("aladdin::") {
        target.rsplit("::").next().unwrap()
    } else {
        target
    }
}

pub struct Message {
    pub level: LogLevel,
    pub module: String,
    pub date: u32,
    pub data: String,
    pub count: u32
}

lazy_static! {
    static ref HISTORY: RwLock<VecDeque<Message>> = RwLock::new(VecDeque::new());
}

fn save_to_history(message: Message) {
    let mut history = HISTORY.write().unwrap();

    if let Some(last) = history.back_mut() {
        if (&last.module, &last.data) == (&message.module, &message.data) {
            last.count += 1;
            return;
        }
    }

    if history.len() as u32 >= HISTORY_SIZE {
        history.pop_front();
    }

    history.push_back(message);
}

pub fn acquire_history() -> RwLockReadGuard<'static, VecDeque<Message>> {
    HISTORY.read().unwrap()
}

pub fn init() -> Result<(), SetLoggerError> {
    let mut env_log_builder = EnvLogBuilder::new();

    if let Ok(s) = env::var("RUST_LOG") {
        env_log_builder.parse(&s);
    }

    let env_logger = env_log_builder.format(format).build();

    log::set_logger(|max_log_level| {
        max_log_level.set(env_logger.filter());
        Box::new(Logger(env_logger))
    })
}
