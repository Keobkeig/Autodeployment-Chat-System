use anyhow::{Result, anyhow};
use git2::Repository;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use walkdir::WalkDir;
use regex::Regex;
use crate::nlp::ApplicationType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryAnalysis {
    pub app_type: ApplicationType,
    pub dependencies: Vec<String>,
    pub build_commands: Vec<String>,
    pub start_commands: Vec<String>,
    pub environment_variables: Vec<String>,
    pub exposed_ports: Vec<u16>,
    pub static_files_dir: Option<String>,
    pub database_migrations: bool,
    pub requires_build_step: bool,
    pub docker_config: Option<DockerConfig>,
    pub package_manager: PackageManager,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerConfig {
    pub dockerfile_path: String,
    pub exposed_ports: Vec<u16>,
    pub volumes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PackageManager {
    Pip,
    Npm,
    Yarn,
    Maven,
    Gradle,
    Bundler,
    Composer,
    Unknown,
}

pub async fn clone_repository(repo_url: &str) -> Result<TempDir> {
    let temp_dir = tempfile::tempdir()?;
    let repo_path = temp_dir.path();
    
    log::info!("Cloning repository {} to {:?}", repo_url, repo_path);
    
    // Use git2 for actual cloning
    Repository::clone(repo_url, repo_path)
        .map_err(|e| anyhow!("Failed to clone repository: {}", e))?;
    
    log::info!("Successfully cloned repository to {:?}", repo_path);
    Ok(temp_dir)
}

pub fn analyze_repository(repo_path: &Path) -> Result<RepositoryAnalysis> {
    log::info!("Analyzing repository at {:?}", repo_path);
    
    let mut analysis = RepositoryAnalysis {
        app_type: ApplicationType::Unknown,
        dependencies: Vec::new(),
        build_commands: Vec::new(),
        start_commands: Vec::new(),
        environment_variables: Vec::new(),
        exposed_ports: Vec::new(),
        static_files_dir: None,
        database_migrations: false,
        requires_build_step: false,
        docker_config: None,
        package_manager: PackageManager::Unknown,
    };
    
    analysis.app_type = detect_application_type(repo_path)?;
    analysis.package_manager = detect_package_manager(repo_path)?;
    analysis.dependencies = extract_dependencies(repo_path, &analysis.package_manager)?;
    analysis.docker_config = analyze_dockerfile(repo_path)?;
    analysis.exposed_ports = detect_exposed_ports(repo_path)?;
    analysis.static_files_dir = detect_static_files(repo_path);
    analysis.database_migrations = detect_database_migrations(repo_path);
    analysis.environment_variables = extract_environment_variables(repo_path)?;
    
    let (build_commands, start_commands, requires_build) = generate_commands(&analysis)?;
    analysis.build_commands = build_commands;
    analysis.start_commands = start_commands;
    analysis.requires_build_step = requires_build;
    
    Ok(analysis)
}

fn detect_application_type(repo_path: &Path) -> Result<ApplicationType> {
    let files = collect_files(repo_path)?;
    
    if files.contains(&"requirements.txt".to_string()) || 
       files.contains(&"Pipfile".to_string()) ||
       files.iter().any(|f| f.ends_with(".py")) {
        
        let has_flask = files.iter().any(|f| f.contains("flask")) || 
                        files.iter().any(|f| file_contains_keyword(repo_path, f, "Flask").unwrap_or(false));
        let has_django = files.iter().any(|f| f.contains("django")) || 
                         files.iter().any(|f| file_contains_keyword(repo_path, f, "Django").unwrap_or(false));
        let has_fastapi = files.iter().any(|f| f.contains("fastapi")) || 
                          files.iter().any(|f| file_contains_keyword(repo_path, f, "FastAPI").unwrap_or(false));
        
        if has_flask {
            return Ok(ApplicationType::Flask);
        } else if has_django {
            return Ok(ApplicationType::Django);
        } else if has_fastapi {
            return Ok(ApplicationType::FastAPI);
        }
    }
    
    if files.contains(&"package.json".to_string()) {
        let package_json_path = repo_path.join("package.json");
        if let Ok(content) = fs::read_to_string(&package_json_path) {
            if content.contains("\"react\"") {
                return Ok(ApplicationType::React);
            } else if content.contains("\"next\"") {
                return Ok(ApplicationType::NextJS);
            } else if content.contains("\"express\"") {
                return Ok(ApplicationType::Express);
            } else {
                return Ok(ApplicationType::NodeJS);
            }
        }
    }
    
    if files.contains(&"Gemfile".to_string()) {
        return Ok(ApplicationType::Rails);
    }
    
    if files.contains(&"pom.xml".to_string()) || files.contains(&"build.gradle".to_string()) {
        return Ok(ApplicationType::Spring);
    }
    
    Ok(ApplicationType::Unknown)
}

fn detect_package_manager(repo_path: &Path) -> Result<PackageManager> {
    let files = collect_files(repo_path)?;
    
    if files.contains(&"requirements.txt".to_string()) || files.contains(&"Pipfile".to_string()) {
        Ok(PackageManager::Pip)
    } else if files.contains(&"yarn.lock".to_string()) {
        Ok(PackageManager::Yarn)
    } else if files.contains(&"package.json".to_string()) {
        Ok(PackageManager::Npm)
    } else if files.contains(&"pom.xml".to_string()) {
        Ok(PackageManager::Maven)
    } else if files.contains(&"build.gradle".to_string()) {
        Ok(PackageManager::Gradle)
    } else if files.contains(&"Gemfile".to_string()) {
        Ok(PackageManager::Bundler)
    } else if files.contains(&"composer.json".to_string()) {
        Ok(PackageManager::Composer)
    } else {
        Ok(PackageManager::Unknown)
    }
}

fn extract_dependencies(repo_path: &Path, package_manager: &PackageManager) -> Result<Vec<String>> {
    let mut dependencies = Vec::new();
    
    match package_manager {
        PackageManager::Pip => {
            if let Ok(content) = fs::read_to_string(repo_path.join("requirements.txt")) {
                dependencies = content.lines()
                    .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
                    .map(|line| {
                        // Split on various operators: check longer operators first
                        let operators = [">=", "<=", "~=", "==", ">", "<"];
                        for op in &operators {
                            if line.contains(op) {
                                if let Some(pkg_name) = line.split(op).next() {
                                    return pkg_name.trim().to_string();
                                }
                            }
                        }
                        line.trim().to_string()
                    })
                    .collect();
            }
        },
        PackageManager::Npm | PackageManager::Yarn => {
            if let Ok(content) = fs::read_to_string(repo_path.join("package.json")) {
                if let Ok(package_json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(deps) = package_json.get("dependencies").and_then(|d| d.as_object()) {
                        dependencies.extend(deps.keys().cloned());
                    }
                    if let Some(dev_deps) = package_json.get("devDependencies").and_then(|d| d.as_object()) {
                        dependencies.extend(dev_deps.keys().cloned());
                    }
                }
            }
        },
        _ => {}
    }
    
    Ok(dependencies)
}

fn analyze_dockerfile(repo_path: &Path) -> Result<Option<DockerConfig>> {
    let dockerfile_path = repo_path.join("Dockerfile");
    if !dockerfile_path.exists() {
        return Ok(None);
    }
    
    let content = fs::read_to_string(&dockerfile_path)?;
    let mut exposed_ports = Vec::new();
    let mut volumes = Vec::new();
    
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("EXPOSE") {
            if let Some(port_str) = line.split_whitespace().nth(1) {
                if let Ok(port) = port_str.parse::<u16>() {
                    exposed_ports.push(port);
                }
            }
        } else if line.starts_with("VOLUME") {
            if let Some(volume) = line.split_whitespace().nth(1) {
                volumes.push(volume.trim_matches('"').to_string());
            }
        }
    }
    
    Ok(Some(DockerConfig {
        dockerfile_path: "Dockerfile".to_string(),
        exposed_ports,
        volumes,
    }))
}

fn detect_exposed_ports(repo_path: &Path) -> Result<Vec<u16>> {
    let mut ports = Vec::new();
    
    for entry in WalkDir::new(repo_path).max_depth(3) {
        let entry = entry?;
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                if ext == "py" || ext == "js" || ext == "ts" {
                    if let Ok(content) = fs::read_to_string(entry.path()) {
                        let port_regex = Regex::new(r"(?:port|PORT)[:=\s]*(\d+)").unwrap();
                        for caps in port_regex.captures_iter(&content) {
                            if let Some(port_match) = caps.get(1) {
                                if let Ok(port) = port_match.as_str().parse::<u16>() {
                                    if port > 1000 && port < 65535 {
                                        ports.push(port);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    ports.sort();
    ports.dedup();
    
    if ports.is_empty() {
        ports.push(5000); // Default Flask port
    }
    
    Ok(ports)
}

fn detect_static_files(repo_path: &Path) -> Option<String> {
    let static_dirs = ["static", "public", "assets", "dist", "build"];
    
    for dir in &static_dirs {
        let static_path = repo_path.join(dir);
        if static_path.exists() && static_path.is_dir() {
            return Some(dir.to_string());
        }
    }
    
    None
}

fn detect_database_migrations(repo_path: &Path) -> bool {
    let migration_indicators = ["migrations", "migrate", "alembic", "db/migrate"];
    
    for indicator in &migration_indicators {
        let migration_path = repo_path.join(indicator);
        if migration_path.exists() {
            return true;
        }
    }
    
    false
}

fn extract_environment_variables(repo_path: &Path) -> Result<Vec<String>> {
    let mut env_vars = Vec::new();
    
    let env_files = [".env", ".env.example", ".env.template"];
    for env_file in &env_files {
        let env_path = repo_path.join(env_file);
        if env_path.exists() {
            if let Ok(content) = fs::read_to_string(&env_path) {
                for line in content.lines() {
                    let line = line.trim();
                    if !line.is_empty() && !line.starts_with('#') && line.contains('=') {
                        if let Some(var_name) = line.split('=').next() {
                            env_vars.push(var_name.to_string());
                        }
                    }
                }
            }
        }
    }
    
    Ok(env_vars)
}

fn generate_commands(analysis: &RepositoryAnalysis) -> Result<(Vec<String>, Vec<String>, bool)> {
    let mut build_commands = Vec::new();
    let mut start_commands = Vec::new();
    let mut requires_build = false;
    
    match analysis.app_type {
        ApplicationType::Flask => {
            build_commands.push("pip install -r requirements.txt".to_string());
            start_commands.push("python app.py".to_string());
            requires_build = true;
        },
        ApplicationType::Django => {
            build_commands.push("pip install -r requirements.txt".to_string());
            if analysis.database_migrations {
                build_commands.push("python manage.py migrate".to_string());
            }
            start_commands.push("python manage.py runserver 0.0.0.0:8000".to_string());
            requires_build = true;
        },
        ApplicationType::NodeJS | ApplicationType::Express => {
            match analysis.package_manager {
                PackageManager::Yarn => {
                    build_commands.push("yarn install".to_string());
                    start_commands.push("yarn start".to_string());
                },
                _ => {
                    build_commands.push("npm install".to_string());
                    start_commands.push("npm start".to_string());
                }
            }
            requires_build = true;
        },
        ApplicationType::React | ApplicationType::NextJS => {
            match analysis.package_manager {
                PackageManager::Yarn => {
                    build_commands.push("yarn install".to_string());
                    build_commands.push("yarn build".to_string());
                    start_commands.push("yarn start".to_string());
                },
                _ => {
                    build_commands.push("npm install".to_string());
                    build_commands.push("npm run build".to_string());
                    start_commands.push("npm start".to_string());
                }
            }
            requires_build = true;
        },
        _ => {
            start_commands.push("echo 'Unknown application type'".to_string());
        }
    }
    
    Ok((build_commands, start_commands, requires_build))
}

fn collect_files(repo_path: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    
    for entry in WalkDir::new(repo_path).max_depth(2) {
        let entry = entry?;
        if entry.file_type().is_file() {
            if let Some(file_name) = entry.file_name().to_str() {
                files.push(file_name.to_string());
            }
        }
    }
    
    Ok(files)
}

fn file_contains_keyword(repo_path: &Path, file_name: &str, keyword: &str) -> Result<bool> {
    for entry in WalkDir::new(repo_path).max_depth(3) {
        let entry = entry?;
        if entry.file_type().is_file() {
            if let Some(name) = entry.file_name().to_str() {
                if name == file_name {
                    if let Ok(content) = fs::read_to_string(entry.path()) {
                        return Ok(content.contains(keyword));
                    }
                }
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn test_clone_real_repository() {
        let rt = Runtime::new().unwrap();
        let result = rt.block_on(clone_repository("https://github.com/Arvo-AI/hello_world"));
        
        assert!(result.is_ok());
        let temp_dir = result.unwrap();
        
        // Check that the cloned directory exists and has expected files
        let repo_path = temp_dir.path();
        assert!(repo_path.exists());
        assert!(repo_path.is_dir());
        
        // Should have some common files
        let readme_path = repo_path.join("README.md");
        assert!(readme_path.exists() || repo_path.join("readme.md").exists());
    }

    #[test]
    fn test_analyze_flask_repository() {
        let rt = Runtime::new().unwrap();
        let temp_dir = rt.block_on(clone_repository("https://github.com/Arvo-AI/hello_world")).unwrap();
        
        let analysis = analyze_repository(temp_dir.path()).unwrap();
        
        assert_eq!(analysis.app_type, ApplicationType::Flask);
        assert!(matches!(analysis.package_manager, PackageManager::Pip));
        assert!(analysis.requires_build_step);
        assert!(analysis.exposed_ports.contains(&5000));
    }

    #[test]
    fn test_detect_application_type() {
        // Create a temporary directory with test files
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_path = temp_dir.path();
        
        // Test Flask detection
        fs::write(repo_path.join("requirements.txt"), "Flask==2.0.1").unwrap();
        fs::write(repo_path.join("app.py"), "from flask import Flask").unwrap();
        
        let app_type = detect_application_type(repo_path).unwrap();
        assert_eq!(app_type, ApplicationType::Flask);
    }

    #[test]
    fn test_detect_package_manager() {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_path = temp_dir.path();
        
        // Test pip detection
        fs::write(repo_path.join("requirements.txt"), "Flask==2.0.1").unwrap();
        let pkg_mgr = detect_package_manager(repo_path).unwrap();
        assert_eq!(pkg_mgr, PackageManager::Pip);
        
        // Test npm detection
        fs::remove_file(repo_path.join("requirements.txt")).ok();
        fs::write(repo_path.join("package.json"), r#"{"name": "test"}"#).unwrap();
        let pkg_mgr = detect_package_manager(repo_path).unwrap();
        assert_eq!(pkg_mgr, PackageManager::Npm);
    }

    #[test]
    fn test_extract_dependencies() {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_path = temp_dir.path();
        
        // Test Python requirements
        fs::write(repo_path.join("requirements.txt"), "Flask==2.0.1\nrequests>=2.25.0\n# comment\n").unwrap();
        let deps = extract_dependencies(repo_path, &PackageManager::Pip).unwrap();
        assert!(deps.contains(&"Flask".to_string()));
        assert!(deps.contains(&"requests".to_string()));
        assert_eq!(deps.len(), 2);
        
        // Test package.json
        fs::write(repo_path.join("package.json"), r#"{"dependencies": {"express": "^4.17.1", "lodash": "^4.17.21"}}"#).unwrap();
        let deps = extract_dependencies(repo_path, &PackageManager::Npm).unwrap();
        assert!(deps.contains(&"express".to_string()));
        assert!(deps.contains(&"lodash".to_string()));
    }

    #[test]
    fn test_generate_commands() {
        let analysis = RepositoryAnalysis {
            app_type: ApplicationType::Flask,
            dependencies: vec!["Flask".to_string()],
            build_commands: vec![],
            start_commands: vec![],
            environment_variables: vec![],
            exposed_ports: vec![5000],
            static_files_dir: None,
            database_migrations: false,
            requires_build_step: false,
            docker_config: None,
            package_manager: PackageManager::Pip,
        };
        
        let (build_commands, start_commands, requires_build) = generate_commands(&analysis).unwrap();
        
        assert!(build_commands.contains(&"pip install -r requirements.txt".to_string()));
        assert!(start_commands.contains(&"python app.py".to_string()));
        assert!(requires_build);
    }

    #[test]
    fn test_detect_exposed_ports() {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_path = temp_dir.path();
        
        // Create a Python file with port configuration
        fs::write(repo_path.join("app.py"), "app.run(host='0.0.0.0', port=3000)").unwrap();
        
        let ports = detect_exposed_ports(repo_path).unwrap();
        assert!(ports.contains(&3000));
    }
}