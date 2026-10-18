//! Chronodrive: not implemented yet, every command answers "not supported".

use super::Shop;
use anyhow::Result;

pub struct Chronodrive;

impl Chronodrive {
    pub fn open(_headless: bool) -> Result<Chronodrive> {
        Ok(Chronodrive)
    }
}

impl Shop for Chronodrive {}
