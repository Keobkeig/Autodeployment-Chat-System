use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use which::which;
use chrono::Utc;
use log::info;

use crate::nlp::{ApplicationType, CloudProvider, DeploymentRequirements, ScalingRequirements};
use crate::repository::RepositoryAnalysis;
use crate::ai_nlp;
use crate::credentials::CloudCredentials;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfrastructureDecision {
    pub deployment_type: DeploymentType,
    pub instance_type: String,
    pub terraform_config: TerraformConfig,
    pub estimated_cost: f64,
    pub justification: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeploymentType {
    SingleVM,
    ContainerService,
    Serverless,
    Kubernetes,
    StaticSite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerraformConfig {
    pub provider: String,
    pub resources: Vec<TerraformResource>,
    pub variables: HashMap<String, serde_json::Value>,
    pub outputs: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerraformResource {
    pub resource_type: String,
    pub name: String,
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentResult {
    pub url: String,
    pub infrastructure_type: String,
    pub public_ip: Option<String>,
    pub logs: Vec<String>,
}

pub async fn decide_infrastructure(
    requirements: &DeploymentRequirements,
    analysis: &RepositoryAnalysis,
    description: &str,
    repository_url: &str,
) -> Result<InfrastructureDecision> {
    let deployment_type = determine_deployment_type(requirements, analysis);
    let instance_type = determine_instance_type(&deployment_type, &requirements.cloud_provider);
    let terraform_config = ai_nlp::generate_terraform_with_ai(
        description,
        &requirements.cloud_provider,
        &format!("{:?}", deployment_type),
        &analysis.app_type,
        repository_url,
    ).await?;
    let estimated_cost = estimate_cost(&deployment_type, &requirements.cloud_provider);
    let justification = generate_justification(&deployment_type, requirements, analysis);

    Ok(InfrastructureDecision {
        deployment_type,
        instance_type,
        terraform_config,
        estimated_cost,
        justification,
    })
}

fn determine_deployment_type(
    requirements: &DeploymentRequirements,
    analysis: &RepositoryAnalysis,
) -> DeploymentType {
    match requirements.scaling_requirements {
        ScalingRequirements::Serverless => DeploymentType::Serverless,
        ScalingRequirements::LoadBalanced => DeploymentType::Kubernetes,
        _ => match analysis.app_type {
            ApplicationType::React | ApplicationType::NextJS if !analysis.requires_build_step => {
                DeploymentType::StaticSite
            }
            _ if analysis.docker_config.is_some() => DeploymentType::ContainerService,
            _ => DeploymentType::SingleVM,
        },
    }
}

fn determine_instance_type(
    deployment_type: &DeploymentType,
    cloud_provider: &CloudProvider,
) -> String {
    match (deployment_type, cloud_provider) {
        (DeploymentType::SingleVM, CloudProvider::AWS) => "t3.micro".to_string(),
        (DeploymentType::SingleVM, CloudProvider::GCP) => "e2-micro".to_string(),
        (DeploymentType::SingleVM, CloudProvider::Azure) => "Standard_B1s".to_string(),
        (DeploymentType::ContainerService, CloudProvider::AWS) => "t3.small".to_string(),
        (DeploymentType::ContainerService, CloudProvider::GCP) => "e2-small".to_string(),
        (DeploymentType::Kubernetes, CloudProvider::AWS) => "t3.medium".to_string(),
        (DeploymentType::Kubernetes, CloudProvider::GCP) => "e2-medium".to_string(),
        (DeploymentType::Serverless, _) => "lambda".to_string(),
        (DeploymentType::StaticSite, _) => "static-hosting".to_string(),
        _ => "t3.micro".to_string(),
    }
}

// Note: All Terraform generation now handled by AI in ai_nlp module

fn estimate_cost(deployment_type: &DeploymentType, cloud_provider: &CloudProvider) -> f64 {
    match (deployment_type, cloud_provider) {
        (DeploymentType::SingleVM, CloudProvider::AWS) => 8.76, // t3.micro monthly
        (DeploymentType::SingleVM, CloudProvider::GCP) => 5.32, // e2-micro monthly
        (DeploymentType::ContainerService, _) => 25.0,
        (DeploymentType::Kubernetes, _) => 73.0,
        (DeploymentType::Serverless, _) => 5.0,
        (DeploymentType::StaticSite, _) => 1.0,
        _ => 10.0,
    }
}

fn generate_justification(
    deployment_type: &DeploymentType,
    _requirements: &DeploymentRequirements,
    analysis: &RepositoryAnalysis,
) -> String {
    match deployment_type {
        DeploymentType::SingleVM => {
            format!(
                "Single VM deployment chosen for {:?} application. Cost-effective for simple apps with moderate traffic. Estimated cost: $8.76/month.",
                analysis.app_type
            )
        },
        DeploymentType::ContainerService => {
            "Container service deployment for better scalability and isolation. Suitable for applications with Docker configuration.".to_string()
        },
        DeploymentType::Serverless => {
            "Serverless deployment for automatic scaling and pay-per-use pricing. Ideal for applications with variable traffic.".to_string()
        },
        DeploymentType::Kubernetes => {
            "Kubernetes deployment for high availability and advanced orchestration. Suitable for complex applications requiring load balancing.".to_string()
        },
        DeploymentType::StaticSite => {
            "Static site hosting for frontend applications. Most cost-effective for client-side rendered applications.".to_string()
        },
    }
}

pub async fn provision_infrastructure(
    decision: &InfrastructureDecision,
    repo_url: &str,
    _work_dir: &Path,
    dry_run: bool,
    cloud_provider: &CloudProvider,
) -> Result<DeploymentResult> {
    // Create persistent terraform output directory
    let current_dir = std::env::current_dir()?;
    let terraform_output_dir = current_dir.join("terraform-output");
    fs::create_dir_all(&terraform_output_dir)?;
    
    // Create timestamped subdirectory for this deployment
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let terraform_dir = terraform_output_dir.join(format!("deployment_{}", timestamp));
    fs::create_dir_all(&terraform_dir)?;

    // Generate Terraform files
    generate_terraform_files(&decision.terraform_config, &terraform_dir, repo_url)?;

    let mut logs = Vec::new();
    logs.push("✅ Terraform files generated successfully".to_string());
    logs.push(format!(
        "📁 Generated terraform configuration for {:?}",
        decision.deployment_type
    ));
    logs.push(format!("📄 Files saved to: {}", terraform_dir.display()));
    
    // Log the file locations for easy access
    info!("📁 Terraform files saved to: {}", terraform_dir.display());
    info!("📄 Generated files:");
    info!("   - main.tf");
    info!("   - variables.tf"); 
    info!("   - outputs.tf");
    
    println!("📁 Terraform files saved to: {}", terraform_dir.display());
    println!("📄 You can now review and test the generated Terraform configuration!");

    if dry_run {
        logs.push("🧪 Dry run - no infrastructure provisioned".to_string());
        logs.push("📄 Terraform files available for review and testing".to_string());
        return Ok(DeploymentResult {
            url: "dry-run".to_string(),
            infrastructure_type: format!("{:?}", decision.deployment_type),
            public_ip: None,
            logs,
        });
    }

    // Check if Terraform is installed
    if which("terraform").is_err() {
        return Err(anyhow!(
            "Terraform is not installed. Please install Terraform to deploy for real."
        ));
    }

    // Load and set up credentials
    let credentials = CloudCredentials::load_from_file()
        .unwrap_or_else(|_| CloudCredentials::new());
    
    let env_vars = if let Some(cred_env) = credentials.get_credentials_for(cloud_provider) {
        info!("🔑 Setting up {} credentials for Terraform", format!("{:?}", cloud_provider));
        cred_env
    } else {
        return Err(anyhow!(
            "No credentials found for {:?}. Set up with: cargo run -- credentials setup {}",
            cloud_provider,
            format!("{:?}", cloud_provider).to_lowercase()
        ));
    };

    // Initialize Terraform with credentials
    logs.push("🔧 Initializing Terraform...".to_string());
    let mut cmd = Command::new("terraform");
    cmd.arg("init").current_dir(&terraform_dir);
    
    // Add credentials as environment variables
    for (key, value) in &env_vars {
        cmd.env(key, value);
    }
    
    let output = cmd.output()?;

    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        logs.push(format!("❌ Terraform init failed: {}", error_msg));
        return Err(anyhow!("Terraform init failed: {}", error_msg));
    }

    logs.push("✅ Terraform initialized successfully".to_string());

    // Plan Terraform
    logs.push("📋 Planning Terraform deployment...".to_string());
    let mut cmd = Command::new("terraform");
    cmd.arg("plan").arg("-out=tfplan").current_dir(&terraform_dir);
    
    match cloud_provider {
        CloudProvider::GCP => {
            if let Some(gcp_creds) = &credentials.gcp {
                cmd.arg("-var").arg(format!("project_id={}", gcp_creds.project_id));
                let region = gcp_creds.region.as_deref().unwrap_or("us-central1");
                cmd.arg("-var").arg(format!("region={}", region));
                cmd.arg("-var").arg(format!("zone={}-a", region));
            }
        },
        CloudProvider::AWS => {
            if let Some(aws_creds) = &credentials.aws {
                let region = aws_creds.region.as_deref().unwrap_or("us-east-1");
                cmd.arg("-var").arg(format!("region={}", region));
            }
        },
        _ => {}
    }
    
    // Add credentials as environment variables
    for (key, value) in &env_vars {
        cmd.env(key, value);
    }
    
    let output = cmd.output()?;

    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        logs.push(format!("❌ Terraform plan failed: {}", error_msg));
        return Err(anyhow!("Terraform plan failed: {}", error_msg));
    }

    logs.push("✅ Terraform plan completed successfully".to_string());

    // Apply Terraform
    logs.push("🚀 Applying Terraform configuration...".to_string());
    let mut cmd = Command::new("terraform");
    cmd.arg("apply").arg("-auto-approve").arg("tfplan").current_dir(&terraform_dir);
    
    // Add credentials as environment variables
    for (key, value) in &env_vars {
        cmd.env(key, value);
    }
    
    let output = cmd.output()?;

    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        logs.push(format!("❌ Terraform apply failed: {}", error_msg));
        return Err(anyhow!("Terraform apply failed: {}", error_msg));
    }

    logs.push("✅ Infrastructure provisioned successfully!".to_string());

    // Get outputs
    let mut cmd = Command::new("terraform");
    cmd.arg("output").arg("-json").current_dir(&terraform_dir);
    
    // Add credentials as environment variables
    for (key, value) in &env_vars {
        cmd.env(key, value);
    }
    
    let output = cmd.output()?;

    let url = if output.status.success() {
        if let Ok(outputs) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            outputs
                .get("instance_ip")
                .or_else(|| outputs.get("public_ip"))
                .or_else(|| outputs.get("public_dns"))
                .or_else(|| outputs.get("website_url"))
                .and_then(|v| v.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string()
        } else {
            "unknown".to_string()
        }
    } else {
        "unknown".to_string()
    };

    let public_ip = if output.status.success() {
        if let Ok(outputs) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            outputs
                .get("instance_ip")
                .or_else(|| outputs.get("public_ip"))
                .and_then(|v| v.get("value"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    } else {
        None
    };

    logs.push(format!("🌐 Deployment URL: {}", url));

    Ok(DeploymentResult {
        url: format!("http://{}", url),
        infrastructure_type: format!("{:?}", decision.deployment_type),
        public_ip,
        logs,
    })
}

fn generate_terraform_files(
    config: &TerraformConfig,
    terraform_dir: &Path,
    repo_url: &str,
) -> Result<()> {
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    // Generate main.tf
    let mut main_tf = String::new();

    // Provider configuration
    match config.provider.as_str() {
        "aws" => {
            main_tf.push_str("terraform {\n");
            main_tf.push_str("  required_providers {\n");
            main_tf.push_str("    aws = {\n");
            main_tf.push_str("      source  = \"hashicorp/aws\"\n");
            main_tf.push_str("      version = \"~> 5.0\"\n");
            main_tf.push_str("    }\n");
            main_tf.push_str("  }\n");
            main_tf.push_str("}\n\n");
            main_tf.push_str("provider \"aws\" {\n");
            main_tf.push_str("  region = var.region\n");
            main_tf.push_str("}\n\n");
        }
        "gcp" => {
            main_tf.push_str("terraform {\n");
            main_tf.push_str("  required_providers {\n");
            main_tf.push_str("    google = {\n");
            main_tf.push_str("      source  = \"hashicorp/google\"\n");
            main_tf.push_str("      version = \"~> 4.0\"\n");
            main_tf.push_str("    }\n");
            main_tf.push_str("  }\n");
            main_tf.push_str("}\n\n");
            main_tf.push_str("provider \"google\" {\n");
            main_tf.push_str("  project = var.project\n");
            main_tf.push_str("  region  = var.region\n");
            main_tf.push_str("}\n\n");
        }
        _ => {}
    }

    // Resources
    for resource in &config.resources {
        main_tf.push_str(&format!(
            "resource \"{}\" \"{}\" {{\n",
            resource.resource_type, resource.name
        ));
        for (key, value) in &resource.config {
            // Add sed commands to startup scripts to replace localhost with 0.0.0.0
            let processed_value = if key == "metadata_startup_script" || key == "user_data" {
                replace_git_clone_with_download(value, repo_url)
            } else {
                value.clone()
            };
            
            // Add timestamp to firewall rule names to avoid conflicts
            if key == "name" && resource.resource_type.contains("firewall") {
                if let serde_json::Value::String(name) = &processed_value {
                    let unique_name = format!("{}-{}", name, timestamp);
                    main_tf.push_str(&format!("  name = \"{}\"\n", unique_name));
                    continue;
                }
            }
            main_tf.push_str(&format!("  {}\n", json_to_hcl(key, &processed_value, 1)));
        }
        main_tf.push_str("}\n\n");
    }

    fs::write(terraform_dir.join("main.tf"), main_tf)?;

    // Generate variables.tf
    let mut variables_tf = String::new();
    variables_tf.push_str(&format!("variable \"repository_url\" {{\n  description = \"Repository URL\"\n  type = string\n  default = \"{}\"\n}}\n\n", repo_url));
    variables_tf.push_str("variable \"region\" {\n  description = \"Cloud region\"\n  type = string\n  default = \"us-east-1\"\n}\n\n");

    let mut added_vars = std::collections::HashSet::new();
    added_vars.insert("repository_url".to_string());
    added_vars.insert("region".to_string());
    
    for (var_name, var_config) in &config.variables {
        // Skip if we already added this variable
        if added_vars.contains(var_name) {
            continue;
        }
        
        variables_tf.push_str(&format!("variable \"{}\" {{\n", var_name));
        
        if let Some(var_type) = var_config.get("type") {
            if let Some(type_str) = var_type.as_str() {
                variables_tf.push_str(&format!("  type = {}\n", type_str));
            }
        }
        
        if let Some(description) = var_config.get("description") {
            if let Some(desc_str) = description.as_str() {
                variables_tf.push_str(&format!("  description = \"{}\"\n", desc_str));
            }
        }
        
        if let Some(default) = var_config.get("default") {
            if let Some(default_str) = default.as_str() {
                variables_tf.push_str(&format!("  default = \"{}\"\n", default_str));
            }
        }
        
        variables_tf.push_str("}\n\n");
        added_vars.insert(var_name.clone());
    }

    fs::write(terraform_dir.join("variables.tf"), variables_tf)?;

    // Generate outputs.tf
    let mut outputs_tf = String::new();
    for (output_name, output_config) in &config.outputs {
        outputs_tf.push_str(&format!("output \"{}\" {{\n", output_name));
        
        if let Some(value) = output_config.get("value") {
            if let Some(value_str) = value.as_str() {
                // Don't quote Terraform interpolation expressions
                outputs_tf.push_str(&format!("  value = {}\n", value_str));
            }
        }
        
        if let Some(description) = output_config.get("description") {
            if let Some(desc_str) = description.as_str() {
                outputs_tf.push_str(&format!("  description = \"{}\"\n", desc_str));
            }
        }
        
        outputs_tf.push_str("}\n\n");
    }

    fs::write(terraform_dir.join("outputs.tf"), outputs_tf)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nlp::{
        ApplicationType, CloudProvider, DatabaseType, DeploymentRequirements, ScalingRequirements,
    };
    use crate::repository::{PackageManager, RepositoryAnalysis};
    

    fn create_test_requirements() -> DeploymentRequirements {
        DeploymentRequirements {
            cloud_provider: CloudProvider::AWS,
            application_type: Some(ApplicationType::Flask),
            scaling_requirements: ScalingRequirements::Single,
            database_requirements: vec![DatabaseType::PostgreSQL],
            environment_variables: HashMap::new(),
            port_requirements: vec![80, 443],
            ssl_required: true,
            custom_domain: Some("example.com".to_string()),
        }
    }

    fn create_test_analysis() -> RepositoryAnalysis {
        RepositoryAnalysis {
            app_type: ApplicationType::Flask,
            dependencies: vec!["Flask".to_string(), "psycopg2".to_string()],
            build_commands: vec!["pip install -r requirements.txt".to_string()],
            start_commands: vec!["python app.py".to_string()],
            environment_variables: vec!["DATABASE_URL".to_string()],
            exposed_ports: vec![5000],
            static_files_dir: Some("static".to_string()),
            database_migrations: true,
            requires_build_step: true,
            docker_config: None,
            package_manager: PackageManager::Pip,
        }
    }

    #[tokio::test]
    async fn test_decide_infrastructure_single_vm() {
        let requirements = create_test_requirements();
        let analysis = create_test_analysis();

        let decision = decide_infrastructure(&requirements, &analysis, "", "https://github.com/Arvo-AI/hello_world/tree/main").await.unwrap();

        assert!(matches!(decision.deployment_type, DeploymentType::SingleVM));
        assert_eq!(decision.instance_type, "t3.micro");
        assert!(decision.estimated_cost > 0.0);
        assert!(decision.justification.contains("Flask"));
    }

    #[tokio::test]
    async fn test_decide_infrastructure_serverless() {
        let mut requirements = create_test_requirements();
        requirements.scaling_requirements = ScalingRequirements::Serverless;
        let analysis = create_test_analysis();

        let decision = decide_infrastructure(&requirements, &analysis, "", "https://github.com/Arvo-AI/hello_world/tree/main").await.unwrap();

        assert!(matches!(
            decision.deployment_type,
            DeploymentType::Serverless
        ));
        assert_eq!(decision.instance_type, "lambda");
    }

    #[tokio::test]
    async fn test_decide_infrastructure_static_site() {
        let requirements = create_test_requirements();
        let mut analysis = create_test_analysis();
        analysis.app_type = ApplicationType::React;
        analysis.requires_build_step = false;

        let decision = decide_infrastructure(&requirements, &analysis, "", "https://github.com/Arvo-AI/hello_world/tree/main").await.unwrap();

        assert!(matches!(
            decision.deployment_type,
            DeploymentType::StaticSite
        ));
        assert_eq!(decision.instance_type, "static-hosting");
    }

    #[tokio::test]
    async fn test_generate_terraform_vm_config() {
        let requirements = create_test_requirements();
        let analysis = create_test_analysis();

        let decision = decide_infrastructure(&requirements, &analysis, "", "https://github.com/Arvo-AI/hello_world/tree/main").await.unwrap();

        // Check that Terraform config is generated
        assert_eq!(decision.terraform_config.provider, "aws");
        assert!(!decision.terraform_config.resources.is_empty());

        // Should have security group and instance
        let resource_types: Vec<&str> = decision
            .terraform_config
            .resources
            .iter()
            .map(|r| r.resource_type.as_str())
            .collect();
        assert!(resource_types.contains(&"aws_security_group"));
        assert!(resource_types.contains(&"aws_instance"));
    }

    #[tokio::test]
    async fn test_generate_terraform_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let terraform_dir = temp_dir.path().join("terraform");
        fs::create_dir_all(&terraform_dir).unwrap();

        let requirements = create_test_requirements();
        let analysis = create_test_analysis();
        let decision = decide_infrastructure(&requirements, &analysis, "", "https://github.com/Arvo-AI/hello_world/tree/main").await.unwrap();

        let result = generate_terraform_files(
            &decision.terraform_config,
            &terraform_dir,
            "https://github.com/test/repo",
        );

        assert!(result.is_ok());

        // Check that files were created
        assert!(terraform_dir.join("main.tf").exists());
        assert!(terraform_dir.join("variables.tf").exists());
        assert!(terraform_dir.join("outputs.tf").exists());

        // Check main.tf content
        let main_tf_content = fs::read_to_string(terraform_dir.join("main.tf")).unwrap();
        assert!(main_tf_content.contains("provider \"aws\""));
        assert!(main_tf_content.contains("aws_security_group"));
        assert!(main_tf_content.contains("aws_instance"));
    }

    #[tokio::test]
    async fn test_provision_infrastructure_dry_run() {
        let temp_dir = tempfile::tempdir().unwrap();
        let requirements = create_test_requirements();
        let analysis = create_test_analysis();
        let decision = decide_infrastructure(&requirements, &analysis, "", "https://github.com/Arvo-AI/hello_world/tree/main").await.unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();

        let result = rt.block_on(provision_infrastructure(
            &decision,
            "https://github.com/test/repo",
            temp_dir.path(),
            true, // dry_run
            &requirements.cloud_provider, // Add the missing fifth argument
        ));

        assert!(result.is_ok());
        let deployment_result = result.unwrap();
        assert_eq!(deployment_result.url, "dry-run");
        assert!(deployment_result
            .logs
            .iter()
            .any(|log| log.contains("Dry run")));
    }

    #[tokio::test]
    async fn test_provision_infrastructure_no_terraform() {
        let temp_dir = tempfile::tempdir().unwrap();
        let requirements = create_test_requirements();
        let analysis = create_test_analysis();
        let decision = decide_infrastructure(&requirements, &analysis, "", "https://github.com/Arvo-AI/hello_world/tree/main").await.unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(provision_infrastructure(
            &decision,
            "https://github.com/test/repo",
            temp_dir.path(),
            false,
            &requirements.cloud_provider // not dry_run
        ));

        // Should fail because Terraform is not installed
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Terraform is not installed"));
    }

    #[test]
    fn test_cost_estimation() {
        let single_vm_cost = estimate_cost(&DeploymentType::SingleVM, &CloudProvider::AWS);
        let serverless_cost = estimate_cost(&DeploymentType::Serverless, &CloudProvider::AWS);
        let static_cost = estimate_cost(&DeploymentType::StaticSite, &CloudProvider::AWS);

        assert!(single_vm_cost > 0.0);
        assert!(serverless_cost > 0.0);
        assert!(static_cost > 0.0);
        assert!(single_vm_cost > static_cost); // VM should cost more than static hosting
    }

    #[test]
    fn test_cloud_provider_instance_types() {
        // Test AWS
        let aws_vm = determine_instance_type(&DeploymentType::SingleVM, &CloudProvider::AWS);
        assert_eq!(aws_vm, "t3.micro");

        // Test GCP
        let gcp_vm = determine_instance_type(&DeploymentType::SingleVM, &CloudProvider::GCP);
        assert_eq!(gcp_vm, "e2-micro");

        // Test serverless
        let serverless = determine_instance_type(&DeploymentType::Serverless, &CloudProvider::AWS);
        assert_eq!(serverless, "lambda");
    }
}

fn escape_hcl_string(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace('"', "\\\"")
     .replace('\n', "\\n")
     .replace('\r', "\\r")
     .replace('\t', "\\t")
}

fn json_to_hcl(key: &str, value: &serde_json::Value, indent_level: usize) -> String {
    let indent = "  ".repeat(indent_level);
    
    match value {
        serde_json::Value::String(s) => {
            // Don't quote if it's a Terraform variable reference
            if s.starts_with("var.") || s.starts_with("${") {
                format!("{} = {}", key, s)
            } else {
                // Properly escape the string for HCL
                let escaped = escape_hcl_string(s);
                format!("{} = \"{}\"", key, escaped)
            }
        }
        serde_json::Value::Number(n) => {
            format!("{} = {}", key, n)
        }
        serde_json::Value::Bool(b) => {
            format!("{} = {}", key, b)
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                format!("{} = []", key)
            } else if arr.iter().all(|v| v.is_string()) {
                // Simple string array
                let items: Vec<String> = arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| format!("\"{}\"", escape_hcl_string(s)))
                    .collect();
                format!("{} = [{}]", key, items.join(", "))
            } else {
                // Complex array - format as multiple blocks
                let mut result = String::new();
                for item in arr {
                    if let serde_json::Value::Object(obj) = item {
                        result.push_str(&format!("{} {{\n", key));
                        for (subkey, subvalue) in obj {
                            result.push_str(&format!("{}  {}\n", indent, json_to_hcl(subkey, subvalue, indent_level + 1)));
                        }
                        result.push_str(&format!("{}}}\n", indent));
                    }
                }
                result.trim_end().to_string()
            }
        }
        serde_json::Value::Object(obj) => {
            // Handle as a block
            let mut result = format!("{} {{\n", key);
            for (subkey, subvalue) in obj {
                result.push_str(&format!("{}  {}\n", indent, json_to_hcl(subkey, subvalue, indent_level + 1)));
            }
            result.push_str(&format!("{}}}", indent));
            result
        }
        serde_json::Value::Null => {
            format!("{} = null", key)
        }
    }
}

