use std::fmt::{self, Debug, Formatter};
use std::time::SystemTime;
use crypto::sha2::Sha256;
use crypto::digest::Digest;
use std::collections::{BTreeMap};
use ed25519_dalek::{PublicKey, Signature, Keypair};

#[derive(Serialize, Deserialize, Clone)]
pub struct Block {
    pub header: BlockHeader,
    pub block_body: BlockBody,
    pub block_trailer: BlockTrailer
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BlockHeader{
    pub block_id: u64,
    pub epoch_seq_num: u64, //Unimplemented
    pub block_type: BlockType, //Type of block (Normal, Configuration ..)
    pub prev_block_size: u64, //Size of previous block in bytes
    pub timestamp: u128,
    pub prev_block_hash: String,
    pub proposer: u64, //id of the leader who created this block
}

impl Debug for Block {
    fn fmt (&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "Block-id[{}], at: {}, type: {}, hash: {} ...",
               &self.header.block_id,
               &self.header.timestamp,
               &self.header.block_type,
               &self.hash()[0..5]
        )
    }
}

impl Block {

    pub fn new (block_id: u64, epoch_seq_num: u64, block_type: BlockType, prev_block_size: u64,
                prev_block_hash: String, proposer_id: u64, key_pair: &Keypair) -> Self {
        let block_body = match block_type {
            BlockType::Normal => {
                let block_body_string = format!("{}{}","This is block with id ".to_string(), block_id.to_string());
                BlockBody::Normal(NormalBlockBody::new(block_body_string))
            },
            BlockType::Config => BlockBody::Config(ConfiglBlockBody::default()),
        };

        let block_header = BlockHeader::new(block_id,epoch_seq_num,
                                            block_type,prev_block_size,
                                            prev_block_hash, proposer_id);

        let block_trailer = BlockTrailer::new(&block_header, &block_body, key_pair);

        Block {
            header: block_header,
            block_body: block_body,
            block_trailer: block_trailer,
        }
    }

    pub fn genesis(genesis_block_body: ConfiglBlockBody, leader_id: u64, key_pair: &Keypair) -> Self{

        let block_header = BlockHeader::new(1,1,
                                            BlockType::Config,0,
                                            "1".to_string(), leader_id);
        let block_body = BlockBody::Config(genesis_block_body);
        let block_trailer = BlockTrailer::new(&block_header, &block_body, key_pair);
        Block {
            header: block_header,
            block_body: block_body,
            block_trailer: block_trailer
        }
    }

    pub fn hash(&self) -> String{
        //Get block bytes
        let mut data = bincode::serialize(&self.header).expect("Error while serializing block header");
        data.extend( bincode::serialize(&self.block_body).expect("Error while serializing block body"));
        let mut hasher = Sha256::new();
        // write input message
        hasher.input(&data);
        // read hash digest
        hasher.result_str()
    }

    pub fn is_valid(&self, elector_list: &BTreeMap<u64, PublicKey>) -> bool{

        match elector_list.get(&self.header.proposer){
            Some(public_key) => {
                match public_key.verify(&self.hash().as_bytes(), &self.block_trailer.proposer_signature).is_ok(){
                    true => {
                        debug!("[BLOCK SIGNATURE CORRECT]");
                        return true
                    },
                    false =>  {
                        debug!("[BLOCK SIGNATURE INCORRECT]");
                        return false
                    }
                }
            }
            None => {
                debug!("[BLOCK CREATED BY UNKNOWN NODE] - Received block from node with unknown public key");
                return false
            }
        }
    }
}

impl BlockHeader {

    pub fn new (block_id: u64, epoch_seq_num: u64, block_type: BlockType, prev_block_size: u64,
                prev_block_hash: String, proposer_id: u64) -> Self
    {
        BlockHeader {
            block_id: block_id,
            epoch_seq_num: epoch_seq_num,
            block_type: block_type,
            prev_block_size: prev_block_size,
            timestamp: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)
                .expect("Time went backwards").as_secs() as u128,
            prev_block_hash: prev_block_hash,
            proposer: proposer_id
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NormalBlockBody{
    pub text: String
}

impl NormalBlockBody{
    pub fn new(text: String) -> Self {
        NormalBlockBody{
            text
        }
    }
}

impl Default for NormalBlockBody {
    fn default() -> NormalBlockBody {
        NormalBlockBody {
            text: "".to_string()
        }
    }
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConfiglBlockBody{
    pub list_of_elector_nodes: BTreeMap<u64, PublicKey>,
    pub pace_of_block_creation: u64,
    pub heartbeat_frequency: u64,
    pub min_election_timeout: u64,
    pub max_election_timeout: u64
}

impl ConfiglBlockBody{
    pub fn new(list_of_elector_nodes: BTreeMap<u64, PublicKey>, pace_of_block_creation: u64,
               heartbeat_frequency: u64, min_election_timeout: u64, max_election_timeout: u64) -> Self {
        ConfiglBlockBody{
            list_of_elector_nodes,
            pace_of_block_creation,
            heartbeat_frequency,
            min_election_timeout,
            max_election_timeout
        }
    }
}

impl Default for ConfiglBlockBody {
    fn default() -> ConfiglBlockBody {
        let electors: BTreeMap<u64, PublicKey>  = BTreeMap::new();
        ConfiglBlockBody{
            list_of_elector_nodes:  electors,
            pace_of_block_creation: 10000,
            heartbeat_frequency: 1500,
            min_election_timeout: 15000,
            max_election_timeout: 50000
        }
    }
}

#[derive(Debug,Serialize, Deserialize, Clone)]
pub enum BlockType {
    Normal,
    Config
}

impl fmt::Display for BlockType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockType::Normal => write!(f, "Normal"),
            BlockType::Config => write!(f, "Config"),
        }

    }
}

#[derive(Debug,Serialize, Deserialize, Clone)]
pub enum BlockBody {
    Normal(NormalBlockBody),
    Config(ConfiglBlockBody)
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BlockTrailer{
    pub proposer_signature: Signature, //leader signature of this block hash
}

impl BlockTrailer{
    pub fn new(block_header: &BlockHeader, block_body: &BlockBody, key_pair: &Keypair) -> Self {

        let mut data = bincode::serialize(block_header).expect("Error while serializing block header");
        data.extend( bincode::serialize(block_body).expect("Error while serializing block body"));
        let mut hasher = Sha256::new();
        // write input message
        hasher.input(&data);
        // read hash digest
        let hash = hasher.result_str();

        let signature = key_pair.sign(&hash.as_bytes());
        BlockTrailer{
            proposer_signature: signature
        }
    }
}