use anyhow::{Result, anyhow};
use log::{info, warn, error};
use regex::Regex;
use std::io::{self, Write};
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use walkdir::WalkDir;

use crate::ai_nlp;
use crate::repository::{clone_repository, analyze_repository, RepositoryAnalysis};
use crate::infrastructure::{decide_infrastructure, provision_infrastructure, DeploymentResult, InfrastructureDecision};
use crate::credentials::CloudCredentials;

pub async fn deploy_application(
    description: &str,
    repository: &str,
    cloud_provider: Option<&str>,
    dry_run: bool,
    force_deploy: bool,
) -> Result<DeploymentResult> {
    info!("🚀 Starting deployment process...");
    
    // Parse natural language requirements using AI
    info!("📝 Parsing deployment requirements from description using AI...");
    let mut requirements = ai_nlp::parse_deployment_requirements(description).await?;
    
    // Use CLI cloud provider if provided, otherwise use LLM-parsed provider
    if let Some(provider) = cloud_provider {
        requirements.cloud_provider = match provider.to_lowercase().as_str() {
            "aws" => crate::nlp::CloudProvider::AWS,
            "gcp" | "google" => crate::nlp::CloudProvider::GCP,
            "azure" => crate::nlp::CloudProvider::Azure,
            "digitalocean" => crate::nlp::CloudProvider::DigitalOcean,
            _ => {
                warn!("Unknown cloud provider '{}', defaulting to AWS", provider);
                crate::nlp::CloudProvider::AWS
            }
        };
    }
    // If no CLI provider specified, use what the LLM parsed from description

    // Check credentials for non-dry-run deployments
    if !dry_run || force_deploy {
        let credentials = CloudCredentials::load_from_file()
            .unwrap_or_else(|_| CloudCredentials::new());
        
        if !credentials.has_credentials_for(&requirements.cloud_provider) {
            let provider_str = match requirements.cloud_provider {
                crate::nlp::CloudProvider::AWS => "aws",
                crate::nlp::CloudProvider::GCP => "gcp",
                crate::nlp::CloudProvider::Azure => "azure",
                crate::nlp::CloudProvider::DigitalOcean => "digitalocean",
                crate::nlp::CloudProvider::Unknown => "aws", // fallback
            };
            return Err(anyhow!(
                "❌ No credentials found for {:?}.\n💡 Set up credentials with: cargo run -- credentials setup {}",
                requirements.cloud_provider,
                provider_str
            ));
        }
        
        info!("✅ Credentials found for {:?}", requirements.cloud_provider);
    }
    
    info!("Requirements parsed: Cloud Provider: {:?}", requirements.cloud_provider);
    
    // Clone and analyze repository
    info!("📥 Cloning repository: {}", repository);
    let temp_repo = clone_repository(repository).await?;
    
    // Get public IP first to replace localhost references before analysis
    info!("🌐 Getting public IP for localhost replacement...");
    let public_ip = get_public_ip().await.unwrap_or_else(|_| "0.0.0.0".to_string());
    
    if let Err(e) = replace_localhost_in_repository(temp_repo.path(), &public_ip) {
        warn!("⚠️ Failed to replace localhost references: {}", e);
    } else {
        info!("✅ Successfully updated localhost references in repository files");
    }
    
    info!("🔍 Analyzing repository structure...");
    let analysis = analyze_repository(temp_repo.path())?;
    
    info!("Analysis complete: App Type: {:?}", analysis.app_type);
    info!("Dependencies found: {}", analysis.dependencies.len());
    info!("Exposed ports: {:?}", analysis.exposed_ports);
    
    
    // Make infrastructure decision
    info!("🏗️ Determining optimal infrastructure using AI...");
    let infrastructure_decision = decide_infrastructure(&requirements, &analysis, description, repository).await?;
    
    info!("Infrastructure decision: {:?}", infrastructure_decision.deployment_type);
    info!("Justification: {}", infrastructure_decision.justification);
    
    // Generate Terraform files (even for dry-run to allow review)
    info!("📄 Generating Terraform configuration files...");
    let work_dir = tempfile::tempdir()?;
    let file_generation_result = provision_infrastructure(
        &infrastructure_decision,
        repository,
        work_dir.path(),
        true, // Always generate files for review
        &requirements.cloud_provider,
    ).await?;
    
    if dry_run {
        info!("🧪 Dry run complete - no infrastructure will be provisioned");
        return Ok(DeploymentResult {
            url: "dry-run".to_string(),
            infrastructure_type: format!("{:?}", infrastructure_decision.deployment_type),
            public_ip: None,
            logs: file_generation_result.logs,
        });
    }
    
    // Provision infrastructure (sed will handle localhost replacement in startup script)
    info!("☁️ Provisioning infrastructure...");
    let work_dir = tempfile::tempdir()?;
    let mut deployment_result = provision_infrastructure(
        &infrastructure_decision,
        repository, // Use original repository - sed will fix localhost in startup script
        work_dir.path(),
        false, // Actually deploy  
        &requirements.cloud_provider,
    ).await?;
    
    // Fix URL if it contains "unknown" 
    if let Some(public_ip) = &deployment_result.public_ip {
        if deployment_result.url.contains("unknown") {
            deployment_result.url = format!("http://{}:5000", public_ip);
        }
    }
    
    info!("✅ Deployment completed successfully!");
    info!("🌐 Application URL: {}", deployment_result.url);
    
    Ok(deployment_result)
}