/// Add sed commands to startup scripts to replace localhost with 0.0.0.0 after git clone
fn replace_git_clone_with_download(script: &serde_json::Value, _download_url: &str) -> serde_json::Value {
    if let serde_json::Value::String(script_str) = script {
        let mut modified_script = script_str.clone();
        
        // If script contains git clone, add localhost replacement commands after it
        if script_str.contains("git clone") {
            // Add comprehensive sed commands to replace localhost references in all relevant files
            let sed_commands = " && find . -name '*.py' -exec sed -i 's/127\\.0\\.0\\.1/0.0.0.0/g' {} \\; && find . -name '*.py' -exec sed -i 's/localhost/0.0.0.0/g' {} \\; && find . -name '*.html' -exec sed -i 's/http:\\/\\/localhost:5000//g' {} \\; && find . -name '*.js' -exec sed -i 's/http:\\/\\/localhost:5000//g' {} \\; && find . -name '*.ts' -exec sed -i 's/http:\\/\\/localhost:5000//g' {} \\;";
            
            // Insert sed commands after any git clone and cd commands
            if let Some(pos) = script_str.rfind(" && cd ") {
                // Find the end of the cd command (next && or end of string)
                let after_cd = &script_str[pos + 6..]; // Skip " && cd "
                if let Some(next_and) = after_cd.find(" && ") {
                    let insert_pos = pos + 6 + next_and;
                    modified_script.insert_str(insert_pos, sed_commands);
                } else {
                    // cd is at the end, append sed commands
                    modified_script.push_str(sed_commands);
                }
            } else if script_str.contains("git clone") {
                // No cd command, just append sed commands at the end
                modified_script.push_str(sed_commands);
            }
        }
        
        serde_json::Value::String(modified_script)
    } else {
        script.clone()
    }
}
