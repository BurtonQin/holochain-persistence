use error;
use net::endpoint::Endpoint;
use net::event::{Event, ClientEvent, ServerEvent};
use net::session_client::SessionClient;
use net::session_server::SessionServer;
use std;
use std::collections::{hash_map, HashMap};

struct StdNetListenCon {
    socket: std::net::TcpListener,
}

impl StdNetListenCon {
    fn new (socket: std::net::TcpListener) -> Self {
        StdNetListenCon {
            socket: socket,
        }
    }
}

fn wrap_listen (endpoint: &Endpoint) -> error::Result<std::net::TcpListener> {
    let addr = endpoint.to_socket_addr()?; 
    let socket = std::net::TcpListener::bind(addr)?;
    socket.set_nonblocking(true)?;
    Ok(socket)
}

pub struct StdNetNode {
    node_id: Vec<u8>,
    listen_cons: Vec<StdNetListenCon>,
    server_new_cons: Vec<SessionServer>,
    server_cons: HashMap<String, SessionServer>,
    client_cons: Vec<SessionClient>,
    events: Vec<Event>,
}

impl StdNetNode {
    pub fn new (node_id: &[u8]) -> Self {
        StdNetNode {
            node_id: node_id.to_vec(),
            listen_cons: Vec::new(),
            server_new_cons: Vec::new(),
            server_cons: HashMap::new(),
            client_cons: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn process_once (&mut self) -> Vec<Event> {
        self.process_listen_cons();
        self.process_server_cons();
        self.process_client_cons();

        self.events.drain(..).collect()
    }

    pub fn listen (&mut self, endpoint: &Endpoint) {
        let socket = match wrap_listen(endpoint) {
            Err(e) => {
                self.events.push(Event::OnServerEvent(ServerEvent::OnError(error::Error::from(e))));
                return;
            }
            Ok(s) => s,
        };
        self.listen_cons.push(StdNetListenCon::new(socket));
        self.events.push(Event::OnServerEvent(ServerEvent::OnListening(endpoint.clone())));
    }

    pub fn connect (&mut self, endpoint: &Endpoint) {
        let session = match SessionClient::new_initial_connect(endpoint, &self.node_id) {
            Err(e) => {
                self.events.push(Event::OnClientEvent(ClientEvent::OnError(error::Error::from(e))));
                return;
            }
            Ok(s) => s,
        };
        self.client_cons.push(session);
    }

    // -- private -- //

    fn process_listen_cons (&mut self) {
        let mut new_listen_cons: Vec<StdNetListenCon> = Vec::new();
        'top: for con in self.listen_cons.drain(..) {
            loop {
                match con.socket.accept() {
                    Ok((s, addr)) => {
                        let addr = Endpoint::from(addr);
                        println!("con addr: {:?}", addr);
                        if let Err(e) = s.set_nonblocking(true) {
                            self.events.push(Event::OnServerEvent(ServerEvent::OnError(error::Error::from(e))));
                            continue;
                        }
                        let mut session = match SessionServer::new(&self.node_id, &addr) {
                            Err(e) => {
                                self.events.push(Event::OnServerEvent(ServerEvent::OnError(error::Error::from(e))));
                                continue;
                            }
                            Ok(s) => s,
                        };
                        session.cur_socket = Some(s);
                        self.server_new_cons.push(session);
                        continue;
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        break;
                    }
                    Err(e) => {
                        self.events.push(Event::OnServerEvent(ServerEvent::OnError(error::Error::from(e))));
                        break 'top;
                    }
                }
            }

            new_listen_cons.push(con);
        }
        self.listen_cons = new_listen_cons;
    }

    fn process_server_cons (&mut self) {
        let mut new_cons_list: Vec<SessionServer> = Vec::new();
        let mut new_cons_hash: HashMap<String, SessionServer> = HashMap::new();

        for (mut _k, mut con) in self.server_cons.drain() {
            let (con, mut events) = con.process_once();
            if let Some(con) = con {
                new_cons_hash.insert(con.session_id.clone(), con);
            }
            self.events.append(&mut events);
        }

        for mut con in self.server_new_cons.drain(..) {
            let (con, mut events) = con.process_once();
            if let Some(con) = con {
                if con.session_id.len() > 0 {
                    let key = con.session_id.clone();
                    match new_cons_hash.entry(key) {
                        hash_map::Entry::Occupied(mut e) => {
                            let session = e.get_mut();
                            session.cur_socket = con.cur_socket;
                            session.cur_request = con.cur_request;
                        }
                        hash_map::Entry::Vacant(e) => {
                            e.insert(con);
                        }
                    }
                } else {
                    new_cons_list.push(con);
                }
            }
            self.events.append(&mut events);
        }

        self.server_new_cons = new_cons_list;
        self.server_cons = new_cons_hash;
    }

    fn process_client_cons (&mut self) {
        let mut new_client_cons: Vec<SessionClient> = Vec::new();
        for mut con in self.client_cons.drain(..) {
            let (con, mut events) = con.process_once();
            if let Some(con) = con {
                new_client_cons.push(con);
            }
            self.events.append(&mut events);
        }
        self.client_cons = new_client_cons;
    }
}
