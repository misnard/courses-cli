//! Intermarché: not implemented yet, every command answers "not supported".

use super::Shop;
use anyhow::Result;

pub struct Intermarche;

impl Intermarche {
    pub fn open(_headless: bool) -> Result<Intermarche> {
        Ok(Intermarche)
    }
}

impl Shop for Intermarche {}
