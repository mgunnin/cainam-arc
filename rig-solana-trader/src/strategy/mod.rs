//! Trading Strategy Implementation
//!
//! This module implements the core trading logic using LLM-powered analysis.
//! The strategy combines multiple factors:
//!
//! # Analysis Factors
//! - Market momentum and trends
//! - Volume and liquidity analysis
//! - Price action patterns
//! - Social sentiment and metrics
//! - On-chain activity
//!
//! # Risk Management
//! Configurable parameters (via .env):
//! - `MAX_POSITION_SIZE_SOL`: Maximum position size (default: 1.0 SOL)
//! - `MIN_POSITION_SIZE_SOL`: Minimum position size (default: 0.1 SOL)
//! - `MAX_TOKENS_PER_WALLET`: Maximum concurrent positions
//! - `STOP_LOSS_PERCENTAGE`: Auto stop-loss trigger
//! - `TAKE_PROFIT_PERCENTAGE`: Auto take-profit levels
//! - `MIN_LIQUIDITY_USD`: Minimum liquidity requirement
//! - `MIN_CONFIDENCE_THRESHOLD`: Required confidence for trades
//!
//! # Position Management
//! - Automatic position tracking
//! - Partial profit taking
//! - Dynamic position sizing
//! - Trading cooldown periods

pub mod llm;
pub mod technical;
pub mod risk;
pub mod execution;

