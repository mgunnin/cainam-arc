use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, debug};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub total_trades: u32,
    pub winning_trades: u32,
    pub losing_trades: u32,
    pub total_profit_loss: f64,
    pub win_rate: f64,
    pub average_return: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub current_drawdown: f64,
    pub risk_adjusted_return: f64,
    pub asset_performance: HashMap<String, AssetMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMetrics {
    pub symbol: String,
    pub total_trades: u32,
    pub profitable_trades: u32,
    pub total_profit_loss: f64,
    pub average_hold_time: f64,
    pub best_trade: f64,
    pub worst_trade: f64,
    pub win_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub token_address: String,
    pub entry_price: f64,
    pub exit_price: Option<f64>,
    pub size_sol: f64,
    pub entry_time: DateTime<Utc>,
    pub exit_time: Option<DateTime<Utc>>,
    pub profit_loss: Option<f64>,
    pub strategy_name: String,
    pub confidence_score: f64,
    pub execution_type: String,
}

pub struct PerformanceAnalyzer {
    trades: Vec<Trade>,
    metrics: PerformanceMetrics,
    initial_capital: f64,
    current_capital: f64,
    peak_capital: f64,
}

impl PerformanceAnalyzer {
    pub fn new(initial_capital: f64) -> Self {
        Self {
            trades: Vec::new(),
            metrics: PerformanceMetrics {
                total_trades: 0,
                winning_trades: 0,
                losing_trades: 0,
                total_profit_loss: 0.0,
                win_rate: 0.0,
                average_return: 0.0,
                sharpe_ratio: 0.0,
                max_drawdown: 0.0,
                current_drawdown: 0.0,
                risk_adjusted_return: 0.0,
                asset_performance: HashMap::new(),
            },
            initial_capital,
            current_capital: initial_capital,
            peak_capital: initial_capital,
        }
    }

    pub fn record_trade(&mut self, trade: Trade) {
        if let Some(pl) = trade.profit_loss {
            // Update capital
            self.current_capital += pl;
            self.peak_capital = self.peak_capital.max(self.current_capital);
            
            // Update metrics
            self.metrics.total_trades += 1;
            if pl > 0.0 {
                self.metrics.winning_trades += 1;
            } else {
                self.metrics.losing_trades += 1;
            }
            self.metrics.total_profit_loss += pl;

            // Update asset-specific metrics
            let asset_metrics = self.metrics.asset_performance
                .entry(trade.token_address.clone())
                .or_insert(AssetMetrics {
                    symbol: trade.token_address.clone(),
                    total_trades: 0,
                    profitable_trades: 0,
                    total_profit_loss: 0.0,
                    average_hold_time: 0.0,
                    best_trade: f64::NEG_INFINITY,
                    worst_trade: f64::INFINITY,
                    win_rate: 0.0,
                });

            asset_metrics.total_trades += 1;
            if pl > 0.0 {
                asset_metrics.profitable_trades += 1;
            }
            asset_metrics.total_profit_loss += pl;
            asset_metrics.best_trade = asset_metrics.best_trade.max(pl);
            asset_metrics.worst_trade = asset_metrics.worst_trade.min(pl);
            
            if let Some(exit_time) = trade.exit_time {
                let hold_time = exit_time.signed_duration_since(trade.entry_time).num_hours() as f64;
                asset_metrics.average_hold_time = (asset_metrics.average_hold_time * (asset_metrics.total_trades - 1) as f64 + hold_time) 
                    / asset_metrics.total_trades as f64;
            }

            asset_metrics.win_rate = asset_metrics.profitable_trades as f64 / asset_metrics.total_trades as f64;
        }

        // Store trade for historical analysis
        self.trades.push(trade);
        
        // Recalculate performance metrics
        self.update_metrics();
    }

