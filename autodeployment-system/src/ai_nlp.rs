use anyhow::{Result, anyhow};
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use log::info;

use crate::nlp::{DeploymentRequirements, ApplicationType, CloudProvider, ScalingRequirements, DatabaseType};
use crate::infrastructure::TerraformConfig;

const GEMINI_API_KEY: &str = "AIzaSyAiA5zlxjAJiMkoDCSJFPnTklvLCE786pk";
const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash-exp:generateContent";

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    generation_config: GeminiGenerationConfig,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    temperature: f32,
    #[serde(rename = "topK")]
    top_k: i32,
    #[serde(rename = "topP")]
    top_p: f32,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: i32,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiResponseContent,
}

#[derive(Deserialize)]
struct GeminiResponseContent {
    parts: Vec<GeminiResponsePart>,
}

#[derive(Deserialize)]
struct GeminiResponsePart {
    text: String,
}

#[derive(Deserialize)]
struct ParsedRequirements {
    application_type: String,
    scaling_requirements: String,
    database_requirements: Vec<String>,
    cloud_provider: String,
    port_requirements: Vec<u16>,
    ssl_required: bool,
    custom_domain: Option<String>,
    environment_variables: HashMap<String, String>,
}

pub async fn parse_deployment_requirements(description: &str) -> Result<DeploymentRequirements> {
    info!("🤖 Using Gemini 2.5 Flash to parse deployment requirements...");
    
    let prompt = format!(
        r#"Analyze this deployment description and extract structured deployment requirements in JSON format:

Description: "{}"

Extract the following information and respond with ONLY a JSON object (no markdown, no explanation):

{{
  "application_type": "Flask|Django|FastAPI|NodeJS|React|NextJS|Express|Go|Rust|Ruby|PHP|Static|Unknown",
  "scaling_requirements": "Single|AutoScaling|LoadBalanced|Serverless",
  "database_requirements": ["PostgreSQL", "MySQL", "MongoDB", "Redis", "None"],
  "cloud_provider": "AWS|GCP|Azure|DigitalOcean",
  "port_requirements": [80, 443],
  "ssl_required": true,
  "custom_domain": "example.com or null",
  "environment_variables": {{"DATABASE_URL": "postgresql://...", "API_KEY": "secret"}}
}}

Rules:
- If not specified, use sensible defaults
- application_type: infer from keywords (Flask, Django, Node, React, etc.)
- scaling_requirements: "Single" unless "auto-scaling", "load balanced", or "serverless" mentioned
- database_requirements: extract database types mentioned, use ["None"] if none
- cloud_provider: AWS unless GCP/Google/Azure/DigitalOcean specified
- port_requirements: [80, 443] for web apps, [80] for simple apps
- ssl_required: true for production deployments
- custom_domain: extract domain if mentioned, otherwise null
- environment_variables: extract any env vars or configs mentioned"#,
        description
    );

    let response_text = call_gemini_api(&prompt).await?;
    
    // Clean the response to extract JSON
    let json_text = extract_json_from_response(&response_text)?;
    
    // Parse the JSON response
    let parsed: ParsedRequirements = serde_json::from_str(&json_text)
        .map_err(|e| anyhow!("Failed to parse Gemini response as JSON: {}. Response: {}", e, json_text))?;

    // Convert to our internal types
    let application_type = match parsed.application_type.as_str() {
        "Flask" => Some(ApplicationType::Flask),
        "Django" => Some(ApplicationType::Django),
        "FastAPI" => Some(ApplicationType::FastAPI),
        "NodeJS" => Some(ApplicationType::NodeJS),
        "React" => Some(ApplicationType::React),
        "NextJS" => Some(ApplicationType::NextJS),
        "Express" => Some(ApplicationType::Express),
        "Go" => Some(ApplicationType::Unknown),
        "Rust" => Some(ApplicationType::Unknown),
        "Ruby" => Some(ApplicationType::Unknown),
        "PHP" => Some(ApplicationType::Unknown),
        "Static" => Some(ApplicationType::React),
        _ => None,
    };

    let scaling_requirements = match parsed.scaling_requirements.as_str() {
        "AutoScaling" => ScalingRequirements::AutoScale,
        "LoadBalanced" => ScalingRequirements::LoadBalanced,
        "Serverless" => ScalingRequirements::Serverless,
        _ => ScalingRequirements::Single,
    };

    let cloud_provider = match parsed.cloud_provider.as_str() {
        "GCP" => CloudProvider::GCP,
        "Azure" => CloudProvider::Azure,
        "DigitalOcean" => CloudProvider::DigitalOcean,
        _ => CloudProvider::AWS,
    };

    let database_requirements = parsed.database_requirements
        .iter()
        .filter_map(|db| match db.as_str() {
            "PostgreSQL" => Some(DatabaseType::PostgreSQL),
            "MySQL" => Some(DatabaseType::MySQL),
            "MongoDB" => Some(DatabaseType::MongoDB),
            "Redis" => Some(DatabaseType::Redis),
            _ => None,
        })
        .collect();

    info!("✅ Successfully parsed requirements using AI");
    info!("   Application Type: {:?}", application_type);
    info!("   Scaling: {:?}", scaling_requirements);
    info!("   Cloud Provider: {:?}", cloud_provider);
    info!("   Databases: {:?}", database_requirements);

    Ok(DeploymentRequirements {
        cloud_provider,
        application_type,
        scaling_requirements,
        database_requirements,
        environment_variables: parsed.environment_variables,
        port_requirements: parsed.port_requirements,
        ssl_required: parsed.ssl_required,
        custom_domain: parsed.custom_domain,
    })
}

