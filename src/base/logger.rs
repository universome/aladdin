use std::env;
use std::sync::{RwLock, RwLockReadGuard};
use std::collections::VecDeque;
use log::{self, Log, LogRecord, LogLevel, LogMetadata, SetLoggerError};
use env_logger::{LogBuilder as EnvLogBuilder, Logger as EnvLogger};
use time;

struct Logger(EnvLogger);

impl Log for Logger {
    fn enabled(&self, metadata: &LogMetadata) -> bool {
        self.0.enabled(metadata)
    }

    fn log(&self, record: &LogRecord) {
        self.0.log(record);

        if self.enabled(record.metadata()) && record.level() <= LogLevel::Warn {
            let mut messages = MESSAGES.write().unwrap();
            let target = record.target();

            messages.push_back(Message {
                level: record.level(),
                module: if target.starts_with("aladdin::") {
                    target.rsplit("::").next().unwrap().to_string()
                } else {
                    target.to_string()
                },
                date: time::get_time().sec as u32,
                data: format!("{}", record.args())
            });
        }
    }
}

pub struct Message {
    pub level: LogLevel,
    pub module: String,
    pub date: u32,
    pub data: String
}

lazy_static! {
    static ref MESSAGES: RwLock<VecDeque<Message>> = RwLock::new(VecDeque::new());
}

pub fn acquire_messages() -> RwLockReadGuard<'static, VecDeque<Message>> {
    MESSAGES.read().unwrap()
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
