use anyhow::Result;
use rig::pipeline::{Op, Pipeline, TryOp};
use crate::{
    market_data::{MarketDataProvider, TokenAnalysis},
    strategy::{TradingStrategy, TradingDecision, ExecutionResult},
};
use std::sync::Arc;
use tracing::{info, debug, warn};

pub struct TradingPipeline<M> {
    market_data: Arc<MarketDataProvider>,
    strategy: Arc<TradingStrategy<M>>,
    risk_manager: Arc<RiskManager>,
    execution_engine: Arc<ExecutionEngine>,
}

impl<M: CompletionModel> TradingPipeline<M> {
    pub fn new(
        market_data: Arc<MarketDataProvider>,
        strategy: Arc<TradingStrategy<M>>,
        risk_manager: Arc<RiskManager>,
        execution_engine: Arc<ExecutionEngine>,
    ) -> Self {
        Self {
            market_data,
            strategy,
            risk_manager,
            execution_engine,
        }
    }

    pub async fn process_token(&self, token_address: &str) -> Result<Option<ExecutionResult>> {
        debug!("Processing token {}", token_address);

        // 1. Market Data Analysis
        let token_data = self.analyze_market_data(token_address).await?;
        
        // Skip if token doesn't meet basic criteria
        if !self.meets_basic_criteria(&token_data) {
            debug!("Token {} doesn't meet basic criteria", token_address);
            return Ok(None);
        }

        // 2. Technical Analysis
        let technical_signals = self.analyze_technical_indicators(&token_data).await?;

        // 3. Risk Assessment
        let risk_assessment = self.risk_manager.assess_risk(
            &token_data.metadata,
            &technical_signals,
            &token_data.market_context,
        ).await?;

        // Skip if risk is too high
        if risk_assessment.risk_score > 0.7 {
            debug!("Risk too high for token {}", token_address);
            return Ok(None);
        }

        // 4. Trading Decision
        let decision = self.strategy.analyze_opportunity(
            &token_data.metadata,
            &token_data.features,
            &token_data.macro_indicators,
        ).await?;

        // 5. Execute Trade if conditions are met
        match decision.action {
            TradeAction::Hold => {
                debug!("No trade action required for {}", token_address);
                Ok(None)
            }
            _ if decision.confidence >= 0.8 => {
                info!("Executing trade for {} with confidence {}", token_address, decision.confidence);
                let result = self.execution_engine.execute_trade(&decision).await?;
                Ok(Some(result))
            }
            _ => {
                debug!("Confidence too low for trade execution");
                Ok(None)
            }
        }
    }

    async fn analyze_market_data(&self, token_address: &str) -> Result<TokenAnalysis> {
        debug!("Analyzing market data for {}", token_address);
        self.market_data.analyze_token(token_address).await
    }

    async fn analyze_technical_indicators(&self, token_data: &TokenAnalysis) -> Result<TechnicalSignals> {
        let mut analyzer = TechnicalAnalyzer::new();
        analyzer.analyze(&token_data.price_history)
    }

    fn meets_basic_criteria(&self, token_data: &TokenAnalysis) -> bool {
        // Check minimum liquidity
        let min_liquidity_usd = 100_000.0; // $100k minimum liquidity
        if token_data.metadata.liquidity_usd < min_liquidity_usd {
            return false;
        }

        // Check minimum volume
        let min_volume_usd = 50_000.0; // $50k minimum 24h volume
        if token_data.metadata.volume_24h < min_volume_usd {
            return false;
        }

        // Check if token is verified
        if !token_data.metadata.is_verified {
            return false;
        }

        // Check holder concentration
        let max_holder_concentration = 0.4; // Maximum 40% held by top holders
        if token_data.metadata.holder_concentration > max_holder_concentration {
            return false;
        }

        true
    }

    pub async fn monitor_positions(&self) -> Result<()> {
        let positions = self.strategy.get_portfolio_positions();
        
        for position in positions {
            // Get current market data
            let token_data = self.market_data.analyze_token(&position.token.address).await?;
            
            // Check stop loss
            if let Some(stop_loss) = position.stop_loss {
                if token_data.metadata.price_usd.unwrap_or_default() <= stop_loss {
                    warn!("Stop loss triggered for {}", position.token.symbol);
                    self.execute_exit(&position.token.address).await?;
                    continue;
                }
            }

            // Check take profit levels
            if let Some(take_profits) = &position.take_profit_levels {
                let current_price = token_data.metadata.price_usd.unwrap_or_default();
                for level in take_profits {
                    if current_price >= level.price && !level.triggered {
                        info!("Take profit triggered at ${} for {}", level.price, position.token.symbol);
                        self.execute_partial_exit(&position, level).await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn execute_exit(&self, token_address: &str) -> Result<()> {
        let position = self.strategy.get_position(token_address)?;
        let decision = TradingDecision {
            token_address: token_address.to_string(),
            action: TradeAction::Sell,
            size_in_sol: position.quantity,
            confidence: 1.0, // High confidence for risk management actions
            reasoning: "Stop loss triggered".to_string(),
            risk_score: 0.0,
            technical_signals: TechnicalSignals::default(),
            market_context: MarketContext::default(),
            execution_params: ExecutionParams {
                entry_type: "market".to_string(),
                time_horizon: "immediate".to_string(),
                stop_loss: 0.0,
                take_profit: vec![],
                max_slippage: self.execution_engine.max_slippage,
                dca_config: None,
            },
        };

        self.execution_engine.execute_trade(&decision).await?;
        Ok(())
    }

    async fn execute_partial_exit(&self, position: &PortfolioPosition, level: &TakeProfitLevel) -> Result<()> {
        let exit_size = position.quantity * level.size_percentage;
        let decision = TradingDecision {
            token_address: position.token.address.clone(),
            action: TradeAction::Sell,
            size_in_sol: exit_size,
            confidence: 1.0,
            reasoning: format!("Take profit triggered at ${}", level.price),
            risk_score: 0.0,
            technical_signals: TechnicalSignals::default(),
            market_context: MarketContext::default(),
            execution_params: ExecutionParams {
                entry_type: "market".to_string(),
                time_horizon: "immediate".to_string(),
                stop_loss: 0.0,
                take_profit: vec![],
                max_slippage: self.execution_engine.max_slippage,
                dca_config: None,
            },
        };

        self.execution_engine.execute_trade(&decision).await?;
        Ok(())
    }
}