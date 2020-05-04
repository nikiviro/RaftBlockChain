use crate::Block;
use std::collections::HashMap;

pub mod block;

pub struct Blockchain {
    pub blocks: Vec<block::Block>,
    block_hash_map: HashMap<String, Block>,
    pub uncommited_block_queue: HashMap<String, Block>
}

impl Blockchain {
    pub fn new () -> Self {
        Blockchain {
            blocks: vec![],
            block_hash_map: HashMap::new(),
            uncommited_block_queue: HashMap::new()
        }
    }

    pub fn add_block (&mut self, block: block::Block) {

        self.blocks.push(block.clone());

        self.block_hash_map.insert(block.hash(), block);
    }

    pub fn block_extends_chain_head(& self, block: &Block) -> bool{

        match self.get_chain_head() {
            //Check if this block prev_block_hash is equal to current head block hash
            Some(head_block) => block.header.prev_block_hash == head_block.hash(),
            // If there is no block in the blockchain - accept
            _ => true
        }
    }

    pub fn get_chain_head(& self) -> Option<Block> {
        self.blocks.last().cloned()
    }

    pub fn remove_head(&mut self){
        self.blocks.remove(self.blocks.len()-1);
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

    pub fn is_known_block(&self, block_hash: &String) -> bool {
        self.block_hash_map.contains_key(block_hash) ||
            self.uncommited_block_queue.contains_key(block_hash)
    }

    pub fn remove_from_uncommitted_block_que(&mut self, block_hash: &String) -> Option<Block> {
        debug!("Removed from uncommited block que - {}", block_hash);
        self.uncommited_block_queue.remove(block_hash)
    }

    pub fn add_to_uncommitted_block_que(&mut self, block: Block) -> Option<Block> {
        debug!("Added to uncommited block que - {}", block.hash());
        self.uncommited_block_queue.insert(block.hash(), block)
    }
}