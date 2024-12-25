use std::borrow::ToOwned;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    pub permissions: Vec<String>
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    pub groups: Vec<String>
}