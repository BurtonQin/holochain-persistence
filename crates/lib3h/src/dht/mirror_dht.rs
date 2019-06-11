use crate::dht::{
    dht_event::*,
    dht_trait::Dht,
};
use lib3h_protocol::{AddressRef, DidWork, Lib3hResult,Address, data_types::EntryData};
use std::collections::{
    VecDeque, HashMap,
};

use serde::{Deserialize, Serialize};
use rmps::{Deserializer, Serializer};

/// Mirror DHT implementation: Holds and reflect everything back to other nodes (fullsync)
///  - On *HoldRequest, store and gossip data back to every known peer.
///  - Gossip can only be a *HoldRequest
///  - Monotonic data
pub struct MirrorDht {
    /// FIFO of DhtEvents send to us
    inbox: VecDeque<DhtEvent>,
    /// Storage of PeerHoldRequestData
    peer_list: HashMap<String, PeerHoldRequestData>,
    /// Storage of EntryData with empty aspect content?
    data_list: HashMap<Address, EntryData>,
    /// PeerHoldRequestData of this peer
    this_peer: PeerHoldRequestData,
}

/// Constructor
impl MirrorDht {
    pub fn new(peer_address: &str, peer_transport: &str) -> Self {
        MirrorDht {
            inbox: VecDeque::new(),
            peer_list: HashMap::new(),
            data_list: HashMap::new(),
            this_peer: PeerHoldRequestData {
                peer_address: peer_address.to_string(),
                transport: peer_transport.to_string(),
                timestamp: 0,
            },
        }
    }
}

/// Impl Dht interface
impl Dht for MirrorDht {
    // -- Getters -- //

    fn this_peer(&self) -> Lib3hResult<&str> {
        Ok(&self.this_peer.peer_address)
    }

    // -- Peer -- //

    fn get_peer(&self, peer_address: &str) -> Option<PeerHoldRequestData> {
        let res = self.peer_list.get(peer_address);
        if let Some(pd) = res {
            return Some(pd.clone());
        }
        None
    }
    fn fetch_peer(&self, peer_address: &str) -> Option<PeerHoldRequestData> {
        // FIXME: wait 200ms
        self.get_peer(peer_address)
    }
    fn drop_peer(&self, peer_address: &str) -> Lib3hResult<()> {
        let res = self.peer_list.remove(peer_address);
        match res {
            None => Err(format_err!("unknown peer: {}", peer_address)),
            Some(_) => Ok(())
        }
    }
    fn get_peer_list(&self) -> Vec<PeerHoldRequestData> {
        let mut res = Vec::new();
        for (_, peer) in self.peer_list.iter() {
            res.push(peer.clone());
        }
        res
    }

    // -- Data -- //

    fn get_entry(&self, data_address: &AddressRef) -> Option<EntryData> {
        let res = self.data_list.get(data_address);
        if let Some(entry) = res {
            return Some(entry.clone());
        }
        None
    }

    /// Same as get_data with artificial wait
    fn fetch_entry(&self, data_address: &AddressRef) -> Option<EntryData> {
        // FIXME: wait 200ms
        self.get_entry(data_address)
    }

    // -- Processing -- //

    fn post(&mut self, evt: DhtEvent) -> Lib3hResult<()> {
        self.inbox.push_back(evt);
        Ok(())
    }

    ///
    fn process(&mut self) -> Lib3hResult<(DidWork, Vec<DhtEvent>)> {
        let outbox = Vec::new();
        let mut did_work = false;
        loop {
            let evt = match self.inbox.pop_front() {
                None => break,
                Some(msg) => msg,
            };
            let res = self.serve_DhtEvent(&evt);
            if let Ok(mut output) = res {
                did_work = true;
                outbox.append(&mut output);
            }
        }
        Ok((did_work, outbox))
    }
}

/// Internals
impl MirrorDht {
    /// Return true if new peer or updated peer
    fn add_peer(&mut self, peer_info: &PeerHoldRequestData) -> bool {
        let mut maybe_peer = self.peer_list.get_mut(&peer_info.peer_address);
        match maybe_peer {
            None => {
                self.peer_list.insert(peer_info.peer_address, peer_info.clone());
                true
            }
            Some(mut peer) => {
                if peer_info.timestamp <= peer.timestamp {
                    return false;
                }
                peer.timestamp = peer_info.timestamp;
                true
            }
        }
    }