use crate::market_data::{EnhancedTokenMetadata, FeatureVector, MacroIndicator};
use anyhow::Result;
use rig::agent::Agent;
use rig::completion::{CompletionModel, Prompt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::Utc;
use crate::analysis::Analysis;
use solana_sdk::nonce::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioPosition {
    pub token: EnhancedTokenMetadata,
    pub quantity: f64,
    pub cost_basis_sol: f64,
    pub entry_timestamp: i64,
    pub partial_sells: Vec<PartialSell>,
    #[serde(default)]
    pub mongodb_id: Option<String>, // For MongoDB document tracking
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialSell {
    pub quantity: f64,
    pub price_sol: f64,
    pub timestamp: i64,
    pub tx_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingDecision {
    pub token_address: String,
    pub action: TradeAction,
    pub size_in_sol: f64,
    pub confidence: f64,
    pub reasoning: String,
    pub risk_score: f64,
    pub technical_signals: TechnicalSignals,
    pub market_context: MarketContext,
    pub execution_params: ExecutionParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalSignals {
    pub trend_strength: f64,
    pub momentum_score: f64,
    pub volatility_score: f64,
    pub support_resistance: Vec<f64>,
    pub signal_type: String,
    pub timeframe: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketContext {
    pub market_trend: String,
    pub sector_performance: f64,
    pub liquidity_score: f64,
    pub volume_profile: String,
    pub sentiment_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionParams {
    pub entry_type: String,
    pub time_horizon: String,
    pub stop_loss: f64,
    pub take_profit: Vec<f64>,
    pub max_slippage: f64,
    pub dca_config: Option<DCAConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DCAConfig {
    pub num_entries: u32,
    pub time_between_entries: u32,
    pub size_per_entry: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TradeAction {
    Buy,
    Sell,
    Hold
}

pub struct TradingStrategy<M: CompletionModel> {
    agent: Agent<M>,
    risk_manager: risk::RiskManager,
    technical_analyzer: technical::TechnicalAnalyzer,
    execution_engine: execution::ExecutionEngine,
    portfolio: HashMap<String, PortfolioPosition>,
    config: StrategyConfig,
}

#[derive(Debug, Clone)]
pub struct StrategyConfig {
    pub max_position_sol: f64,
    pub min_position_sol: f64,
    pub max_tokens: u32,
    pub min_confidence: f64,
    pub min_liquidity_usd: f64,
    pub max_slippage: f64,
}

impl<M: CompletionModel> TradingStrategy<M> {
    pub fn new(
        agent: Agent<M>,
        config: StrategyConfig,
    ) -> Self {
        Self {
            agent,
            risk_manager: risk::RiskManager::new(config.clone()),
            technical_analyzer: technical::TechnicalAnalyzer::new(),
            execution_engine: execution::ExecutionEngine::new(config.max_slippage),
            portfolio: HashMap::new(),
            config,
        }
    }

    pub async fn analyze_opportunity(
        &self,
        token: &EnhancedTokenMetadata,
        features: &FeatureVector,
        macro_indicators: &MacroIndicator,
    ) -> Result<TradingDecision> {
        // 1. Technical Analysis
        let technical_signals = self.technical_analyzer.analyze(token).await?;

        // 2. Market Context Analysis
        let market_context = self.analyze_market_context(token, macro_indicators).await?;

        // 3. Risk Assessment
        let risk_score = self.risk_manager.assess_risk(token, &technical_signals, &market_context).await?;

        // 4. LLM-based Analysis
        let llm_analysis = self.perform_llm_analysis(
            token,
            features,
            &technical_signals,
            &market_context,
            risk_score,
        ).await?;

        // 5. Final Decision Making
        let decision = self.make_decision(
            token,
            llm_analysis,
            risk_score,
            &technical_signals,
            &market_context,
        ).await?;

        Ok(decision)
    }

    async fn analyze_market_context(
        &self,
        token: &EnhancedTokenMetadata,
        macro_indicators: &MacroIndicator,
    ) -> Result<MarketContext> {
        Ok(MarketContext {
            market_trend: macro_indicators.market_trend.clone(),
            sector_performance: 0.0, // TODO: Implement sector analysis
            liquidity_score: token.liquidity_usd / token.market_cap,
            volume_profile: if token.volume_change_24h > 50.0 { "High".to_string() } else { "Normal".to_string() },
            sentiment_score: token.social_sentiment.unwrap_or(0.0),
        })
    }

    async fn perform_llm_analysis(
        &self,
        token: &EnhancedTokenMetadata,
        features: &FeatureVector,
        technical_signals: &TechnicalSignals,
        market_context: &MarketContext,
        risk_score: f64,
    ) -> Result<String> {
        let prompt = format!(
            r#"Analyze trading opportunity for token {}.
Technical Signals:
- Trend Strength: {:.2}
- Momentum Score: {:.2}
- Volatility Score: {:.2}
- Signal Type: {}

Market Context:
- Market Trend: {}
- Liquidity Score: {:.2}
- Volume Profile: {}
- Sentiment Score: {:.2}

Risk Score: {:.2}

Additional Metrics:
- Price Change 24h: {:.2}%
- Volume Change 24h: {:.2}%
- Liquidity Change 24h: {:.2}%

Provide trading analysis and recommendation in a concise format."#,
            token.symbol,
            technical_signals.trend_strength,
            technical_signals.momentum_score,
            technical_signals.volatility_score,
            technical_signals.signal_type,
            market_context.market_trend,
            market_context.liquidity_score,
            market_context.volume_profile,
            market_context.sentiment_score,
            risk_score,
            token.price_change_24h,
            token.volume_change_24h,
            token.liquidity_change_24h,
        );

        let response = self.agent.prompt(&prompt).await?;
        Ok(response.to_string())
    }

    async fn make_decision(
        &self,
        token: &EnhancedTokenMetadata,
        llm_analysis: String,
        risk_score: f64,
        technical_signals: &TechnicalSignals,
        market_context: &MarketContext,
    ) -> Result<TradingDecision> {
        // Parse LLM analysis
        let analysis: serde_json::Value = serde_json::from_str(&llm_analysis)?;
        
        // Extract key metrics for decision making
        let confidence = analysis["confidence"].as_f64().unwrap_or(0.0);
        let momentum = analysis["market_analysis"]["momentum_indicators"]["overall_momentum"].as_str().unwrap_or("neutral");
        let liquidity_score = analysis["market_analysis"]["liquidity_assessment"]["liquidity_score"].as_f64().unwrap_or(0.0);
        let smart_money_flow = analysis["market_analysis"]["on_chain_metrics"]["smart_money_flow"].as_str().unwrap_or("neutral");
        
        // Determine action based on multiple factors
        let action = if confidence >= self.config.min_confidence 
            && liquidity_score >= 0.7 
            && risk_score <= 0.3 
            && (momentum == "strong_buy" || momentum == "buy") 
            && smart_money_flow == "inflow" 
        {
            TradeAction::Buy
        } else if risk_score > 0.7 
            || momentum == "strong_sell" 
            || smart_money_flow == "outflow" 
        {
            TradeAction::Sell
        } else {
            TradeAction::Hold
        };

        // Calculate position size based on multiple factors
        let base_size = self.calculate_position_size(risk_score, confidence);
        let liquidity_multiplier = (liquidity_score * 1.5).min(1.0);
        let momentum_multiplier = match momentum {
            "strong_buy" => 1.0,
            "buy" => 0.8,
            "neutral" => 0.5,
            _ => 0.3,
        };
        
        let final_size = (base_size * liquidity_multiplier * momentum_multiplier)
            .max(self.config.min_position_sol)
            .min(self.config.max_position_sol);

        // Generate execution parameters
        let execution_params = self.generate_execution_params(
            technical_signals,
            risk_score,
            &analysis["execution_strategy"],
        )?;

        Ok(TradingDecision {
            token_address: token.address.clone(),
            action,
            size_in_sol: final_size,
            confidence,
            reasoning: analysis["reasoning"].as_str().unwrap_or("").to_string(),
            risk_score,
            technical_signals: technical_signals.clone(),
            market_context: market_context.clone(),
            execution_params,
        })
    }

    fn calculate_position_size(&self, risk_score: f64, trend_strength: f64) -> f64 {
        let base_size = self.config.max_position_sol * 0.2;
        let risk_multiplier = 1.0 - risk_score;
        let trend_multiplier = trend_strength;
        
        (base_size * risk_multiplier * trend_multiplier)
            .max(self.config.min_position_sol)
            .min(self.config.max_position_sol)
    }

    fn generate_execution_params(
        &self,
        signals: &TechnicalSignals,
        risk_score: f64,
        execution_strategy: &serde_json::Value,
    ) -> Result<ExecutionParams> {
        let stop_loss = if let Some(sl) = execution_strategy["stop_loss_pct"].as_f64() {
            sl
        } else {
            // Fallback to risk-based stop loss
            if risk_score > 0.7 { 0.05 } else { 0.1 }
        };

        let take_profits = if let Some(tp_array) = execution_strategy["take_profit_levels"].as_array() {
            tp_array.iter()
                .filter_map(|tp| tp["price_target"].as_f64())
                .collect()
        } else {
            vec![0.1, 0.2, 0.3] // Default take profit levels
        };

        let dca_config = if execution_strategy["dca_strategy"]["should_dca"].as_bool().unwrap_or(false) {
            Some(DCAConfig {
                num_entries: execution_strategy["dca_strategy"]["num_entries"].as_u64().unwrap_or(3) as u32,
                time_between_entries: execution_strategy["dca_strategy"]["interval_hours"].as_u64().unwrap_or(24) as u32,
                size_per_entry: execution_strategy["position_size_sol"].as_f64().unwrap_or(0.1),
            })
        } else {
            None
        };

        Ok(ExecutionParams {
            entry_type: execution_strategy["entry_type"].as_str().unwrap_or("Market").to_string(),
            time_horizon: signals.timeframe.clone(),
            stop_loss,
            take_profit: take_profits,
            max_slippage: self.config.max_slippage,
            dca_config,
        })
    }

    pub fn update_portfolio(&mut self, token: EnhancedTokenMetadata, quantity: f64, cost_basis_sol: f64) {
        let now = Utc::now().timestamp();
        let token_address = token.address.clone();
        self.portfolio.insert(
            token.address.clone(),
            PortfolioPosition {
                token,
                quantity,
                cost_basis_sol,
                entry_timestamp: now,
                partial_sells: Vec::new(),
                mongodb_id: None,
            },
        );
    }

    pub fn record_partial_sell(
        &mut self,
        token_address: &str,
        quantity: f64,
        price_sol: f64,
    ) -> Result<()> {
        let position = self.portfolio.get_mut(token_address)
            .ok_or_else(|| anyhow::anyhow!("Position not found"))?;

        let now = Utc::now().timestamp();
        position.partial_sells.push(PartialSell {
            quantity,
            price_sol,
            timestamp: now,
            tx_signature: None,
        });
        position.quantity -= quantity;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::providers::openai;

    #[tokio::test]
    async fn test_trading_strategy() {
        // Add tests with mock agent responses
    }
}