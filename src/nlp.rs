use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentRequirements {
    pub cloud_provider: CloudProvider,
    pub application_type: Option<ApplicationType>,
    pub scaling_requirements: ScalingRequirements,
    pub database_requirements: Vec<DatabaseType>,
    pub environment_variables: HashMap<String, String>,
    pub port_requirements: Vec<u16>,
    pub ssl_required: bool,
    pub custom_domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CloudProvider {
    AWS,
    GCP,
    Azure,
    DigitalOcean,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApplicationType {
    Flask,
    Django,
    NodeJS,
    React,
    NextJS,
    Express,
    FastAPI,
    Rails,
    Spring,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScalingRequirements {
    Single,
    AutoScale,
    LoadBalanced,
    Serverless,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DatabaseType {
    PostgreSQL,
    MySQL,
    MongoDB,
    Redis,
    SQLite,
    None,
}

impl Default for DeploymentRequirements {
    fn default() -> Self {
        Self {
            cloud_provider: CloudProvider::AWS,
            application_type: None,
            scaling_requirements: ScalingRequirements::Single,
            database_requirements: vec![DatabaseType::None],
            environment_variables: HashMap::new(),
            port_requirements: vec![80, 443],
            ssl_required: false,
            custom_domain: None,
        }
    }
}

