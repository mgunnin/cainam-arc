use crate::{
    market_data::MarketDataProvider,
    strategy::{TradingStrategy, analytics::PerformanceAnalyzer},
};
use anyhow::Result;
use rustyline::Editor;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

pub struct TradingAgentCLI {
    market_data: Arc<MarketDataProvider>,
    strategy: Arc<TradingStrategy>,
    performance_analyzer: Arc<Mutex<PerformanceAnalyzer>>,
    editor: Editor<()>,
}

impl TradingAgentCLI {
    pub fn new(
        market_data: Arc<MarketDataProvider>,
        strategy: Arc<TradingStrategy>,
        performance_analyzer: Arc<Mutex<PerformanceAnalyzer>>,
    ) -> Self {
        Self {
            market_data,
            strategy,
            performance_analyzer,
            editor: Editor::<()>::new().unwrap(),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        println!("\n🤖 Welcome to the Solana Trading Agent CLI!");
        println!("Type 'help' to see available commands\n");

        loop {
            let input = match self.editor.readline("agent> ") {
                Ok(line) => {
                    self.editor.add_history_entry(line.as_str());
                    line
                }
                Err(_) => break,
            };

            let command = input.trim();
            match self.handle_command(command).await {
                Ok(_) => continue,
                Err(e) => println!("Error: {}", e),
            }
        }

        Ok(())
    }

    async fn handle_command(&self, command: &str) -> Result<()> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }

        match parts[0] {
            "help" => self.show_help(),
            "analyze" if parts.len() > 1 => self.analyze_token(parts[1]).await?,
            "performance" => self.show_performance().await?,
            "positions" => self.show_positions().await?,
            "signals" if parts.len() > 1 => self.show_signals(parts[1]).await?,
            "market" => self.show_market_overview().await?,
            "trending" => self.show_trending_tokens().await?,
            "risk" if parts.len() > 1 => self.analyze_risk(parts[1]).await?,
            "settings" => self.show_settings(),
            "exit" | "quit" => std::process::exit(0),
            _ => println!("Unknown command. Type 'help' to see available commands."),
        }

