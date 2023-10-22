use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct MilestoneVersions {
    pub timestamp: String,
    pub milestones: HashMap<String, Milestone>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Milestone {
    pub milestone: String,
    pub version: String,
    pub revision: String,
}
