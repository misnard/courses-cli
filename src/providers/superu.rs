//! Super U (Courses U): not implemented yet, every command answers "not supported".

use super::Shop;
use anyhow::Result;

pub struct Superu;

impl Superu {
    pub fn open(_headless: bool) -> Result<Superu> {
        Ok(Superu)
    }
}

impl Shop for Superu {}
