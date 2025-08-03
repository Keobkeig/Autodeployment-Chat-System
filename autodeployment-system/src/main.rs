use clap::{Parser, Subcommand};
use anyhow::Result;
use log::{info, error};

mod deployment;
mod repository;
mod infrastructure;
mod nlp;
mod ai_nlp;
mod credentials;

#[derive(Parser)]
#[clap(name = "autodeployment")]
#[clap(about = "Automate application deployment based on natural language input")]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Deploy {
        #[clap(short, long)]
        description: String,
        
        #[clap(short, long)]
        repository: String,
        
        #[clap(short, long, default_value = "aws")]
        cloud_provider: String,
        
        #[clap(long)]
        dry_run: bool,

        #[clap(long)]
        force_deploy: bool,
    },
    Chat {
        #[clap(short, long)]
        repository: Option<String>,
    },
    Credentials {
        #[clap(subcommand)]
        command: CredentialsCommand,
    },
}

#[derive(Subcommand)]
enum CredentialsCommand {
    Setup {
        #[clap(help = "Cloud provider: aws, gcp, azure")]
        provider: String,
    },
    Status,
    Clear {
        #[clap(help = "Cloud provider to clear: aws, gcp, azure, all")]
        provider: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenv::dotenv().ok();
    
    env_logger::init();
    
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Deploy { description, repository, cloud_provider, dry_run, force_deploy } => {
            info!("Starting deployment process...");
            info!("Description: {}", description);
            info!("Repository: {}", repository);
            info!("Cloud Provider: {}", cloud_provider);
            
            let deployment_result = deployment::deploy_application(
                &description,
                &repository,
                &cloud_provider,
                dry_run,
                force_deploy,
            ).await;
            
            match deployment_result {
                Ok(deployment_info) => {
                    println!("🚀 Deployment successful!");
                    println!("Application URL: {}", deployment_info.url);
                    println!("Infrastructure: {}", deployment_info.infrastructure_type);
                }
                Err(e) => {
                    error!("Deployment failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Chat { repository } => {
            info!("Starting interactive chat mode...");
            deployment::interactive_chat(repository).await?;
        }
        Commands::Credentials { command } => {
            match command {
                CredentialsCommand::Setup { provider } => {
                    let cloud_provider = match provider.to_lowercase().as_str() {
                        "aws" => nlp::CloudProvider::AWS,
                        "gcp" | "google" => nlp::CloudProvider::GCP,
                        "azure" => nlp::CloudProvider::Azure,
                        _ => {
                            error!("Unsupported cloud provider: {}. Use: aws, gcp, azure", provider);
                            std::process::exit(1);
                        }
                    };
                    
                    if let Err(e) = credentials::prompt_for_credentials(&cloud_provider).await {
                        error!("Failed to set up credentials: {}", e);
                        std::process::exit(1);
                    }
                }
                CredentialsCommand::Status => {
                    if let Err(e) = credentials::check_credentials_status() {
                        error!("Failed to check credentials: {}", e);
                        std::process::exit(1);
                    }
                }
                CredentialsCommand::Clear { provider } => {
                    if let Err(e) = clear_credentials(&provider).await {
                        error!("Failed to clear credentials: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
    }
    
    Ok(())
}

async fn clear_credentials(provider: &str) -> Result<()> {
    use credentials::CloudCredentials;
    
    let mut credentials = CloudCredentials::load_from_file().unwrap_or_else(|_| CloudCredentials::new());
    
    match provider.to_lowercase().as_str() {
        "aws" => {
            credentials.aws = None;
            println!("✅ AWS credentials cleared");
        }
        "gcp" | "google" => {
            credentials.gcp = None;
            println!("✅ GCP credentials cleared");
        }
        "azure" => {
            credentials.azure = None;
            println!("✅ Azure credentials cleared");
        }
        "all" => {
            credentials.aws = None;
            credentials.gcp = None;
            credentials.azure = None;
            println!("✅ All credentials cleared");
        }
        _ => {
            return Err(anyhow::anyhow!("Unknown provider: {}. Use: aws, gcp, azure, all", provider));
        }
    }
    
    credentials.save_to_file()?;
    Ok(())
}