pub async fn interactive_chat(repository: Option<String>) -> Result<()> {
    println!("🤖 Welcome to AutoDeployment Chat!");
    println!("Type 'help' for commands, 'quit' to exit.");
    
    let mut current_repo: Option<(String, TempDir, RepositoryAnalysis)> = None;
    
    // If repository provided, analyze it upfront
    if let Some(repo_url) = repository {
        println!("📥 Analyzing repository: {}", repo_url);
        match clone_repository(&repo_url).await {
            Ok(temp_repo) => {
                match analyze_repository(temp_repo.path()) {
                    Ok(analysis) => {
                        println!("✅ Repository analyzed successfully!");
                        println!("   App Type: {:?}", analysis.app_type);
                        println!("   Dependencies: {}", analysis.dependencies.len());
                        println!("   Exposed Ports: {:?}", analysis.exposed_ports);
                        current_repo = Some((repo_url, temp_repo, analysis));
                    },
                    Err(e) => {
                        error!("Failed to analyze repository: {}", e);
                    }
                }
            },
            Err(e) => {
                error!("Failed to clone repository: {}", e);
            }
        }
    }
    
    loop {
        print!("\n> ");
        io::stdout().flush()?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();
        
        if input.is_empty() {
            continue;
        }
        
        match input.to_lowercase().as_str() {
            "quit" | "exit" => {
                println!("👋 Goodbye!");
                break;
            },
            "help" => {
                print_help();
            },
            "status" => {
                if let Some((repo_url, _, analysis)) = &current_repo {
                    print_status(repo_url, analysis);
                } else {
                    println!("❌ No repository loaded. Use 'load <repo_url>' to load a repository.");
                }
            },
            _ if input.starts_with("load ") => {
                let repo_url = input.strip_prefix("load ").unwrap().trim();
                match load_repository(repo_url).await {
                    Ok((temp_repo, analysis)) => {
                        println!("✅ Repository loaded successfully!");
                        current_repo = Some((repo_url.to_string(), temp_repo, analysis));
                    },
                    Err(e) => {
                        error!("Failed to load repository: {}", e);
                    }
                }
            },
            _ if input.starts_with("deploy ") => {
                let description = input.strip_prefix("deploy ").unwrap().trim();
                if let Some((repo_url, _, analysis)) = &current_repo {
                    match deploy_with_chat(description, repo_url, analysis).await {
                        Ok(result) => {
                            println!("🚀 Deployment successful!");
                            println!("📍 URL: {}", result.url);
                            println!("🏗️ Infrastructure: {}", result.infrastructure_type);
                        },
                        Err(e) => {
                            error!("Deployment failed: {}", e);
                        }
                    }
                } else {
                    println!("❌ No repository loaded. Use 'load <repo_url>' first.");
                }
            },
            _ if input.starts_with("plan ") => {
                let description = input.strip_prefix("plan ").unwrap().trim();
                if let Some((_repo_url, _, analysis)) = &current_repo {
                    match plan_deployment(description, analysis).await {
                        Ok(decision) => {
                            print_deployment_plan(&decision);
                        },
                        Err(e) => {
                            error!("Planning failed: {}", e);
                        }
                    }
                } else {
                    println!("❌ No repository loaded. Use 'load <repo_url>' first.");
                }
            },
            _ => {
                // Treat as a deployment description if repository is loaded
                if let Some((_repo_url, _, _analysis)) = &current_repo {
                    println!("🤔 Did you mean to deploy? Use 'deploy {}' to proceed.", input);
                    println!("    Or use 'plan {}' to see the deployment plan.", input);
                } else {
                    println!("❓ Unknown command. Type 'help' for available commands.");
                }
            }
        }
    }
    
    Ok(())
}

