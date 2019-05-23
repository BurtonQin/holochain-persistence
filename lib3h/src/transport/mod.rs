//! common types and traits for working with Transport instances

mod error;

use holochain_lib3h_protocol::DidWork;

/// a connection identifier
pub type TransportId = String;
pub type TransportIdRef = str;

pub use self::error::{TransportError, TransportResult};

/// events that can be generated during a connection `process()`
#[derive(Debug, PartialEq, Clone)]
pub enum TransportEvent {
    TransportError(TransportId, TransportError),
    Connect(TransportId),
    Message(TransportId, Vec<u8>),
    Close(TransportId),
}

/// represents a pool of connections to remote nodes
pub trait Transport {
    /// establish a connection to a remote node
    fn connect(&mut self, uri: &str) -> TransportResult<TransportId>;

    /// close an existing open connection
    fn close(&mut self, id: TransportId) -> TransportResult<()>;

    /// close all existing open connections
    fn close_all(&mut self) -> TransportResult<()>;

    /// get a list of all open transport ids
    fn transport_id_list(&self) -> TransportResult<Vec<TransportId>>;

    /// send a payload to remote nodes
    fn send(&mut self, id_list: &[&TransportIdRef], payload: &[u8]) -> TransportResult<()>;

    /// send a payload to all remote nodes
    fn send_all(&mut self, payload: &[u8]) -> TransportResult<()>;

    /// Poll TransportEvents received from the transport / network
    fn poll(&mut self) -> TransportResult<(DidWork, Vec<TransportEvent>)>;
}
