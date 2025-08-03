use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::io::{self, Write};
use log::info;

use crate::nlp::CloudProvider;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudCredentials {
    pub aws: Option<AwsCredentials>,
    pub gcp: Option<GcpCredentials>,
    pub azure: Option<AzureCredentials>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: Option<String>,
    pub session_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcpCredentials {
    pub service_account_key: String, // JSON key content
    pub project_id: String,
    pub region: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureCredentials {
    pub client_id: String,
    pub client_secret: String,
    pub tenant_id: String,
    pub subscription_id: String,
}

impl CloudCredentials {
    pub fn new() -> Self {
        Self {
            aws: None,
            gcp: None,
            azure: None,
        }
    }

    pub fn load_from_file() -> Result<Self> {
        let config_path = get_config_path()?;
        
        if !config_path.exists() {
            info!("📝 No existing credentials found, starting fresh");
            return Ok(Self::new());
        }

        let content = fs::read_to_string(&config_path)?;
        let credentials: CloudCredentials = serde_json::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse credentials file: {}", e))?;
        
        info!("✅ Loaded existing credentials from: {}", config_path.display());
        Ok(credentials)
    }

    pub fn save_to_file(&self) -> Result<()> {
        let config_path = get_config_path()?;
        
        // Ensure parent directory exists
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        fs::write(&config_path, content)?;
        
        // Set file permissions to be readable only by owner
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&config_path)?.permissions();
            perms.set_mode(0o600); // rw-------
            fs::set_permissions(&config_path, perms)?;
        }

        info!("💾 Credentials saved to: {}", config_path.display());
        Ok(())
    }

    pub fn has_credentials_for(&self, provider: &CloudProvider) -> bool {
        match provider {
            CloudProvider::AWS => self.aws.is_some(),
            CloudProvider::GCP => self.gcp.is_some(),
            CloudProvider::Azure => self.azure.is_some(),
            CloudProvider::DigitalOcean => false, // Not implemented yet
            CloudProvider::Unknown => false,
        }
    }

    pub fn get_credentials_for(&self, provider: &CloudProvider) -> Option<HashMap<String, String>> {
        match provider {
            CloudProvider::AWS => {
                self.aws.as_ref().map(|aws| {
                    let mut env_vars = HashMap::new();
                    env_vars.insert("AWS_ACCESS_KEY_ID".to_string(), aws.access_key_id.clone());
                    env_vars.insert("AWS_SECRET_ACCESS_KEY".to_string(), aws.secret_access_key.clone());
                    
                    if let Some(region) = &aws.region {
                        env_vars.insert("AWS_DEFAULT_REGION".to_string(), region.clone());
                    }
                    
                    if let Some(token) = &aws.session_token {
                        env_vars.insert("AWS_SESSION_TOKEN".to_string(), token.clone());
                    }
                    
                    env_vars
                })
            },
            CloudProvider::GCP => {
                self.gcp.as_ref().map(|gcp| {
                    let mut env_vars = HashMap::new();
                    
                    // Write service account key to temp file
                    if let Ok(key_path) = write_gcp_service_account_key(&gcp.service_account_key) {
                        env_vars.insert("GOOGLE_APPLICATION_CREDENTIALS".to_string(), key_path);
                    }
                    
                    env_vars.insert("GOOGLE_PROJECT".to_string(), gcp.project_id.clone());
                    
                    if let Some(region) = &gcp.region {
                        env_vars.insert("GOOGLE_REGION".to_string(), region.clone());
                    }
                    
                    env_vars
                })
            },
            CloudProvider::Azure => {
                self.azure.as_ref().map(|azure| {
                    let mut env_vars = HashMap::new();
                    env_vars.insert("ARM_CLIENT_ID".to_string(), azure.client_id.clone());
                    env_vars.insert("ARM_CLIENT_SECRET".to_string(), azure.client_secret.clone());
                    env_vars.insert("ARM_TENANT_ID".to_string(), azure.tenant_id.clone());
                    env_vars.insert("ARM_SUBSCRIPTION_ID".to_string(), azure.subscription_id.clone());
                    env_vars
                })
            },
            CloudProvider::DigitalOcean => None,
            CloudProvider::Unknown => None,
        }
    }
}