pub async fn generate_terraform_with_ai(
    description: &str,
    cloud_provider: &CloudProvider,
    deployment_type: &str,
) -> Result<TerraformConfig> {
    info!("🤖 Using Gemini 2.5 Flash to generate Terraform configuration...");
    
    let prompt = format!(
        r#"Generate a Terraform configuration for this deployment:

Description: "{}"
Cloud Provider: {:?}
Deployment Type: {}

Generate Terraform configuration as JSON with this exact structure:

{{
  "provider": "aws",
  "resources": [
    {{
      "resource_type": "aws_instance",
      "name": "app_instance",
      "config": {{
        "instance_type": "t3.micro",
        "ami": "ami-0c02fb55956c7d316",
        "vpc_security_group_ids": ["aws_security_group.app_sg.id"],
        "user_data": "base64:setup_script_base64_encoded"
      }}
    }},
    {{
      "resource_type": "aws_security_group",
      "name": "app_sg",
      "config": {{
        "name": "app_sg",
        "description": "Allow inbound traffic",
        "ingress": [
          {{
            "from_port": 22,
            "to_port": 22,
            "protocol": "tcp",
            "cidr_blocks": ["0.0.0.0/0"],
            "description": "SSH",
            "ipv6_cidr_blocks": [],
            "prefix_list_ids": [],
            "security_groups": [],
            "self": false
          }}
        ],
        "egress": [
          {{
            "from_port": 0,
            "to_port": 0,
            "protocol": "-1",
            "cidr_blocks": ["0.0.0.0/0"],
            "description": "All outbound",
            "ipv6_cidr_blocks": [],
            "prefix_list_ids": [],
            "security_groups": [],
            "self": false
          }}
        ]
      }}
    }}
  ],
  "variables": {{
    "region": "AWS region",
    "key_name": "AWS key pair"
  }},
  "outputs": {{
    "public_ip": {{
      "value": "aws_instance.app_instance.public_ip",
      "description": "Instance public IP"
    }},
    "public_dns": {{
      "value": "aws_instance.app_instance.public_dns", 
      "description": "Instance public DNS"
    }}
  }}
}}

Requirements:
- For AWS: Use EC2 instances, security groups, proper AMIs
- For GCP: Use compute instances, firewall rules, proper images  
- For Azure: Use virtual machines, network security groups, proper images
- Include proper networking, security, and application setup
- Use appropriate instance types for the workload
- For user_data: use simple bootstrap commands like "apt update && apt install python3 -y"
- Avoid complex multi-line scripts or embedded quotes
- Set up proper ports based on application type

IMPORTANT: 
- Keep strings simple, avoid nested quotes, use minimal user_data scripts
- Use modern Terraform syntax: "aws_instance.app_instance.public_ip" not "${{aws_instance.app_instance.public_ip}}"
- Output values should be unquoted resource references
- Variable references should be simple: "var.region" not "${{var.region}}"

Respond with ONLY the JSON object, no markdown or explanation."#,
        description, cloud_provider, deployment_type
    );

    let response_text = call_gemini_api(&prompt).await?;
    let json_text = extract_json_from_response(&response_text)?;
    
    let config: TerraformConfig = serde_json::from_str(&json_text)
        .map_err(|e| anyhow!("Failed to parse AI-generated Terraform config: {}. Response: {}", e, json_text))?;

    info!("✅ Successfully generated Terraform config using AI");
    info!("   Provider: {}", config.provider);
    info!("   Resources: {}", config.resources.len());

    Ok(config)
}

