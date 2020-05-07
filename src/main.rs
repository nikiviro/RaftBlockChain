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
extern crate clap;


use std::collections::{HashMap, BTreeMap};
use std::sync::{Arc};
use config::{Config as ConfigLoader, File};
use rand::rngs::OsRng;
use ed25519_dalek::{Keypair, PUBLIC_KEY_LENGTH, PublicKey, SecretKey};
use clap::{Arg, App};

pub use crate::blockchain::*;
pub use crate::blockchain::block::Block;
pub use crate::raft_node::*;
pub use crate::raft_node::RaftNode;
pub use crate::proposal::Proposal;
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

    let matches = App::new("Raft Blockchain prototype")
        .version("1.0")
        .author("Nikolas Virostek")
        .about("Prototype of blockchain network with modified Raft consensus protocol")
        .arg(Arg::with_name("config")
            .short("c")
            .long("config")
            .value_name("FILE")
            .help("Sets a config file name")
            .takes_value(true)
            .required_unless("generate_keypair")
            .default_value("config/config.json")
            .required(true))
        .arg(Arg::with_name("genesis")
            .short("g")
            .long("genesis")
            .value_name("FILE")
            .help("Sets a genesis file name")
            .takes_value(true)
            .default_value("config/genesis.json")
            .required_unless("generate_keypair")
            .required(true))
        .arg(Arg::with_name("generate_keypair")
            .long("generate_keypair")
            .help("Generate public and private key for a new node")
            .takes_value(false))
        .get_matches();

    //public, private key generation
    if matches.is_present("generate_keypair"){
        println!("Generating new key pair...");
        let mut csprng = OsRng{};
        let keypair: Keypair = Keypair::generate(&mut csprng);

        let public_key_bytes: [u8; PUBLIC_KEY_LENGTH] = keypair.public.to_bytes();
        let encoded_public_key = base64::encode(&public_key_bytes);
        println!("public key: {:?}", encoded_public_key);
        let private_key_bytes: [u8; PUBLIC_KEY_LENGTH] = keypair.secret.to_bytes();
        let encoded_private_key = base64::encode(&private_key_bytes);
        println!("private key: {:?}", encoded_private_key);
        return;
    }

    // Gets a config file name
    let config_file_name = matches.value_of("config").unwrap().to_string();
    println!("Config file: {}", config_file_name);

    // Gets a genesis file name
    let genesis_file_name = matches.value_of("genesis").unwrap().to_string();
    println!("Genesis file: {}", genesis_file_name);

    let (config, genesis_config) = load_config(config_file_name,
                                               genesis_file_name);

    let config = Arc::new(config);
    let mut node = Node::new( config);
    //Start blockchain node
    node.start( genesis_config)
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

    let genesis = ConfiglBlockBody::new(list_of_elector_nodes,
                                        genesis_json.pace_of_block_creation,
                                        genesis_json.heartbeat_frequency,
                                        genesis_json.min_election_timeout,
                                        genesis_json.max_election_timeout);

    let config = NodeConfig::new_from_config_json(config_json,
                                                  genesis.list_of_elector_nodes.clone(),
                                                  genesis.pace_of_block_creation);

    (config, genesis)
}

pub fn load_config_from_file(file_name: String) -> ConfigStructJson {

    let mut config = ConfigLoader::default();
    config
        .merge(File::with_name(&format!("{}",file_name))).unwrap();
    // Print out our settings (as a HashMap)
    let configstruct =  config.try_into::<ConfigStructJson>().unwrap();
    println!("\n{:?} \n\n-----------",
             &configstruct);

    configstruct
}

pub fn load_genesis_config(file_name: String) -> GenesisConfigStructJson{

    let mut config = ConfigLoader::default();
    config
        .merge(File::with_name(&format!("{}",file_name))).unwrap();
    // Print out our settings (as a HashMap)
    let genesis_config =  config.try_into::<GenesisConfigStructJson>().unwrap();
    println!("\n{:?} \n\n-----------",
             &genesis_config);

    genesis_config
}

#[derive(Serialize, Deserialize,Debug)]
pub struct
ConfigStructJson {
    pub node_id: u64,
    pub public_key: String,
    pub private_key: String,
    pub nodes_to_connect: HashMap<String, String>,
}

#[derive(Debug)]
pub struct
NodeConfig {
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
    pub pace_of_block_creation: u64,
    pub heartbeat_frequency: u64,
    pub min_election_timeout: u64,
    pub max_election_timeout: u64
}