    fn update_metrics(&mut self) {
        if self.metrics.total_trades == 0 {
            return;
        }

        // Calculate win rate
        self.metrics.win_rate = self.metrics.winning_trades as f64 / self.metrics.total_trades as f64;

        // Calculate average return
        self.metrics.average_return = self.metrics.total_profit_loss / self.metrics.total_trades as f64;

        // Calculate Sharpe ratio (assuming risk-free rate of 2%)
        let risk_free_rate = 0.02;
        let returns: Vec<f64> = self.trades.iter()
            .filter_map(|t| t.profit_loss)
            .collect();
        
        let mean_return = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns.iter()
            .map(|r| (r - mean_return).powi(2))
            .sum::<f64>() / returns.len() as f64;
        let std_dev = variance.sqrt();
        
        self.metrics.sharpe_ratio = if std_dev > 0.0 {
            (mean_return - risk_free_rate) / std_dev
        } else {
            0.0
        };

        // Calculate drawdown metrics
        let mut peak = self.initial_capital;
        let mut max_drawdown = 0.0;
        let mut current_value = self.initial_capital;

        for trade in &self.trades {
            if let Some(pl) = trade.profit_loss {
                current_value += pl;
                peak = peak.max(current_value);
                let drawdown = (peak - current_value) / peak;
                max_drawdown = max_drawdown.max(drawdown);
            }
        }

        self.metrics.max_drawdown = max_drawdown;
        self.metrics.current_drawdown = (self.peak_capital - self.current_capital) / self.peak_capital;

        // Calculate risk-adjusted return
        self.metrics.risk_adjusted_return = if self.metrics.max_drawdown > 0.0 {
            self.metrics.total_profit_loss / self.metrics.max_drawdown
        } else {
            self.metrics.total_profit_loss
        };
    }

    pub fn get_metrics(&self) -> &PerformanceMetrics {
        &self.metrics
    }

    pub fn analyze_strategy_performance(&self, strategy_name: &str) -> Result<StrategyAnalysis> {
        let strategy_trades: Vec<&Trade> = self.trades.iter()
            .filter(|t| t.strategy_name == strategy_name)
            .collect();

        if strategy_trades.is_empty() {
            return Ok(StrategyAnalysis::default());
        }

        let total_trades = strategy_trades.len();
        let profitable_trades = strategy_trades.iter()
            .filter(|t| t.profit_loss.unwrap_or(0.0) > 0.0)
            .count();
        
        let total_profit = strategy_trades.iter()
            .filter_map(|t| t.profit_loss)
            .sum::<f64>();

        let avg_confidence = strategy_trades.iter()
            .map(|t| t.confidence_score)
            .sum::<f64>() / total_trades as f64;

        Ok(StrategyAnalysis {
            strategy_name: strategy_name.to_string(),
            total_trades,
            win_rate: profitable_trades as f64 / total_trades as f64,
            total_profit,
            average_confidence: avg_confidence,
            recommended_adjustments: self.generate_strategy_recommendations(
                strategy_trades,
                profitable_trades,
                avg_confidence
            ),
        })
    }

    fn generate_strategy_recommendations(
        &self,
        trades: Vec<&Trade>,
        profitable_trades: usize,
        avg_confidence: f64,
    ) -> Vec<String> {
        let mut recommendations = Vec::new();

        // Analyze win rate
        let win_rate = profitable_trades as f64 / trades.len() as f64;
        if win_rate < 0.5 {
            recommendations.push(
                "Consider increasing minimum confidence threshold for trade execution".to_string()
            );
        }

        // Analyze confidence correlation
        let high_confidence_profits = trades.iter()
            .filter(|t| t.confidence_score > avg_confidence)
            .filter(|t| t.profit_loss.unwrap_or(0.0) > 0.0)
            .count();
        
        let high_confidence_total = trades.iter()
            .filter(|t| t.confidence_score > avg_confidence)
            .count();

        if high_confidence_total > 0 {
            let high_confidence_win_rate = high_confidence_profits as f64 / high_confidence_total as f64;
            if high_confidence_win_rate > win_rate {
                recommendations.push(
                    "Strategy performs better with higher confidence trades. Consider raising confidence threshold".to_string()
                );
            }
        }

        // Analyze execution types
        let market_orders: Vec<_> = trades.iter()
            .filter(|t| t.execution_type == "market")
            .collect();
        
        let limit_orders: Vec<_> = trades.iter()
            .filter(|t| t.execution_type == "limit")
            .collect();

        if !market_orders.is_empty() && !limit_orders.is_empty() {
            let market_win_rate = market_orders.iter()
                .filter(|t| t.profit_loss.unwrap_or(0.0) > 0.0)
                .count() as f64 / market_orders.len() as f64;

            let limit_win_rate = limit_orders.iter()
                .filter(|t| t.profit_loss.unwrap_or(0.0) > 0.0)
                .count() as f64 / limit_orders.len() as f64;

            if limit_win_rate > market_win_rate {
                recommendations.push(
                    "Limit orders show better performance. Consider increasing limit order usage".to_string()
                );
            }
        }

        recommendations
    }
}

#[derive(Debug, Default)]
pub struct StrategyAnalysis {
    pub strategy_name: String,
    pub total_trades: usize,
    pub win_rate: f64,
    pub total_profit: f64,
    pub average_confidence: f64,
    pub recommended_adjustments: Vec<String>,
}