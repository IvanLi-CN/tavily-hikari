mod server;

use std::net::SocketAddr;

use clap::{Parser, Subcommand};
use dotenvy::dotenv;
use tavily_hikari::{
    DEFAULT_SEARCH_ENDPOINT, SearchDepth, SearchOptions, TavilyBalancer, TavilyError,
};

#[derive(Debug, Parser)]
#[command(author, version, about = "Tavily Hikari client and MCP server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run a one-off Tavily search using the load-balanced client.
    Query(QueryArgs),
    /// Start the HTTP(S) MCP server that exposes Tavily as a remote tool.
    Serve(ServeArgs),
}

#[derive(Debug, Parser)]
struct QueryArgs {
    /// Comma separated Tavily API keys or repeat the flag multiple times.
    #[arg(
        short,
        long,
        value_delimiter = ',',
        env = "TAVILY_API_KEYS",
        hide_env_values = true,
        required = true
    )]
    keys: Vec<String>,

    /// Query string sent to Tavily.
    #[arg(short, long)]
    query: String,

    /// Override the Tavily endpoint URL.
    #[arg(long, env = "TAVILY_ENDPOINT", default_value = DEFAULT_SEARCH_ENDPOINT)]
    endpoint: String,

    /// SQLite database path used to persist key health state and request logs.
    #[arg(long, env = "TAVILY_DB_PATH", default_value = "tavily_keys.db")]
    db_path: String,

    /// Request search depth (basic or advanced).
    #[arg(long, value_enum)]
    search_depth: Option<SearchDepth>,

    /// Limit the maximum number of results.
    #[arg(long)]
    max_results: Option<u8>,

    /// Include generated answer content.
    #[arg(long)]
    include_answer: bool,

    /// Include image URLs when available.
    #[arg(long)]
    include_images: bool,

    /// Include raw page content.
    #[arg(long)]
    include_raw_content: bool,

    /// Restrict search to specific domains.
    #[arg(long, value_delimiter = ',')]
    include_domains: Option<Vec<String>>,

    /// Exclude specific domains from the search.
    #[arg(long, value_delimiter = ',')]
    exclude_domains: Option<Vec<String>>,
}

#[derive(Debug, Parser)]
struct ServeArgs {
    /// Comma separated Tavily API keys or repeat the flag multiple times.
    #[arg(
        long,
        value_delimiter = ',',
        env = "TAVILY_API_KEYS",
        hide_env_values = true
    )]
    keys: Option<Vec<String>>,

    /// Override the Tavily endpoint URL.
    #[arg(long, env = "TAVILY_ENDPOINT", default_value = DEFAULT_SEARCH_ENDPOINT)]
    endpoint: String,

    /// SQLite database path used to persist key health state and request logs.
    #[arg(long, env = "TAVILY_DB_PATH", default_value = "tavily_keys.db")]
    db_path: String,

    /// Address to bind the MCP server to.
    #[arg(long, env = "MCP_BIND", default_value = "127.0.0.1")]
    bind: String,

    /// Port to bind the MCP server to.
    #[arg(long, env = "MCP_PORT", default_value_t = 8080)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();

    match cli.command {
        Command::Query(args) => {
            if let Err(err) = run_query(args).await {
                report_error(&err);
                std::process::exit(1);
            }
        }
        Command::Serve(args) => {
            run_server(args).await?;
        }
    }

    Ok(())
}

async fn run_query(args: QueryArgs) -> Result<(), TavilyError> {
    let balancer = TavilyBalancer::with_endpoint(args.keys, &args.endpoint, &args.db_path).await?;

    let options = SearchOptions {
        search_depth: args.search_depth,
        max_results: args.max_results,
        include_answer: flag_option(args.include_answer),
        include_images: flag_option(args.include_images),
        include_raw_content: flag_option(args.include_raw_content),
        include_domains: sanitize_optional(args.include_domains),
        exclude_domains: sanitize_optional(args.exclude_domains),
    };

    let response = balancer.search(&args.query, &options).await?;
    match serde_json::to_string_pretty(&response) {
        Ok(pretty) => println!("{pretty}"),
        Err(_) => println!("{response}"),
    }

    Ok(())
}

async fn run_server(args: ServeArgs) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = args.endpoint.clone();
    let db_path = args.db_path.clone();
    let keys = args.keys.unwrap_or_default();

    let balancer = TavilyBalancer::with_endpoint(keys, &endpoint, &db_path).await?;
    let addr: SocketAddr = format!("{}:{}", args.bind, args.port).parse()?;

    server::serve(addr, balancer).await
}

fn sanitize_optional(items: Option<Vec<String>>) -> Option<Vec<String>> {
    let items = items.map(|values| {
        values
            .into_iter()
            .map(|item| item.trim().to_owned())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
    });
    items.filter(|list| !list.is_empty())
}

fn flag_option(flag: bool) -> Option<bool> {
    flag.then_some(true)
}

fn report_error(err: &TavilyError) {
    eprintln!("error: {err}");
    match err {
        TavilyError::Http(source) => {
            if let Some(status) = source.status() {
                eprintln!("  status: {status}");
            }
        }
        TavilyError::Database(db_err) => {
            eprintln!("  database: {db_err}");
        }
        TavilyError::Serialization(err) => {
            eprintln!("  serialization: {err}");
        }
        _ => {}
    }
}
