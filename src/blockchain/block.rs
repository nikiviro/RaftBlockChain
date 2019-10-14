use std::fmt::{ self, Debug, Formatter };

#[derive(Serialize, Deserialize)]
pub struct Block {
    pub index: u32,
    pub timestamp: u128,
    pub hash: Vec<u8>,
    pub prev_block_hash: Vec<u8>,
    pub nonce: u64,
    pub list_of_nodes: Vec<u64>,

}


impl Debug for Block {
    fn fmt (&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "Block[{}]: at: {} nonce: {}",
               &self.index,
               &self.timestamp,
               &self.nonce,
        )
    }
}

impl Block {
    pub fn new (index: u32, timestamp: u128, prev_block_hash: Vec<u8>) -> Self {
        Block {
            index,
            timestamp,
            hash: vec![0; 32],
            prev_block_hash,
            nonce: 0,
            list_of_nodes: vec![],
        }
    }

    pub fn mine (&mut self) {
        self.nonce = 1;
        let hash = vec![0; 32];
    }

}