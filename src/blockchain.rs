use crate::Block;
use std::collections::HashMap;

pub mod block;

pub struct Blockchain {
    pub blocks: Vec<block::Block>,
    pub uncommited_block_queue: HashMap<String, Block>
}

impl Blockchain {
    pub fn new () -> Self {
        Blockchain {
            blocks: vec![],
            uncommited_block_queue: HashMap::new()
        }
    }

    pub fn add_block (&mut self, block: block::Block) {

        self.blocks.push(block);
    }

    pub fn get_last_block(& self) -> Option<Block> {
        self.blocks.last().cloned()
    }

    pub fn find_block(&self, block_id: u64, block_hash: String) -> Option<Block> {
        if let Some(block) = self.blocks.iter().find(|&block| block.header.block_id == block_id) {
            Some(block.clone())
        }
        else{
            if let Some(block) = self.uncommited_block_queue.get(&block_hash) {
                Some(block.clone())
            }
            else{
                None
            }
        }
    }
}