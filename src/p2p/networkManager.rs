use std::collections::HashMap;

use crate::p2p::peer::Peer;
use zmq::Context;

pub struct NetworkManager {
    pub peers: HashMap<u64, Peer>,
    pub zero_mq_context: Context
}

impl NetworkManager {
    pub fn new() -> Self{
        NetworkManager {
            peers: HashMap::new(),
            zero_mq_context: Default::default()
        }
    }

    pub fn add_new_peer(
        &mut self,
        port: u64
    ){
        self.peers.insert(port, Peer::new(port, self.zero_mq_context.clone()));
    }
}