    /// Add entry to our local storage
    /// Return true if new data was successfully added
    fn add_data(&mut self, entry: &EntryData) -> bool {
        let maybe_entry = self.data_list.get_mut(&entry.entry_address);
        match maybe_entry {
            None => {
                self.data_list.insert(entry.entry_address, entry.clone());
                true
            }
            Some(mut prev_entry) => {
                return prev_entry.merge(entry);
            }
        }
    }

    /// Process a DhtEvent Command, sent by our owner.
    /// Return a list of DhtEvent to owner.
    #[allow(non_snake_case)]
    fn serve_DhtEvent(
        &mut self,
        evt: &DhtEvent,
    ) -> Lib3hResult<Vec<DhtEvent>> {
        println!("(log.d) >>> '(MirrorDht)' recv evt: {:?}", evt);
        // Note: use same order as the enum
        match evt {
            // Received gossip from remote node. Bundle must be one of the following:
            // - DataHoldRequestData
            // - PeerHoldRequestData
            DhtEvent::RemoteGossipBundle(msg) => {
                let evt;
                let mut de = Deserializer::new(&msg.bundle[..]);
                let maybe_entry: Result<EntryData, Self::Error> = Deserialize::deserialize(&mut de)();
                if let Ok(entry) = maybe_entry {
                    let is_new = self.add_data(&entry);
                    if is_new {
                        return Ok(vec![DhtEvent::EntryHoldRequest(entry)]);
                    }
                    return Ok(vec![]);
                }
                let maybe_peer: Result<PeerHoldRequestData> = Deserialize::deserialize(&mut de)();
                if let Ok(peer) = maybe_peer {
                    let is_new = self.add_peer(&peer);
                    if is_new {
                        return Ok(vec![DhtEvent::PeerHoldRequest(peer)]);
                    }
                    return Ok(vec![]);
                }
                Err(format_err!("RemoteGossipBundle bundle is of unknown type"))
            }
            // N/A
            DhtEvent::GossipTo(_) => {
                panic!("Should not receive this?");
            }
            // N/A
            DhtEvent::UnreliableGossipTo(_) => {
                panic!("Should not receive this?");
            }
            // Owner is asking us to hold peer info
            DhtEvent::PeerHoldRequest(msg) => {
                // Store it
                let received_new_content = self.add_peer(msg);
                // Bail if did not receive new content
                if !received_new_content {
                    return Ok(vec![]);
                }
                // Gossip to everyone to also hold it
                let peer = self.peer_list.get(&msg.peer_address).expect("Should have peer by now");
                let mut buf = Vec::new();
                peer.serialize(&mut Serializer::new(&mut buf)).unwrap();
                let gossip_evt = GossipToData {
                    peer_address_list: self.get_peer_list(), // FIXME serialize PeerHoldData
                    bundle: buf,
                };
                // Done
                Ok(vec![DhtEvent::GossipTo(gossip_evt)])
            }
            // ???
            DhtEvent::PeerTimedOut(_) => {Ok(vec![])}
            // Owner asks us to hold some data. Store it and gossip to every known peer.
            DhtEvent::EntryHoldRequest(entry) => {
                // Local asks us to hold data.
                // Store it
                let received_new_content = self.add_data(entry);
                // Bail if did not receive new content
                if !received_new_content {
                    return Ok(vec![]);
                }
                // Gossip new data to all known peers
                let entry = self.data_list.get(&entry.entry_address).expect("Should have content at this point");
                let mut buf = Vec::new();
                entry.serialize(&mut Serializer::new(&mut buf)).unwrap();
                let gossip_evt = GossipToData {
                    peer_address_list: self.get_peer_list(), // FIXME serialize PeerHoldData
                    bundle: buf,
                };
                // Done
                Ok(vec![DhtEvent::GossipTo(gossip_evt)])
            }
            DhtEvent::EntryFetch(_) => {
                panic!("Should not receive this?");
            }
            DhtEvent::EntryFetchResponse(_) => {
                panic!("Should not receive this?");
            }
            // Monotonic fullsync dht for now
            DhtEvent::EntryPrune(_) => {
                panic!("Should not receive this?");
            }
        }
    }
}
