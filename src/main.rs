extern crate bincode;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate serde;
#[macro_use]
extern crate serde_derive;

use std::thread;
use std::collections::{HashMap, VecDeque};
use std::env;
use std::process::exit;
use std::sync::{Arc, Mutex, RwLock};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use config::{Config as ConfigLoader, File};
use protobuf::{self, Message as ProtobufMessage};
use raft::{prelude::*, StateRole};
use zmq::Socket;

pub use crate::blockchain::*;
pub use crate::blockchain::block::Block;
pub use crate::raft_node::*;
pub use crate::raft_node::RaftNode;
pub use crate::proposal::Proposal;
use crate::p2p::network_manager::NetworkManager;
use crate::raft_engine::RaftEngine;
use crate::node::Node;

mod blockchain;
mod raft_node;
mod proposal;
mod p2p;
mod raft_engine;
mod node;

pub const RAFT_TICK_TIMEOUT: Duration = Duration::from_millis(100);


#[derive(Serialize, Deserialize,Debug)]
pub struct ConfigStruct {
    pub nodes_without_raft: Vec<String>,
}

fn main() {
    env_logger::init();
    let args: Vec<String> = env::args().collect();
    if args.len()<2 {
        eprintln!("Problem parsing arguments: You need to specify how many nodes should be in cluster.  ",);
        eprintln!("Example : Cargo run 'num_of_nodes'");

        exit(1);
    }
    let is_leader_arg: u8= args[1].parse().unwrap();
    let is_leader = is_leader_arg != 0;
    let this_peer_port: u64 = args[2].parse().unwrap();
    let mut peer_list = Vec::new();
    for x in 3..(args.len()){
        println!("Peer: {}", args[x]);
        let peer_port: u64 = args[x].parse().unwrap();
        peer_list.push(peer_port);
    }


    let mut is_raft_node = true;
    let config = load_config_from_file();

    if config.nodes_without_raft.contains(&this_peer_port.to_string()){
        is_raft_node = false;
        println!("No-RAFT node")
    }

    let mut node = Node::new(this_peer_port);

    node.start(this_peer_port, is_raft_node, is_leader, peer_list)
}

fn add_new_node(proposals: &Mutex<VecDeque<Proposal>>, node_id: u64) {
    let mut conf_change = ConfChange::default();
    conf_change.set_node_id(node_id);
    conf_change.set_change_type(ConfChangeType::AddNode);
    let (proposal, rx) = Proposal::conf_change(&conf_change);
    proposals.lock().unwrap().push_back(proposal);
    if rx.recv().unwrap() {
        println!("Node {:?} succesfully added to cluster", node_id);
    }
}

pub fn load_config_from_file() -> ConfigStruct {

    let mut config = ConfigLoader::default();
    config
        .merge(File::with_name("config/config.json")).unwrap();
    // Print out our settings (as a HashMap)
    let configstruct =  config.try_into::<ConfigStruct>().unwrap();
    println!("\n{:?} \n\n-----------",
             &configstruct);

    configstruct
}

pub fn now () -> u128 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        ;

    duration.as_secs() as u128 * 1000 + duration.subsec_millis() as u128
}