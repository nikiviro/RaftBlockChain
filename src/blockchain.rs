use crate::Block;

pub mod block;

pub struct Blockchain {
    pub blocks: Vec<block::Block>,
}

impl Blockchain {
    pub fn new () -> Self {
        Blockchain {
            blocks: vec![],
        }
    }

    pub fn add_block (&mut self, block: block::Block) {

        self.blocks.push(block);
    }

    pub fn get_last_block(&mut self) -> Option<Block> {
        self.blocks.last().cloned()
    }
}