async fn call_gemini_api(prompt: &str) -> Result<String> {
    let client = reqwest::Client::new();
    
    let request = GeminiRequest {
        contents: vec![GeminiContent {
            parts: vec![GeminiPart {
                text: prompt.to_string(),
            }],
        }],
        generation_config: GeminiGenerationConfig {
            temperature: 0.1,
            top_k: 32,
            top_p: 1.0,
            max_output_tokens: 2048,
        },
    };

    let url = format!("{}?key={}", GEMINI_API_URL, GEMINI_API_KEY);
    
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to call Gemini API: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow!("Gemini API error {}: {}", status, error_text));
    }

    let gemini_response: GeminiResponse = response
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse Gemini response: {}", e))?;

    if gemini_response.candidates.is_empty() {
        return Err(anyhow!("No candidates in Gemini response"));
    }

    if gemini_response.candidates[0].content.parts.is_empty() {
        return Err(anyhow!("No parts in Gemini response"));
    }

    Ok(gemini_response.candidates[0].content.parts[0].text.clone())
}

fn extract_json_from_response(response: &str) -> Result<String> {
    // Remove markdown code blocks if present
    let cleaned = response
        .trim()
        .strip_prefix("```json")
        .unwrap_or(response)
        .strip_suffix("```")
        .unwrap_or(response)
        .trim();

    // Find JSON object boundaries
    if let Some(start) = cleaned.find('{') {
        if let Some(end) = cleaned.rfind('}') {
            if end > start {
                return Ok(cleaned[start..=end].to_string());
            }
        }
    }

    // If no clear JSON boundaries, return the cleaned response
    Ok(cleaned.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_from_response() {
        let response_with_markdown = "```json\n{\"test\": \"value\"}\n```";
        let result = extract_json_from_response(response_with_markdown).unwrap();
        assert_eq!(result, "{\"test\": \"value\"}");

        let response_plain = "{\"test\": \"value\"}";
        let result = extract_json_from_response(response_plain).unwrap();
        assert_eq!(result, "{\"test\": \"value\"}");

        let response_with_text = "Here is the JSON: {\"test\": \"value\"} that you requested.";
        let result = extract_json_from_response(response_with_text).unwrap();
        assert_eq!(result, "{\"test\": \"value\"}");
    }

    #[tokio::test]
    async fn test_parse_deployment_requirements_structure() {
        // Test basic structure without calling API
        let sample_json = r#"{
            "application_type": "Flask",
            "scaling_requirements": "Single",
            "database_requirements": ["PostgreSQL"],
            "cloud_provider": "AWS",
            "port_requirements": [80, 443],
            "ssl_required": true,
            "custom_domain": null,
            "environment_variables": {}
        }"#;

        let parsed: ParsedRequirements = serde_json::from_str(sample_json).unwrap();
        assert_eq!(parsed.application_type, "Flask");
        assert_eq!(parsed.scaling_requirements, "Single");
        assert_eq!(parsed.database_requirements, vec!["PostgreSQL"]);
    }
}