pub async fn prompt_for_credentials(provider: &CloudProvider) -> Result<()> {
    let mut credentials = CloudCredentials::load_from_file().unwrap_or_else(|_| CloudCredentials::new());
    
    println!("\n🔐 Setting up credentials for {:?}", provider);
    println!("==========================================");
    
    match provider {
        CloudProvider::AWS => {
            prompt_aws_credentials(&mut credentials).await?;
        },
        CloudProvider::GCP => {
            prompt_gcp_credentials(&mut credentials).await?;
        },
        CloudProvider::Azure => {
            prompt_azure_credentials(&mut credentials).await?;
        },
        CloudProvider::DigitalOcean => {
            return Err(anyhow!("DigitalOcean credentials not yet supported"));
        },
        CloudProvider::Unknown => {
            return Err(anyhow!("Unknown cloud provider"));
        },
    }
    
    credentials.save_to_file()?;
    println!("✅ Credentials saved successfully!");
    
    Ok(())
}

async fn prompt_aws_credentials(credentials: &mut CloudCredentials) -> Result<()> {
    println!("🔑 AWS Credentials Setup");
    println!("You can find these in AWS Console > IAM > Users > Security credentials");
    println!();

    print!("AWS Access Key ID: ");
    io::stdout().flush()?;
    let mut access_key = String::new();
    io::stdin().read_line(&mut access_key)?;
    let access_key = access_key.trim().to_string();

    print!("AWS Secret Access Key: ");
    io::stdout().flush()?;
    let mut secret_key = String::new();
    io::stdin().read_line(&mut secret_key)?;
    let secret_key = secret_key.trim().to_string();

    print!("AWS Region (default: us-east-1): ");
    io::stdout().flush()?;
    let mut region = String::new();
    io::stdin().read_line(&mut region)?;
    let region = region.trim();
    let region = if region.is_empty() {
        "us-east-1".to_string()
    } else {
        region.to_string()
    };

    print!("Session Token (optional, press Enter to skip): ");
    io::stdout().flush()?;
    let mut session_token = String::new();
    io::stdin().read_line(&mut session_token)?;
    let session_token = session_token.trim();
    
    if access_key.is_empty() || secret_key.is_empty() {
        return Err(anyhow!("Access Key ID and Secret Access Key are required"));
    }

    credentials.aws = Some(AwsCredentials {
        access_key_id: access_key,
        secret_access_key: secret_key,
        region: Some(region),
        session_token: if session_token.is_empty() { None } else { Some(session_token.to_string()) },
    });

    println!("✅ AWS credentials configured");
    Ok(())
}

async fn prompt_gcp_credentials(credentials: &mut CloudCredentials) -> Result<()> {
    println!("🔑 Google Cloud Credentials Setup");
    println!("You need a service account JSON key file.");
    println!("Get it from: GCP Console > IAM & Admin > Service Accounts > Create Key");
    println!();

    print!("Project ID: ");
    io::stdout().flush()?;
    let mut project_id = String::new();
    io::stdin().read_line(&mut project_id)?;
    let project_id = project_id.trim().to_string();

    print!("Service Account Key File Path: ");
    io::stdout().flush()?;
    let mut key_path = String::new();
    io::stdin().read_line(&mut key_path)?;
    let key_path = key_path.trim();

    print!("Region (default: us-central1): ");
    io::stdout().flush()?;
    let mut region = String::new();
    io::stdin().read_line(&mut region)?;
    let region = region.trim();
    let region = if region.is_empty() {
        "us-central1".to_string()
    } else {
        region.to_string()
    };

    if project_id.is_empty() || key_path.is_empty() {
        return Err(anyhow!("Project ID and Service Account Key file are required"));
    }

    // Read the service account key file
    let key_content = fs::read_to_string(key_path)
        .map_err(|e| anyhow!("Failed to read service account key file: {}", e))?;

    // Validate it's valid JSON
    serde_json::from_str::<serde_json::Value>(&key_content)
        .map_err(|e| anyhow!("Invalid JSON in service account key file: {}", e))?;

    credentials.gcp = Some(GcpCredentials {
        service_account_key: key_content,
        project_id,
        region: Some(region),
    });

    println!("✅ GCP credentials configured");
    Ok(())
}