async fn load_repository(repo_url: &str) -> Result<(TempDir, RepositoryAnalysis)> {
    println!("📥 Cloning repository...");
    let temp_repo = clone_repository(repo_url).await?;
    
    println!("🔍 Analyzing repository...");
    let analysis = analyze_repository(temp_repo.path())?;
    
    println!("   App Type: {:?}", analysis.app_type);
    println!("   Package Manager: {:?}", analysis.package_manager);
    println!("   Dependencies: {}", analysis.dependencies.len());
    println!("   Build Required: {}", analysis.requires_build_step);
    println!("   Exposed Ports: {:?}", analysis.exposed_ports);
    
    Ok((temp_repo, analysis))
}

async fn deploy_with_chat(
    description: &str,
    repo_url: &str,
    analysis: &RepositoryAnalysis,
) -> Result<DeploymentResult> {
    println!("📝 Parsing deployment requirements using AI...");
    let requirements = ai_nlp::parse_deployment_requirements(description).await?;
    
    println!("🏗️ Planning infrastructure using AI...");
    let decision = decide_infrastructure(&requirements, analysis, description, "https://github.com/Arvo-AI/hello_world/tree/main").await?;
    
    print_deployment_plan(&decision);
    
    print!("🚀 Proceed with deployment? (y/N): ");
    io::stdout().flush()?;
    
    let mut confirm = String::new();
    io::stdin().read_line(&mut confirm)?;
    
    if confirm.trim().to_lowercase() != "y" {
        return Err(anyhow!("Deployment cancelled by user"));
    }
    
    println!("☁️ Provisioning infrastructure...");
    let work_dir = tempfile::tempdir()?;
    let result = provision_infrastructure(&decision, repo_url, work_dir.path(), false, &requirements.cloud_provider).await?;
    
    Ok(result)
}

async fn plan_deployment(description: &str, analysis: &RepositoryAnalysis) -> Result<InfrastructureDecision> {
    let requirements = ai_nlp::parse_deployment_requirements(description).await?;
    let decision = decide_infrastructure(&requirements, analysis, description, "https://github.com/Arvo-AI/hello_world/tree/main").await?;
    Ok(decision)
}

fn print_help() {
    println!("\n📚 Available Commands:");
    println!("  help                    - Show this help message");
    println!("  load <repo_url>         - Load and analyze a repository");
    println!("  status                  - Show current repository status");
    println!("  plan <description>      - Plan deployment without executing");
    println!("  deploy <description>    - Deploy the application");
    println!("  quit/exit               - Exit the chat");
    println!("\n💡 Examples:");
    println!("  load https://github.com/Arvo-AI/hello_world");
    println!("  plan Deploy this Flask app on AWS");
    println!("  deploy Deploy with auto-scaling on GCP");
}

fn print_status(repo_url: &str, analysis: &RepositoryAnalysis) {
    println!("\n📊 Repository Status:");
    println!("  URL: {}", repo_url);
    println!("  App Type: {:?}", analysis.app_type);
    println!("  Package Manager: {:?}", analysis.package_manager);
    println!("  Dependencies: {}", analysis.dependencies.len());
    println!("  Build Required: {}", analysis.requires_build_step);
    println!("  Exposed Ports: {:?}", analysis.exposed_ports);
    println!("  Static Files: {:?}", analysis.static_files_dir);
    println!("  Database Migrations: {}", analysis.database_migrations);
    
    if !analysis.environment_variables.is_empty() {
        println!("  Environment Variables: {:?}", analysis.environment_variables);
    }
    
    println!("\n🛠️ Build Commands:");
    for cmd in &analysis.build_commands {
        println!("    {}", cmd);
    }
    
    println!("\n▶️ Start Commands:");
    for cmd in &analysis.start_commands {
        println!("    {}", cmd);
    }
}

