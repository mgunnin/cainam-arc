use anyhow::Result;
use dotenv::dotenv;
use rig_core::providers::openai;
use rig_solana_trader::{
    market_data::MarketDataProvider,
    strategy::{
        TradingStrategy, StrategyConfig, analytics::PerformanceAnalyzer,
        pipeline::TradingPipeline, risk::RiskManager, execution::ExecutionEngine,
    },
    cli::TradingAgentCLI,
    personality::StoicPersonality,
};
use solana_sdk::signature::{Keypair, read_keypair_file};
use std::{env, sync::Arc};
use tokio::sync::Mutex;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use jup_ag::Jupiter;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_level(true)
        .pretty()
        .init();

    // Load environment variables
    dotenv().ok();
    
    info!("Starting Solana trading agent with interactive CLI...");

    // Initialize OpenAI client and agent
    let openai_client = openai::Client::from_env();
    let agent = openai_client.agent("gpt-4").build();

    // Load wallet
    let wallet = if let Ok(private_key) = env::var("SOLANA_PRIVATE_KEY") {
        let bytes = bs58::decode(&private_key)
            .into_vec()
            .expect("Invalid private key");
        Keypair::from_bytes(&bytes).expect("Invalid keypair")
    } else {
        read_keypair_file(&*shellexpand::tilde("~/.config/solana/id.json"))
            .expect("Failed to read keypair file")
    };

    info!("Wallet loaded: {}", wallet.pubkey());

    // Initialize market data provider
    let market_data = Arc::new(MarketDataProvider::new(
        &env::var("BIRDEYE_API_KEY")?,
        &env::var("SOLANA_RPC_URL")?,
    ).await?);

    // Initialize strategy components
    let strategy_config = StrategyConfig {
        max_position_sol: env::var("MAX_POSITION_SIZE_SOL")?.parse()?,
        min_position_sol: env::var("MIN_POSITION_SIZE_SOL")?.parse()?,
        max_tokens: env::var("MAX_TOKENS_PER_WALLET")?.parse()?,
        min_confidence: env::var("MIN_CONFIDENCE_THRESHOLD")?.parse()?,
        min_liquidity_usd: env::var("MIN_LIQUIDITY_USD")?.parse()?,
        max_slippage: env::var("MAX_SLIPPAGE")?.parse()?,
    };

    let personality = StoicPersonality::new(
        env::var("MAX_DRAWDOWN")?.parse()?,
        env::var("MAX_POSITION_SIZE_SOL")?.parse()?,
    );

    let strategy = Arc::new(TradingStrategy::new(agent.clone(), strategy_config.clone()));
    let risk_manager = Arc::new(RiskManager::new(strategy_config.clone(), personality));

    // Initialize execution engine without Jupiter API key
    let execution_engine = Arc::new(ExecutionEngine::new(
        &env::var("SOLANA_RPC_URL")?,
        strategy_config.max_slippage,
        wallet.pubkey(),
    )?);

    // Initialize trading pipeline
    let pipeline = Arc::new(TradingPipeline::new(
        market_data.clone(),
        strategy.clone(),
        risk_manager.clone(),
        execution_engine.clone(),
    ));

    // Initialize performance analyzer
    let performance_analyzer = Arc::new(Mutex::new(PerformanceAnalyzer::new(
        env::var("INITIAL_PORTFOLIO_VALUE")?.parse()?,
    )));

    // Initialize CLI interface
    let mut cli = TradingAgentCLI::new(
        market_data.clone(),
        strategy.clone(),
        performance_analyzer.clone(),
    );

    // Spawn autonomous trading loop in a separate task
    let trading_task = {
        let market_data = market_data.clone();
        let pipeline = pipeline.clone();
        let performance_analyzer = performance_analyzer.clone();
        
        tokio::spawn(async move {
            loop {
                // Get trending tokens
                if let Ok(trending_tokens) = market_data.get_trending_tokens(10).await {
                    info!("Analyzing {} trending tokens", trending_tokens.len());

                    // Process each token
                    for token in trending_tokens {
                        if let Ok(Some(result)) = pipeline.process_token(&token.address).await {
                            // Record trade result
                            let mut analyzer = performance_analyzer.lock().await;
                            analyzer.record_trade(result.into());
                        }
                    }

                    // Monitor existing positions
                    if let Err(e) = pipeline.monitor_positions().await {
                        info!("Error monitoring positions: {}", e);
                    }
                }

                // Wait before next iteration
                sleep(Duration::from_secs(60)).await;
            }
        })
    };

    // Start CLI interface in the main task
    cli.start().await?;

    // Wait for trading task to complete (it won't unless there's an error)
    trading_task.await??;

    Ok(())
}