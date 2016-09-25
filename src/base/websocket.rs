use std::fmt::{Debug};
use std::str;
use serde_json as json;
use serde::{Serialize, Deserialize};
use websocket::message::{Message, Type};
use websocket::sender::Sender;
use websocket::receiver::Receiver;
use websocket::client::request::Url;
use websocket::client::Client;
use websocket::dataframe::DataFrame;
use websocket::stream::WebSocketStream;

use base::error::{Result, Error};

pub struct Connection(Client<DataFrame, Sender<WebSocketStream>, Receiver<WebSocketStream>>);

impl Connection {
    pub fn new(location: &str) -> Result<Connection> {
        let url = try!(Url::parse(format!("wss://{}", location).as_ref()));
        let request = try!(Client::connect(url));
        let response = try!(request.send());

        try!(response.validate()); // Ensure the response is valid.

        Ok(Connection(response.begin()))
    }

    pub fn send<T: Serialize + Debug>(&mut self, message: T) -> Result<()> {
        debug!("Sending message: {:?}", message);

        let message = Message::text(try!(json::to_string(&message)));

        self.0.send_message(&message).map_err(Error::from)
    }

    pub fn receive<T: Deserialize>(&mut self) -> Result<T> {
        loop {
            // TODO(universome): why the fuck recv_message() not working?
            let message: Message = self.0.incoming_messages().next().unwrap().unwrap();

            match message.opcode {
                Type::Close => {
                    debug!("Received close message. Sending close message.");
                    try!(self.0.send_message(&Message::close()));
                },
                Type::Ping => {
                    debug!("Received ping. Sending pong.");
                    try!(self.0.send_message(&Message::pong(message.payload)));
                },
                Type::Text => {
                    debug!("Received text message: {:?}", str::from_utf8(&*message.payload));

                    match json::from_reader::<&[u8], T>(&*message.payload) {
                        Ok(m) => return Ok(m),
                        Err(err) => {
                            warn!("Error while parsing websocket message: {}", err);
                        }
                    }
                },
                another_type => {
                    debug!("Received not interesting message type: {:?}", another_type);
                }
            }
        }
    }
}
