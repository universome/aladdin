use std::env;
use std::sync::{RwLock, RwLockReadGuard};
use std::collections::VecDeque;
use log::{self, Log, LogRecord, LogLevel, LogMetadata, SetLoggerError};
use env_logger::{LogBuilder as EnvLogBuilder, Logger as EnvLogger};
use time;

use base::config::CONFIG;

struct Logger(EnvLogger);

impl Log for Logger {
    fn enabled(&self, metadata: &LogMetadata) -> bool {
        self.0.enabled(metadata)
    }

    fn log(&self, record: &LogRecord) {
        self.0.log(record);

        if self.enabled(record.metadata()) && record.level() <= LogLevel::Warn {
            let target = record.target();

            save_to_history(Message {
                level: record.level(),
                module: if target.starts_with("aladdin::") {
                    target.rsplit("::").next().unwrap().to_string()
                } else {
                    target.to_string()
                },
                date: time::get_time().sec as u32,
                data: format!("{}", record.args()),
                count: 1
            });
        }
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
    static ref HISTORY_SIZE: usize = CONFIG.lookup("logging.history-size")
        .unwrap().as_integer().unwrap() as usize;
}

fn save_to_history(message: Message) {
    let mut history = HISTORY.write().unwrap();

    if let Some(last) = history.back_mut() {
        if (&last.module, &last.data) == (&message.module, &message.data) {
            last.count += 1;
            return;
        }
    }

    if history.len() >= *HISTORY_SIZE {
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

    let env_logger = env_log_builder.build();

    log::set_logger(|max_log_level| {
        max_log_level.set(env_logger.filter());
        Box::new(Logger(env_logger))
    })
}
