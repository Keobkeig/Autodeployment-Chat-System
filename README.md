# AutoDeployment System

A Rust-based command-line tool that automates application deployment based on natural language input and repository analysis. The system intelligently analyzes code repositories, determines optimal infrastructure configurations, and generates Terraform configurations for deployment with secure credential management.

## Features

- **Natural Language Processing**: Parse deployment requirements from human-readable descriptions using Google Gemini AI
- **Intelligent Repository Analysis**: Automatically detect application types, dependencies, and configurations
- **Smart Infrastructure Decisions**: Choose optimal deployment strategies (VM, containers, serverless, Kubernetes)
- **Multi-Cloud Support**: AWS, GCP, Azure with secure credential management
- **Terraform Integration**: Generate production-ready infrastructure-as-code
- **Interactive Chat Mode**: Conversational interface for deployment planning
- **Cost Estimation**: Provide cost estimates for different deployment options
- **Secure Credential Management**: Store and manage cloud platform credentials securely

## 🛠️ Installation

### Prerequisites

- Rust 1.70+ 
- Git
- Terraform (required for actual deployments)
- Google Gemini API key (for AI-powered natural language processing)

### Build from Source

```bash
   git clone <repository-url>
   cd autodeployment-system
   cargo build --release
```

### Setup

1. **Set up environment variables**:
   ```bash
   # Edit .env and add your Google Gemini API key
   ```
   
Required:
- `GEMINI_API_KEY`: Google Gemini API key for AI-powered natural language processing

Optional:
- `RUST_LOG`: Set logging level (`debug`, `info`, `warn`, `error`)

Example `.env` file:
```env
GEMINI_API_KEY=your_gemini_api_key_here
RUST_LOG=info
```

2. **Configure cloud platform credentials** (choose your cloud provider):
   ```bash
   # AWS
   cargo run -- credentials setup aws
   
   # Google Cloud Platform
   cargo run -- credentials setup gcp
   
   # Microsoft Azure
   cargo run -- credentials setup azure
   ```

3. **Check credential status**:
   ```bash
   cargo run -- credentials status
   ```

## Usage

### Command Line Interface

Deploy an application with a single command:

```bash
# Deploy with natural language description
cargo run -- deploy \
  --description "Deploy this Flask application on GCP" \
  --repository "https://github.com/Arvo-AI/hello_world"
  --cloud-provider "gcp" 
```

### Interactive Chat Mode

Start an interactive session for deployment planning:

```bash
cargo run -- chat

# Or load a repository upfront
cargo run -- chat --repository "https://github.com/user/repo"
```

Chat commands:
- `load <repo_url>` - Load and analyze a repository
- `status` - Show current repository information
- `plan <description>` - Plan deployment without executing
- `deploy <description>` - Deploy the application
- `help` - Show available commands
- `quit` - Exit the chat

## Architecture

The system consists of five main modules:

### 1. AI-Powered NLP (`src/ai_nlp.rs`)
- Uses Google Gemini 2.5 Flash for natural language processing
- Parses deployment requirements from human descriptions
- Generates Terraform configurations with AI assistance
- Supports complex deployment scenarios and infrastructure decisions

### 2. Repository Analysis (`src/repository.rs`)
- Clones and analyzes Git repositories
- Detects application types and frameworks
- Extracts dependencies, build commands, and configuration
- Identifies ports, static files, and database migrations

### 3. Infrastructure Decision Engine (`src/infrastructure.rs`)
- Determines optimal deployment strategy
- Chooses between VM, containers, serverless, or Kubernetes
- Generates Terraform configurations with AI integration
- Provides cost estimates and justification

### 4. Credential Management (`src/credentials.rs`)
- Secure storage of cloud platform credentials
- Multi-cloud authentication support (AWS, GCP, Azure)
- Environment variable injection for Terraform
- Interactive credential setup and management

### 5. Deployment Orchestration (`src/deployment.rs`)
- Coordinates the entire deployment process
- Manages interactive chat sessions
- Handles error scenarios and logging
- Integrates credential validation with deployment flow

## Natural Language Examples

The system understands various deployment requirements:

- **"Deploy this Flask application on AWS"** → Single VM on AWS
- **"Deploy with auto-scaling and load balancing"** → Kubernetes cluster
- **"Deploy serverless on Azure"** → Azure Functions
- **"Deploy with PostgreSQL database"** → VM + RDS/Cloud SQL
- **"Deploy static site with CDN"** → S3/Cloud Storage + CDN

## Cost Estimation

The system provides cost estimates for different deployment options:

- **Single VM**: ~$8.76/month (AWS t3.micro)
- **Container Service**: ~$25/month
- **Kubernetes**: ~$73/month
- **Serverless**: ~$5/month (usage-based)
- **Static Site**: ~$1/month

### Terraform Output

Generated Terraform files are saved to:
- `./terraform-output/deployment_YYYYMMDD_HHMMSS/`
- Contains: `main.tf`, `variables.tf`, `outputs.tf`

## Security Considerations

- Repository cloning uses temporary directories
- **Environment variables**: API keys stored in `.env` file (excluded from version control)
- **Secure credential storage**: Cloud credentials stored in `~/.autodeployment/credentials.json` with 0o600 permissions (readable only by owner)
- **Environment variable injection**: Credentials passed to Terraform via environment variables, never logged
- **Temporary files**: GCP service account keys written to secure temporary files during deployment
- **Git ignore**: `.env` file is excluded from version control to prevent accidental API key commits
- Terraform state should be managed securely in production
- Generated configurations follow security best practices

## Dependencies and Sources

### Crates 
- **clap**: Command-line argument parsing
- **tokio**: Async runtime
- **reqwest**: HTTP client for AI API calls
- **serde**: Serialization framework
- **anyhow**: Error handling
- **regex**: Regular expressions
- **walkdir**: Directory traversal
- **tempfile**: Temporary file management
- **log/env_logger**: Logging framework
- **dirs**: Home directory detection
- **chrono**: Date/time handling
- **dotenv**: Environment variable loading from .env files

### External Tools
- **Git**: Repository cloning
- **Terraform**: Infrastructure provisioning

## License

This project is open-source and available under the MIT License.
---
