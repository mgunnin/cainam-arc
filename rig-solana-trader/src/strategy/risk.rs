use crate::market_data::EnhancedTokenMetadata;
use crate::strategy::{TechnicalSignals, MarketContext, StrategyConfig};
use anyhow::Result;
use rig_solana_trader::personality::StoicPersonality;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub risk_score: f64,
    pub max_position_size: f64,
    pub stop_loss_price: Option<f64>,
    pub risk_factors: Vec<String>,
    pub volatility_rating: VolatilityRating,
    pub market_risk_level: MarketRiskLevel,
    pub concentration_risk: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VolatilityRating {
    VeryLow,
    Low,
    Medium,
    High,
    VeryHigh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketRiskLevel {
    Low,
    Moderate,
    High,
    Extreme,
}

#[derive(Debug)]
pub struct RiskManager {
    config: StrategyConfig,
    max_position_per_token: f64,
    max_drawdown: f64,
    min_liquidity_ratio: f64,
    personality: StoicPersonality,
    portfolio_value: f64,
    position_limits: HashMap<String, f64>,
    market_exposure: f64,
}

impl RiskManager {
    pub fn new(config: StrategyConfig, personality: StoicPersonality) -> Self {
        Self {
            config,
            max_position_per_token: 0.2, // 20% of portfolio per token
            max_drawdown: 0.2,
            min_liquidity_ratio: 0.1, // Minimum liquidity to market cap ratio
            personality,
            portfolio_value: 0.0,
            position_limits: HashMap::new(),
            market_exposure: 0.0,
        }
    }

    pub async fn assess_risk(
        &self,
        token: &EnhancedTokenMetadata,
        technical: &TechnicalSignals,
        market: &MarketContext,
    ) -> Result<RiskAssessment> {
        debug!("Assessing risk for token {}", token.symbol);

        // Calculate volatility risk (0-1)
        let volatility_risk = self.calculate_volatility_risk(technical);
        
        // Calculate liquidity risk (0-1)
        let liquidity_risk = self.calculate_liquidity_risk(token, market);
        
        // Calculate market risk (0-1)
        let market_risk = self.calculate_market_risk(market);
        
        // Calculate concentration risk (0-1)
        let concentration_risk = self.calculate_concentration_risk(&token.address);

        // Identify specific risk factors
        let mut risk_factors = Vec::new();
        if volatility_risk > 0.7 {
            risk_factors.push("High volatility".to_string());
        }
        if liquidity_risk > 0.7 {
            risk_factors.push("Low liquidity".to_string());
        }
        if market_risk > 0.7 {
            risk_factors.push("High market risk".to_string());
        }
        if concentration_risk > 0.7 {
            risk_factors.push("High concentration risk".to_string());
        }

        // Calculate overall risk score (weighted average)
        let risk_score = volatility_risk * 0.3 + 
                        liquidity_risk * 0.3 + 
                        market_risk * 0.2 + 
                        concentration_risk * 0.2;

        // Determine maximum position size based on risk
        let max_position_size = self.calculate_max_position_size(risk_score);

        // Calculate stop loss if technical levels are available
        let stop_loss_price = self.calculate_stop_loss(token, technical);

        // Determine volatility rating
        let volatility_rating = match volatility_risk {
            x if x < 0.2 => VolatilityRating::VeryLow,
            x if x < 0.4 => VolatilityRating::Low,
            x if x < 0.6 => VolatilityRating::Medium,
            x if x < 0.8 => VolatilityRating::High,
            _ => VolatilityRating::VeryHigh,
        };

        // Determine market risk level
        let market_risk_level = match market_risk {
            x if x < 0.3 => MarketRiskLevel::Low,
            x if x < 0.6 => MarketRiskLevel::Moderate,
            x if x < 0.8 => MarketRiskLevel::High,
            _ => MarketRiskLevel::Extreme,
        };

        Ok(RiskAssessment {
            risk_score,
            max_position_size,
            stop_loss_price,
            risk_factors,
            volatility_rating,
            market_risk_level,
            concentration_risk,
        })
    }

    fn calculate_volatility_risk(&self, signals: &TechnicalSignals) -> f64 {
        let volatility_score = signals.volatility_score;
        let trend_strength = signals.trend_strength;
        
        // Higher trend strength can offset some volatility risk
        let adjusted_risk = volatility_score * (1.0 - trend_strength * 0.3);
        adjusted_risk.min(1.0).max(0.0)
    }

    fn calculate_liquidity_risk(&self, token: &EnhancedTokenMetadata, context: &MarketContext) -> f64 {
        let liquidity_score = context.liquidity_score;
        let min_acceptable_liquidity = 0.3;
        
        if liquidity_score < min_acceptable_liquidity {
            1.0
        } else {
            ((1.0 - liquidity_score) * 1.5).min(1.0).max(0.0)
        }
    }

    fn calculate_market_risk(&self, context: &MarketContext) -> f64 {
        let base_risk = match context.market_trend.as_str() {
            "strong_uptrend" => 0.2,
            "uptrend" => 0.3,
            "sideways" => 0.5,
            "downtrend" => 0.7,
            "strong_downtrend" => 0.8,
            _ => 0.5,
        };

        // Adjust based on sector performance
        let sector_adjustment = (1.0 - context.sector_performance) * 0.2;
        (base_risk + sector_adjustment).min(1.0).max(0.0)
    }

    fn calculate_concentration_risk(&self, token_address: &str) -> f64 {
        let current_exposure = self.position_limits.get(token_address).unwrap_or(&0.0);
        let max_concentration = 0.2; // Maximum 20% in single asset
        
        (current_exposure / (self.portfolio_value * max_concentration))
            .min(1.0)
            .max(0.0)
    }

    fn calculate_max_position_size(&self, risk_score: f64) -> f64 {
        let base_size = self.portfolio_value * 0.1; // Base 10% of portfolio
        let risk_adjustment = 1.0 - risk_score;
        
        base_size * risk_adjustment * (1.0 - self.market_exposure)
    }

    fn calculate_stop_loss(
        &self,
        token: &EnhancedTokenMetadata,
        signals: &TechnicalSignals,
    ) -> Option<f64> {
        // Use nearest support level as stop loss
        if let Some(support) = signals.support_levels.first() {
            let current_price = token.price_usd.unwrap_or_default();
            let stop_distance = (current_price - support) / current_price;
            
            // Only set stop loss if distance is reasonable (not too tight or wide)
            if stop_distance > 0.02 && stop_distance < 0.15 {
                Some(*support)
            } else {
                // Fallback to ATR-based stop loss
                let atr = signals.volatility_score * current_price;
                Some(current_price - (2.0 * atr))
            }
        } else {
            None
        }
    }

    pub fn update_portfolio_exposure(&mut self, token_address: &str, amount: f64) {
        *self.position_limits.entry(token_address.to_string()).or_insert(0.0) += amount;
        self.recalculate_market_exposure();
    }

    fn recalculate_market_exposure(&mut self) {
        self.market_exposure = self.position_limits.values().sum::<f64>() / self.portfolio_value;
    }

    pub fn validate_position_size(&self, size_in_sol: f64, current_portfolio_value: f64) -> bool {
        // Check if position size is within limits
        if size_in_sol < self.config.min_position_sol || size_in_sol > self.config.max_position_sol {
            return false;
        }

        // Check position size relative to portfolio
        let position_ratio = size_in_sol / current_portfolio_value;
        if position_ratio > self.max_position_per_token {
            return false;
        }

        true
    }

    pub fn validate_trade(&self, action: &TradeAction) -> Result<()> {
        let risk_score = self.calculate_risk_score(action);
        
        if risk_score > self.personality.risk_tolerance {
            return Err(anyhow::anyhow!(
                "Risk score {} exceeds tolerance {}",
                risk_score,
                self.personality.risk_tolerance
            ));
        }

        Ok(())
    }

    fn calculate_risk_score(&self, action: &TradeAction) -> f64 {
        let market_risk = action.analysis.as_ref().map(|a| a.risk_assessment).unwrap_or(1.0);
        let position_risk = action.params.amount / self.personality.max_position_size;
        
        market_risk * position_risk
    }
}