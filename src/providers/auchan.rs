//! Auchan: not implemented yet, every command answers "not supported".

use super::Shop;
use anyhow::Result;

pub struct Auchan;

impl Auchan {
    pub fn open(_headless: bool) -> Result<Auchan> {
        Ok(Auchan)
    }
}

impl Shop for Auchan {}
