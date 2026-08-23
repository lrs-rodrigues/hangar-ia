#![forbid(unsafe_code)]

use std::io::Read;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use reqwest::blocking::Client;
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(name = "hangar", version, about = "CLI for the Hangar native API")]
struct Args {
    /// Base URL of a running Hangar server.
    #[arg(
        long,
        env = "HANGAR_SERVER_URL",
        default_value = "http://127.0.0.1:8080"
    )]
    server_url: String,

    /// Scoped Hangar API key. It is never written to disk by this CLI.
    #[arg(long, env = "HANGAR_API_KEY")]
    api_key: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check that the service is reachable.
    Health,
    /// Retrieve published durable memories in one workspace.
    SearchMemory {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        workspace_id: String,
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    /// Retrieve evidence-backed document chunks in one workspace.
    SearchDocuments {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        workspace_id: String,
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    /// Compile a governed, evidence-backed context package within a token budget.
    Context {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        workspace_id: String,
        #[arg(long)]
        query: String,
        #[arg(long)]
        token_budget: usize,
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    /// Create a proposed durable memory. Promotion is intentionally separate.
    ProposeMemory {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        workspace_id: String,
        #[arg(long)]
        content: Option<String>,
        /// Read the memory content from standard input.
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        source: Option<String>,
        #[arg(long, default_value_t = 1.0)]
        confidence: f32,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = Client::builder().build()?;
    let value = match args.command {
        Command::Health => request(&client, &args.server_url, None, "/health", None)?,
        Command::SearchMemory {
            organization_id,
            workspace_id,
            query,
            limit,
        } => request(
            &client,
            &args.server_url,
            args.api_key.as_deref(),
            "/v1/retrieve",
            Some(json!({
                "organization_id": organization_id,
                "workspace_id": workspace_id,
                "query": query,
                "limit": limit.clamp(1, 50),
            })),
        )?,
        Command::SearchDocuments {
            organization_id,
            workspace_id,
            query,
            limit,
        } => request(
            &client,
            &args.server_url,
            args.api_key.as_deref(),
            "/v1/retrieve/documents",
            Some(json!({
                "organization_id": organization_id,
                "workspace_id": workspace_id,
                "query": query,
                "limit": limit.clamp(1, 50),
            })),
        )?,
        Command::Context {
            organization_id,
            workspace_id,
            query,
            token_budget,
            limit,
        } => request(
            &client,
            &args.server_url,
            args.api_key.as_deref(),
            "/v1/context-packages",
            Some(json!({
                "organization_id": organization_id,
                "workspace_id": workspace_id,
                "query": query,
                "token_budget": token_budget.clamp(1, 8192),
                "limit": limit.clamp(1, 50),
            })),
        )?,
        Command::ProposeMemory {
            organization_id,
            workspace_id,
            content,
            stdin,
            source,
            confidence,
        } => {
            let content = memory_content(content, stdin)?;
            request(
                &client,
                &args.server_url,
                args.api_key.as_deref(),
                "/v1/memories",
                Some(json!({
                    "organization_id": organization_id,
                    "workspace_id": workspace_id,
                    "content": content,
                    "source": source,
                    "confidence": confidence,
                })),
            )?
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn memory_content(content: Option<String>, stdin: bool) -> anyhow::Result<String> {
    if content.is_some() == stdin {
        bail!("provide exactly one of --content or --stdin")
    }
    if let Some(content) = content {
        return Ok(content);
    }
    let mut content = String::new();
    std::io::stdin()
        .read_to_string(&mut content)
        .context("could not read memory content from standard input")?;
    if content.trim().is_empty() {
        bail!("memory content from standard input cannot be empty")
    }
    Ok(content)
}

fn request(
    client: &Client,
    server_url: &str,
    api_key: Option<&str>,
    path: &str,
    body: Option<Value>,
) -> anyhow::Result<Value> {
    let url = format!("{}{}", server_url.trim_end_matches('/'), path);
    let mut request = if let Some(body) = body {
        client.post(&url).json(&body)
    } else {
        client.get(&url)
    };
    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key);
    } else if path != "/health" {
        bail!("--api-key or HANGAR_API_KEY is required for this command")
    }
    let response = request
        .send()
        .with_context(|| format!("request to {url} failed"))?;
    let status = response.status();
    let body = response.text()?;
    let value = serde_json::from_str(&body)
        .with_context(|| format!("Hangar returned a non-JSON response ({status}): {body}"))?;
    if !status.is_success() {
        bail!("Hangar request failed ({status}): {value}")
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::memory_content;

    #[test]
    fn proposal_requires_one_content_source() {
        assert!(memory_content(None, false).is_err());
        assert!(memory_content(Some("x".into()), true).is_err());
        assert_eq!(memory_content(Some("x".into()), false).unwrap(), "x");
    }
}