fn print_deployment_plan(decision: &InfrastructureDecision) {
    println!("\n📋 Deployment Plan:");
    println!("  Infrastructure: {:?}", decision.deployment_type);
    println!("  Instance Type: {}", decision.instance_type);
    println!("  Estimated Cost: ${:.2}/month", decision.estimated_cost);
    println!("  Justification: {}", decision.justification);
    
    println!("\n🏗️ Resources to be created:");
    for resource in &decision.terraform_config.resources {
        println!("  - {} ({})", resource.name, resource.resource_type);
    }
    
    if !decision.terraform_config.variables.is_empty() {
        println!("\n⚙️ Required Variables:");
        for (var_name, description) in &decision.terraform_config.variables {
            println!("  - {}: {}", var_name, description);
        }
    }
}

/// Get the public IP address of the current machine
async fn get_public_ip() -> Result<String> {
    let response = reqwest::get("https://api.ipify.org?format=text").await?;
    let ip = response.text().await?;
    Ok(ip.trim().to_string())
}

/// Replace localhost references in repository files with the actual public IP
fn replace_localhost_in_repository(repo_path: &Path, public_ip: &str) -> Result<()> {
    info!("🔄 Replacing localhost references with {} in repository files", public_ip);
    
    // Common file extensions that might contain localhost references
    let extensions = &[".py", ".js", ".ts", ".html", ".css", ".json", ".yaml", ".yml", ".toml", ".cfg", ".ini"];
    
    // Find all relevant files
    for entry in WalkDir::new(repo_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if extensions.iter().any(|&e| e.trim_start_matches('.') == ext.to_string_lossy()) {
                    replace_localhost_in_file(path, public_ip)?;
                }
            }
        }
    }
    
    Ok(())
}

/// Replace localhost references in a single file
fn replace_localhost_in_file(file_path: &Path, public_ip: &str) -> Result<()> {
    if let Ok(content) = fs::read_to_string(file_path) {
        let original_content = content.clone();
        
        let mut modified_content = content.clone();
        
        // For Flask specifically, ensure app.run() uses 0.0.0.0 for external access
        if file_path.extension().map_or(false, |ext| ext == "py") {
            // Use regex to replace Flask host parameters more robustly BEFORE general localhost replacement
            
            // Replace app.run() with no host specified
            modified_content = modified_content
                .replace("app.run()", "app.run(host='0.0.0.0', port=5000)");
            
            // Replace localhost host parameters (with and without quotes)
            let localhost_patterns = [
                (r#"host\s*=\s*"localhost""#, r#"host="0.0.0.0""#),
                (r#"host\s*=\s*'localhost'"#, r#"host='0.0.0.0'"#),
                (r#"host\s*=\s*"127\.0\.0\.1""#, r#"host="0.0.0.0""#),
                (r#"host\s*=\s*'127\.0\.0\.1'"#, r#"host='0.0.0.0'"#),
            ];
            
            for (pattern, replacement) in localhost_patterns {
                if let Ok(re) = Regex::new(pattern) {
                    modified_content = re.replace_all(&modified_content, replacement).to_string();
                }
            }
            
            // Now replace remaining localhost references with public IP (for frontend API calls, etc.)
            modified_content = modified_content
                .replace("localhost", public_ip)
                .replace("127.0.0.1", public_ip);
        } else {
            // For non-Python files (HTML, JS, etc.), replace localhost with public IP
            modified_content = modified_content
                .replace("localhost", public_ip)
                .replace("127.0.0.1", public_ip)
                .replace("0.0.0.0", public_ip);
        }
        
        // Only write if content changed
        if modified_content != original_content {
            fs::write(file_path, modified_content)?;
        }
    }
    
    Ok(())
}

