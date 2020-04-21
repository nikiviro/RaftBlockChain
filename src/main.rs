extern crate bincode;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate rand;
extern crate ed25519_dalek;
extern crate base64;


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

use rand::rngs::OsRng;
use ed25519_dalek::{Keypair, PUBLIC_KEY_LENGTH, PublicKey, SecretKey};
use ed25519_dalek::Signature;
use base64::{encode, decode};
use bincode::{serialize};

pub use crate::blockchain::*;
pub use crate::blockchain::block::Block;
pub use crate::raft_node::*;
pub use crate::raft_node::RaftNode;
pub use crate::proposal::Proposal;
use crate::p2p::network_manager::NetworkManager;
use crate::raft_engine::RaftEngine;
use crate::node::Node;
use crate::blockchain::block::ConfiglBlockBody;
use std::str::FromStr;


mod blockchain;
mod raft_node;
mod proposal;
mod p2p;
mod raft_engine;
mod node;



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
    let config_file_name: String =  args[2].parse().unwrap();
    let this_peer_port: u64 = args[3].parse().unwrap();
    let mut peer_list = Vec::new();
    for x in 4..(args.len()){
        println!("Peer: {}", args[x]);
        let peer_port: u64 = args[x].parse().unwrap();
        peer_list.push(peer_port);
    }

    // //public,private key generation
    // let mut csprng = OsRng{};
    // let keypair: Keypair = Keypair::generate(&mut csprng);
    //
    // let public_key_bytes: [u8; PUBLIC_KEY_LENGTH] = keypair.public.to_bytes();
    // let encoded_public_key = base64::encode(&public_key_bytes);
    // println!("public key: {:?}", encoded_public_key);
    // let private_key_bytes: [u8; PUBLIC_KEY_LENGTH] = keypair.secret.to_bytes();
    // let encoded_private_key = base64::encode(&private_key_bytes);
    // println!("private key: {:?}", encoded_private_key);

    let mut is_raft_node = true;
    let config_json = load_config_from_file(config_file_name);

    let genesis_config = load_genessis_config();

    if config_json.nodes_without_raft.contains(&this_peer_port.to_string()){
        is_raft_node = false;
        println!("No-RAFT node")
    }

    let config = NodeConfig::new(config_json);

    let mut node = Node::new(this_peer_port);

    node.start(this_peer_port, is_raft_node, peer_list, genesis_config, config)
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

pub fn load_config_from_file(file_name: String) -> ConfigStructJson {

    let config_dir = "config/".to_string();
    let mut config = ConfigLoader::default();
    config
        .merge(File::with_name(&format!("{}{}",config_dir,file_name))).unwrap();
    // Print out our settings (as a HashMap)
    let configstruct =  config.try_into::<ConfigStructJson>().unwrap();
    println!("\n{:?} \n\n-----------",
             &configstruct);

    configstruct
}

pub fn load_genessis_config() -> ConfiglBlockBody{

    let mut config = ConfigLoader::default();
    config
        .merge(File::with_name("config/genesis.json")).unwrap();
    // Print out our settings (as a HashMap)
    let genesis_config =  config.try_into::<ConfiglBlockBody>().unwrap();
    println!("\n{:?} \n\n-----------",
             &genesis_config);

    genesis_config
}

pub fn now () -> u128 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        ;

    duration.as_secs() as u128 * 1000 + duration.subsec_millis() as u128
}


#[derive(Serialize, Deserialize,Debug)]
pub struct
ConfigStructJson {
    pub nodes_without_raft: Vec<String>,
    pub publick_key: String,
    pub private_key: String,
    pub electors: HashMap<String, String>
}

#[derive(Debug)]
pub struct
NodeConfig {
    pub nodes_without_raft: Vec<String>,
    pub key_pair: Keypair,
    pub electors: HashMap<u64, String>
}

impl NodeConfig{
    pub fn new(config_from_json: ConfigStructJson) -> Self{
        let public_key = PublicKey::from_bytes( &base64::decode(config_from_json.publick_key).unwrap()).unwrap();
        let private_key = SecretKey::from_bytes( &base64::decode(config_from_json.private_key).unwrap()).unwrap();

        let key_pair = Keypair{
            secret: private_key,
            public: public_key
        };
        let mut electors: HashMap<u64, String>  = HashMap::new();
        for (key, value) in config_from_json.electors {
            electors.insert(u64::from_str(key.as_ref()).expect("json config file in bad format - elector_id"), value);
        }
        NodeConfig{
            nodes_without_raft: config_from_json.nodes_without_raft,
            key_pair,
            electors
        }
    }
}