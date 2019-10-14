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
}