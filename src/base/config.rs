use toml::{Parser, Value};

lazy_static! {
    pub static ref CONFIG: Value = {
        let config = include_str!("../../config.toml");
        let mut parser = Parser::new(config);

        if let Some(table) = parser.parse() {
            return Value::Table(table);
        }

        for error in parser.errors {
            println!("{}", error);
        }

        panic!("The config is invalid.");
    };
}