async fn prompt_azure_credentials(credentials: &mut CloudCredentials) -> Result<()> {
    println!("🔑 Azure Credentials Setup");
    println!("You need to create a service principal in Azure.");
    println!("Get these from: Azure Portal > App registrations > New registration");
    println!();

    print!("Client ID (Application ID): ");
    io::stdout().flush()?;
    let mut client_id = String::new();
    io::stdin().read_line(&mut client_id)?;
    let client_id = client_id.trim().to_string();

    print!("Client Secret: ");
    io::stdout().flush()?;
    let mut client_secret = String::new();
    io::stdin().read_line(&mut client_secret)?;
    let client_secret = client_secret.trim().to_string();

    print!("Tenant ID (Directory ID): ");
    io::stdout().flush()?;
    let mut tenant_id = String::new();
    io::stdin().read_line(&mut tenant_id)?;
    let tenant_id = tenant_id.trim().to_string();

    print!("Subscription ID: ");
    io::stdout().flush()?;
    let mut subscription_id = String::new();
    io::stdin().read_line(&mut subscription_id)?;
    let subscription_id = subscription_id.trim().to_string();

    if client_id.is_empty() || client_secret.is_empty() || tenant_id.is_empty() || subscription_id.is_empty() {
        return Err(anyhow!("All Azure credential fields are required"));
    }

    credentials.azure = Some(AzureCredentials {
        client_id,
        client_secret,
        tenant_id,
        subscription_id,
    });

    println!("✅ Azure credentials configured");
    Ok(())
}

fn get_config_path() -> Result<PathBuf> {
    let home_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("Could not find home directory"))?;
    
    Ok(home_dir.join(".autodeployment").join("credentials.json"))
}

fn write_gcp_service_account_key(key_content: &str) -> Result<String> {
    let temp_dir = std::env::temp_dir();
    let key_file = temp_dir.join("gcp_service_account.json");
    
    fs::write(&key_file, key_content)?;
    
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&key_file)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&key_file, perms)?;
    }
    
    Ok(key_file.to_string_lossy().to_string())
}

pub fn check_credentials_status() -> Result<()> {
    let credentials = CloudCredentials::load_from_file().unwrap_or_else(|_| CloudCredentials::new());
    
    println!("\n🔐 Credentials Status:");
    println!("====================");
    
    println!("AWS:   {}", if credentials.aws.is_some() { "✅ Configured" } else { "❌ Not set" });
    println!("GCP:   {}", if credentials.gcp.is_some() { "✅ Configured" } else { "❌ Not set" });
    println!("Azure: {}", if credentials.azure.is_some() { "✅ Configured" } else { "❌ Not set" });
    
    if credentials.aws.is_none() && credentials.gcp.is_none() && credentials.azure.is_none() {
        println!("\n💡 Set up credentials with: cargo run -- credentials <cloud>");
        println!("   Example: cargo run -- credentials aws");
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cloud_credentials_new() {
        let creds = CloudCredentials::new();
        assert!(creds.aws.is_none());
        assert!(creds.gcp.is_none());
        assert!(creds.azure.is_none());
    }
    
    #[test]
    fn test_has_credentials_for() {
        let mut creds = CloudCredentials::new();
        assert!(!creds.has_credentials_for(&CloudProvider::AWS));
        
        creds.aws = Some(AwsCredentials {
            access_key_id: "test".to_string(),
            secret_access_key: "test".to_string(),
            region: None,
            session_token: None,
        });
        
        assert!(creds.has_credentials_for(&CloudProvider::AWS));
        assert!(!creds.has_credentials_for(&CloudProvider::GCP));
    }
}