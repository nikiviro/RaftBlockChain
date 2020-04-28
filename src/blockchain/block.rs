use std::fmt::{self, Debug, Formatter};
use std::time::SystemTime;
use crypto::sha2::Sha256;
use crypto::digest::Digest;
use std::collections::{HashMap, BTreeMap};
use ed25519_dalek::PublicKey;

#[derive(Serialize, Deserialize, Clone)]
pub struct Block {
    pub header: BlockHeader,
    pub block_body: BlockBody
    //pub list_of_nodes: Vec<u64>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BlockHeader{
    pub block_id: u64,
    pub epoch_seq_num: u64, // Every block has its sequence number in epoch
    pub block_type: BlockType, //Type of block (Normal, Configuration ..)
    pub prev_block_size: u64, //Size of previous block in bytes
    //pub color_id: String, // Identification of color and its space this block belongs to
    pub timestamp: u128,
    pub prev_block_hash: String,
    //TODO: Move this to trailer
    //Trailer - This will not be hashed
    //pub block_hash: String,
    //pub block_size: u64, // Size of this block in bytes
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
                prev_block_hash: String) -> Self {
        let block_body = match block_type {
            BlockType::Normal => {
                let block_body_string = format!("{}{}","This is block with id ".to_string(), block_id.to_string());
                BlockBody::Normal(NormalBlockBody::new(block_body_string))
            },
            BlockType::Config => BlockBody::Config(ConfiglBlockBody::default()),
        };
        Block {
            header: BlockHeader::new(block_id,epoch_seq_num,
                                     block_type,prev_block_size,
                                     prev_block_hash),
            block_body: block_body
        }
    }

    pub fn genesis(genesis_block_body: ConfiglBlockBody, leader_id: u64) -> Self{
        Block {
            header: BlockHeader::new(1,1,
                                     BlockType::Config,0,
                                     "1".to_string()),
            block_body: BlockBody::Config(genesis_block_body)
        }
    }

    pub fn hash(&self) -> String{
        //Get block bytes
        let mut data = bincode::serialize(&self).expect("Error while serializing block");
        let mut hasher = Sha256::new();
        // write input message
        hasher.input(&data);
        // read hash digest
        hasher.result_str()
    }
}

impl BlockHeader {

    pub fn new (block_id: u64, epoch_seq_num: u64, block_type: BlockType, prev_block_size: u64,
                prev_block_hash: String) -> Self
    {
        BlockHeader {
            block_id: block_id,
            epoch_seq_num: epoch_seq_num,
            block_type: block_type,
            prev_block_size: prev_block_size,
            timestamp: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)
                .expect("Time went backwards").as_secs() as u128,
            prev_block_hash: prev_block_hash
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
    pub current_leader_id: u64
}

impl ConfiglBlockBody{
    pub fn new(list_of_elector_nodes: BTreeMap<u64, PublicKey>, current_leader_id: u64) -> Self {
        ConfiglBlockBody{
            list_of_elector_nodes,
            current_leader_id
        }
    }
}

impl Default for ConfiglBlockBody {
    fn default() -> ConfiglBlockBody {
        let electors: BTreeMap<u64, PublicKey>  = BTreeMap::new();
        ConfiglBlockBody{
            list_of_elector_nodes:  electors,
            current_leader_id: 0
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