        Ok(())
    }

    fn show_help(&self) {
        println!("\nAvailable Commands:");
        println!("  analyze <token>     - Analyze a specific token (e.g., 'analyze SOL')");
        println!("  performance         - Show trading performance metrics");
        println!("  positions           - Show current portfolio positions");
        println!("  signals <token>     - Show trading signals for a token");
        println!("  market             - Show market overview");
        println!("  trending           - Show trending tokens");
        println!("  risk <token>       - Analyze risk for a token");
        println!("  settings           - Show current trading settings");
        println!("  help               - Show this help message");
        println!("  exit/quit          - Exit the program\n");
    }

    async fn analyze_token(&self, token: &str) -> Result<()> {
        println!("\nAnalyzing token: {}", token);
        
        // Get token metadata
        let metadata = self.market_data.get_token_metadata(token).await?;
        
        // Get market analysis
        let analysis = self.strategy.analyze_token_opportunity(&metadata).await?;
        
        // Format and display results
        println!("\nToken Analysis:");
        println!("Symbol: {} ({})", metadata.symbol, metadata.address);
        println!("Price: ${:.4}", metadata.price_usd.unwrap_or_default());
        println!("24h Change: {:.2}%", metadata.price_change_24h);
        println!("24h Volume: ${:.2}", metadata.volume_24h);
        println!("Market Cap: ${:.2}", metadata.market_cap);
        println!("\nTrading Analysis:");
        println!("Action: {:?}", analysis.action);
        println!("Confidence: {:.2}%", analysis.confidence * 100.0);
        println!("Risk Score: {:.2}", analysis.risk_score);
        println!("Reasoning: {}", analysis.reasoning);
        
        if let Some(signals) = &analysis.technical_signals {
            println!("\nTechnical Signals:");
            println!("Trend: {:?}", signals.trend_direction);
            println!("RSI: {:.2}", signals.rsi_signal);
            println!("MACD: {:?}", signals.macd_signal);
        }

        Ok(())
    }

    async fn show_performance(&self) -> Result<()> {
        let metrics = self.performance_analyzer.lock().await.get_metrics();
        
        println!("\nTrading Performance:");
        println!("Total Trades: {}", metrics.total_trades);
        println!("Win Rate: {:.2}%", metrics.win_rate * 100.0);
        println!("Total P/L: {:.4} SOL", metrics.total_profit_loss);
        println!("Sharpe Ratio: {:.2}", metrics.sharpe_ratio);
        println!("Max Drawdown: {:.2}%", metrics.max_drawdown * 100.0);
        println!("Risk-Adjusted Return: {:.2}", metrics.risk_adjusted_return);
        
        if !metrics.asset_performance.is_empty() {
            println!("\nTop Performing Assets:");
            let mut assets: Vec<_> = metrics.asset_performance.values().collect();
            assets.sort_by(|a, b| b.total_profit_loss.partial_cmp(&a.total_profit_loss).unwrap());
            
            for asset in assets.iter().take(5) {
                println!(
                    "{}: {:.4} SOL ({:.2}% win rate)",
                    asset.symbol,
                    asset.total_profit_loss,
                    asset.win_rate * 100.0
                );
            }
        }

        Ok(())
    }

    async fn show_positions(&self) -> Result<()> {
        let positions = self.strategy.get_portfolio_positions();
        
        if positions.is_empty() {
            println!("\nNo active positions");
            return Ok(());
        }

        println!("\nActive Positions:");
        for pos in positions {
            println!(
                "{}: {:.4} SOL (Entry: ${:.4}, Current: ${:.4})",
                pos.token.symbol,
                pos.quantity,
                pos.cost_basis_sol,
                pos.token.price_usd.unwrap_or_default()
            );
        }

        Ok(())
    }

    async fn show_signals(&self, token: &str) -> Result<()> {
        let signals = self.market_data.get_market_signals(token).await?;
        
        println!("\nTrading Signals for {}:", token);
        println!("Price Signals:");
        println!("  Trend: {}", signals.price_trend);
        println!("  Support: ${:.4}", signals.support_level);
        println!("  Resistance: ${:.4}", signals.resistance_level);
        
        println!("\nTechnical Indicators:");
        println!("  RSI (14): {:.2}", signals.rsi_14);
        println!("  MACD: {:.4}", signals.macd);
        println!("  MA50: ${:.4}", signals.ma_50);
        println!("  MA200: ${:.4}", signals.ma_200);
        
        println!("\nMarket Activity:");
        println!("  Volume Trend: {}", signals.volume_trend);
        println!("  Liquidity Score: {:.2}", signals.liquidity_score);
        println!("  Smart Money Flow: {}", signals.smart_money_flow);

        Ok(())
    }

    async fn show_market_overview(&self) -> Result<()> {
        let overview = self.market_data.get_market_overview().await?;
        
        println!("\nMarket Overview:");
        println!("Overall Trend: {}", overview.market_trend);
        println!("24h Volume: ${:.2}M", overview.total_volume_24h / 1_000_000.0);
        println!("Active Tokens: {}", overview.active_tokens);
        println!("Average Volume: ${:.2}K", overview.avg_token_volume / 1_000.0);
        
        if !overview.top_movers.is_empty() {
            println!("\nTop Movers (24h):");
            for mover in overview.top_movers.iter().take(5) {
                println!(
                    "{}: {:.2}% (${:.4})",
                    mover.symbol,
                    mover.price_change_24h,
                    mover.price_usd
                );
            }
        }

        Ok(())
    }

    async fn show_trending_tokens(&self) -> Result<()> {
        let trending = self.market_data.get_trending_tokens(10).await?;
        
        println!("\nTrending Tokens:");
        for token in trending {
            println!(
                "{}: ${:.4} ({:.2}% 24h) - Vol: ${:.2}K",
                token.symbol,
                token.price_usd.unwrap_or_default(),
                token.price_change_24h,
                token.volume_24h / 1_000.0
            );
        }

        Ok(())
    }

    async fn analyze_risk(&self, token: &str) -> Result<()> {
        let metadata = self.market_data.get_token_metadata(token).await?;
        let risk = self.strategy.analyze_token_risk(&metadata).await?;
        
        println!("\nRisk Analysis for {}:", token);
        println!("Overall Risk Score: {:.2}", risk.risk_score);
        println!("Volatility Rating: {:?}", risk.volatility_rating);
        println!("Market Risk Level: {:?}", risk.market_risk_level);
        println!("Max Position Size: {:.4} SOL", risk.max_position_size);
        
        if !risk.risk_factors.is_empty() {
            println!("\nRisk Factors:");
            for factor in risk.risk_factors {
                println!("- {}", factor);
            }
        }

        Ok(())
    }

    fn show_settings(&self) {
        println!("\nTrading Settings:");
        println!("Max Position Size: {} SOL", self.strategy.config.max_position_sol);
        println!("Min Position Size: {} SOL", self.strategy.config.min_position_sol);
        println!("Max Tokens: {}", self.strategy.config.max_tokens);
        println!("Min Confidence: {:.2}", self.strategy.config.min_confidence);
        println!("Min Liquidity: ${}", self.strategy.config.min_liquidity_usd);
        println!("Max Slippage: {:.2}%", self.strategy.config.max_slippage * 100.0);
    }
}