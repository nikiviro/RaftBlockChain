use std::fmt::{ self, Debug, Formatter };

#[derive(Serialize, Deserialize, Clone)]
pub struct Block {
    pub block_id: u64,
    pub epoch_seq_num: u64, // Every block has its sequence number in epoch
    pub preamble_version: u32, //Version of preamble (blockchain header) to know hhow to threat the block
    pub block_size: u64, // Size of this block in bytes
    pub prev_block_size: u64, //Size of previous block in bytes
    pub color_id: String, // Identification of color and its space this block belongs to
    pub timestamp: u128,
    pub hash: Vec<u8>,
    pub prev_block_hash: Vec<u8>,
    pub list_of_nodes: Vec<u64>,

    //Trailer - This will not be hashed
    pub block_hash1: String,
    pub block_hash2: String,
}

impl Debug for Block {
    fn fmt (&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "Block[{}]: at: {}",
               &self.block_id,
               &self.timestamp,
        )
    }
}

impl Block {
    pub fn new (block_id: u64, epoch_seq_num: u64, preamble_version: u32, prev_block_size: u64, color_id: String, timestamp: u128, hash: Vec<u8>, prev_block_hash: Vec<u8>, list_of_nodes: Vec<u64>) -> Self {
        Block {
            block_id: block_id,
            epoch_seq_num: epoch_seq_num,
            preamble_version: preamble_version,
            prev_block_size: prev_block_size,
            block_size: 0,
            color_id: "".to_string(),
            timestamp: timestamp,
            hash: vec![0; 32],
            prev_block_hash: prev_block_hash,
            list_of_nodes: vec![],
            block_hash1: "".to_string(),
            block_hash2: "".to_string()
        }
    }

    pub fn mine (&mut self) {
        let hash = vec![0; 32];
    }

}