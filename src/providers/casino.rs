//! Casino proximité (mescoursesdeproximite.com): not implemented yet, every command answers "not supported".

use super::Shop;
use anyhow::Result;

pub struct Casino;

impl Casino {
    pub fn open(_headless: bool) -> Result<Casino> {
        Ok(Casino)
    }
}

impl Shop for Casino {}
