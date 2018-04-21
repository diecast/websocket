extern crate diecast;
extern crate ws;

use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, channel};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::thread;

use diecast::{Handle, Item};

pub struct WebsocketPipe {
    ws_tx: Mutex<mpsc::Sender<Update>>,
}

pub fn pipe(ws_tx: mpsc::Sender<Update>) -> WebsocketPipe {
    WebsocketPipe {
        ws_tx: Mutex::new(ws_tx),
    }
}

impl Handle<Item> for WebsocketPipe {
    fn handle(&self, item: &mut Item) -> diecast::Result<()> {
        let ws_tx = {
            let sender = self.ws_tx.lock().unwrap();
            sender.clone()
        };

        // TODO
        // need better api for route access
        let uri = format!("/{}", item.route().reading().unwrap().display());
        let update = Update {
            url: uri,
            body: item.body.clone(),
        };

        try!(ws_tx.send(update));

        Ok(())
    }
}

#[derive(Debug)]
pub struct Update {
    url: String,
    body: String,
}

struct WebSocketHandler {
    ws: ws::Sender,
    path: String,
    registry: Arc<Mutex<SocketRegistry>>,
}

impl ws::Handler for WebSocketHandler {
    fn on_open(&mut self, shake: ws::Handshake) -> ws::Result<()> {
        // Get the request.
        self.path = shake.request.resource().to_string();

        println!("Received connection for {}", self.path);

        self.registry.lock().unwrap().add_client(self);

        Ok(())
    }

    fn on_close(&mut self, _code: ws::CloseCode, _reason: &str) {
        println!("Removed connection for {}", self.path);

        self.registry.lock().unwrap().remove_client(self);
    }
}

impl WebSocketHandler {
    pub fn new(ws: ws::Sender, registry: Arc<Mutex<SocketRegistry>>) -> WebSocketHandler {
        WebSocketHandler {
            ws: ws,
            path: "".to_string(),
            registry: registry,
        }
    }
}

struct SocketRegistry {
    // mapping Path → {Token → Sender}
    clients: HashMap<String, HashMap<ws::util::Token, ws::Sender>>,
}

impl SocketRegistry {
    fn new() -> SocketRegistry {
        SocketRegistry {
            clients: HashMap::new(),
        }
    }

    // Insert the sender into the hashmap for this url
    fn add_client(&mut self, client: &WebSocketHandler) {
        self.clients
            .entry(client.path.clone()).or_insert(HashMap::new())
            .entry(client.ws.token()).or_insert(client.ws.clone());

        println!("added client {:?}", client.ws.token());
    }

    // Remove the sender
    fn remove_client(&mut self, client: &WebSocketHandler) {
        if let Some(clients) = self.clients.get_mut(&client.path) {
            clients.remove(&client.ws.token()).unwrap();
        }

        println!("removed client {:?}", client.ws.token());
    }

    // Go through each client subscribed to this path and send the update
    fn send_update(&self, update: Update) {
        if let Some(clients) = self.clients.get(&update.url) {
            for (_token, sender) in clients.iter() {
                sender.send(update.body.clone()).unwrap();
            }
        }
    }
}

struct WebSocketServer {
    registry: Arc<Mutex<SocketRegistry>>,
}

impl WebSocketServer {
    pub fn new(registry: Arc<Mutex<SocketRegistry>>) -> WebSocketServer {
        WebSocketServer {
            registry: registry,
        }
    }
}

impl ws::Factory for WebSocketServer {
    type Handler = WebSocketHandler;

    fn connection_made(&mut self, sender: ws::Sender) -> Self::Handler {
        // Create handler.
        WebSocketHandler::new(sender, self.registry.clone())
    }
}

// Initialize websocket server and return a channel sender-end used to publish
// Updates to pages.
pub fn init() -> mpsc::Sender<Update> {
    let (tx, rx) = channel::<Update>();

    {
        let registry = Arc::new(Mutex::new(SocketRegistry::new()));
        let server = WebSocketServer::new(registry.clone());
        let websocket = ws::WebSocket::new(server).unwrap();

        thread::spawn(move || {
            for update in rx.iter() {
                registry.lock().unwrap().send_update(update);
            }
        });

        thread::spawn(move || websocket.listen("0.0.0.0:9160").unwrap());
    }

    tx
}
