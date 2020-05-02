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
use std::collections::{HashMap, VecDeque, BTreeMap};
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
use std::rc::Rc;


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

    let (config, genesis_config) = load_config(config_file_name, "genesis.json".to_string());

    if !config.is_elector_node{
        println!("No-RAFT node")
    }

    let config = Arc::new(config);

    let mut node = Node::new(this_peer_port, config);

    node.start( genesis_config)
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

pub fn load_config(config_file_name: String, genesis_file_name: String) -> (NodeConfig, ConfiglBlockBody){

    //load config json to Rust struct
    let config_json = load_config_from_file(config_file_name);
    //load genesis json to Rust struct
    let genesis_json = load_genesis_config(genesis_file_name);

    // convert HashMap<String, String> from json to HashMap<u64, PublicKey>
    let mut list_of_elector_nodes: BTreeMap<u64, PublicKey>  = BTreeMap::new();
    for (key, value) in genesis_json.list_of_elector_nodes {
        let node_id = u64::from_str(key.as_ref()).expect("json config file in bad format - elector_id");
        let node_public_key = PublicKey::from_bytes( &base64::decode(value).unwrap()).unwrap();
        list_of_elector_nodes.insert(node_id,node_public_key);
    }

    let genesis = ConfiglBlockBody::new(list_of_elector_nodes, 0, genesis_json.pace_of_block_creation);

    let config = NodeConfig::new_from_config_json(config_json, genesis.list_of_elector_nodes.clone(), genesis.pace_of_block_creation);

    (config, genesis)
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

pub fn load_genesis_config(file_name: String) -> GenesisConfigStructJson{

    let config_dir = "config/".to_string();
    let mut config = ConfigLoader::default();
    config
        .merge(File::with_name(&format!("{}{}",config_dir,file_name))).unwrap();
    // Print out our settings (as a HashMap)
    let genesis_config =  config.try_into::<GenesisConfigStructJson>().unwrap();
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
    pub is_elector_node: bool,
    pub node_id: u64,
    pub public_key: String,
    pub private_key: String,
    pub nodes_to_connect: HashMap<String, String>,
}

#[derive(Debug)]
pub struct
NodeConfig {
    pub is_elector_node: bool,
    pub node_id: u64,
    pub key_pair: Keypair,
    pub nodes_to_connect: HashMap<u64, String>,
    pub electors: BTreeMap<u64, PublicKey>,
    pub pace_of_block_creation: u64,
}

impl NodeConfig{
    pub fn new_from_config_json(config_from_json: ConfigStructJson, electors: BTreeMap<u64, PublicKey>, pace_of_block_creation: u64) -> Self{
        let public_key = PublicKey::from_bytes( &base64::decode(config_from_json.public_key).unwrap()).unwrap();
        let private_key = SecretKey::from_bytes( &base64::decode(config_from_json.private_key).unwrap()).unwrap();

        let key_pair = Keypair{
            secret: private_key,
            public: public_key
        };

        let mut nodes_to_connect: HashMap<u64, String>  = HashMap::new();
        for (key, value) in config_from_json.nodes_to_connect {
            let node_id = u64::from_str(key.as_ref()).expect("json config file in bad format - elector_id");
            nodes_to_connect.insert(node_id,value);
        }


        NodeConfig{
            is_elector_node: config_from_json.is_elector_node,
            node_id: config_from_json.node_id,
            key_pair,
            nodes_to_connect,
            electors,
            pace_of_block_creation
        }
    }
}

#[derive(Serialize, Deserialize,Debug)]
pub struct
GenesisConfigStructJson {
    pub list_of_elector_nodes: HashMap<String, String>,
    pub current_leader_id: u64,
    pub pace_of_block_creation: u64,
}