use crate::market_data::EnhancedTokenMetadata;
use crate::strategy::{TradingDecision, ExecutionParams};
use anyhow::Result;
use std::collections::HashMap;
use chrono::{DateTime, Utc};
use tracing::{info, warn, error, debug};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{sleep};
use solana_sdk::pubkey::Pubkey;
use jup_ag::{
    Jupiter, QuoteRequest, SwapRequest, Side,
    TokenInfo, TokenRouteInfo,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub token_address: String,
    pub action: TradeAction,
    pub amount_sol: f64,
    pub price: f64,
    pub slippage: f64,
    pub tx_signature: Option<String>,
    pub execution_time: i64,
    pub execution_type: ExecutionType,
    pub order_status: OrderStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionType {
    Market,
    Limit,
    DCA,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderStatus {
    Completed,
    Partial,
    Failed,
}

pub struct ExecutionEngine {
    jupiter: Arc<Jupiter>,
    max_retries: u32,
    retry_delay: Duration,
    max_slippage: f64,
    wallet_pubkey: Pubkey,
}

impl ExecutionEngine {
    pub fn new(rpc_url: &str, max_slippage: f64, wallet_pubkey: Pubkey) -> Result<Self> {
        Ok(Self {
            jupiter: Arc::new(Jupiter::new(rpc_url)?),
            max_retries: 3,
            retry_delay: Duration::from_secs(1),
            max_slippage,
            wallet_pubkey,
        })
    }

    pub async fn execute_trade(&self, decision: &TradingDecision) -> Result<ExecutionResult> {
        debug!("Executing trade for token {}", decision.token_address);

        match decision.execution_params.entry_type.as_str() {
            "market" => self.execute_market_order(decision).await,
            "limit" => self.execute_limit_order(decision).await,
            "dca" if decision.execution_params.dca_config.is_some() => {
                self.execute_dca_strategy(decision).await
            }
            _ => {
                warn!("Unsupported execution type, falling back to market order");
                self.execute_market_order(decision).await
            }
        }
    }

    async fn execute_market_order(&self, decision: &TradingDecision) -> Result<ExecutionResult> {
        let mut attempts = 0;
        let start_time = chrono::Utc::now().timestamp();
        let token_pubkey = Pubkey::from_str(&decision.token_address)?;

        while attempts < self.max_retries {
            // Get quote
            let quote_request = QuoteRequest::builder()
                .input_mint(solana_sdk::native_token::NATIVE_MINT) // SOL
                .output_mint(token_pubkey)
                .amount(decision.size_in_sol)
                .slippage_bps((self.max_slippage * 10000.0) as u64)
                .build()?;

            let quote = self.jupiter.quote(quote_request).await?;

            // Check if slippage is acceptable
            let slippage = quote.price_impact_pct;
            if slippage > self.max_slippage {
                warn!("Slippage too high: {}%, max allowed: {}%", slippage, self.max_slippage);
                attempts += 1;
                sleep(self.retry_delay).await;
                continue;
            }

            // Execute swap
            let swap_request = SwapRequest::builder()
                .quote(quote)
                .wallet_address(self.wallet_pubkey)
                .build()?;

            match self.jupiter.swap(swap_request).await {
                Ok(result) => {
                    info!("Trade executed successfully: {}", result.signature);
                    return Ok(ExecutionResult {
                        token_address: decision.token_address.clone(),
                        action: decision.action.clone(),
                        amount_sol: decision.size_in_sol,
                        price: quote.price,
                        slippage,
                        tx_signature: Some(result.signature),
                        execution_time: chrono::Utc::now().timestamp() - start_time,
                        execution_type: ExecutionType::Market,
                        order_status: OrderStatus::Completed,
                    });
                }
                Err(e) => {
                    warn!("Trade execution failed: {}", e);
                    attempts += 1;
                    sleep(self.retry_delay).await;
                }
            }
        }

        Err(anyhow::anyhow!("Failed to execute trade after {} attempts", self.max_retries))
    }

    async fn execute_limit_order(&self, decision: &TradingDecision) -> Result<ExecutionResult> {
        let start_time = chrono::Utc::now().timestamp();
        let token_pubkey = Pubkey::from_str(&decision.token_address)?;

        // Get market quote for reference price
        let quote_request = QuoteRequest::builder()
            .input_mint(solana_sdk::native_token::NATIVE_MINT)
            .output_mint(token_pubkey)
            .amount(decision.size_in_sol)
            .slippage_bps((self.max_slippage * 10000.0) as u64)
            .build()?;

        let quote = self.jupiter.quote(quote_request).await?;

        let limit_price = match decision.action {
            TradeAction::Buy => quote.price * (1.0 - decision.execution_params.max_slippage),
            TradeAction::Sell => quote.price * (1.0 + decision.execution_params.max_slippage),
            _ => return Err(anyhow::anyhow!("Invalid trade action for limit order")),
        };

        // Note: Jupiter doesn't support limit orders directly
        // This would need to be implemented using a DEX that supports limit orders
        // For now, return a partial result indicating the limit order request

        Ok(ExecutionResult {
            token_address: decision.token_address.clone(),
            action: decision.action.clone(),
            amount_sol: decision.size_in_sol,
            price: limit_price,
            slippage: 0.0,
            tx_signature: None,
            execution_time: chrono::Utc::now().timestamp() - start_time,
            execution_type: ExecutionType::Limit,
            order_status: OrderStatus::Partial,
        })
    }

    async fn execute_dca_strategy(&self, decision: &TradingDecision) -> Result<ExecutionResult> {
        let dca_config = decision.execution_params.dca_config.as_ref()
            .ok_or_else(|| anyhow::anyhow!("DCA config not provided"))?;

        let entry_size = decision.size_in_sol / dca_config.num_entries as f64;
        let mut total_executed = 0.0;
        let mut total_cost = 0.0;
        let start_time = chrono::Utc::now().timestamp();

        for i in 0..dca_config.num_entries {
            // Execute individual DCA order
            match self.execute_market_order(&TradingDecision {
                size_in_sol: entry_size,
                ..decision.clone()
            }).await {
                Ok(result) => {
                    total_executed += entry_size;
                    total_cost += entry_size * result.price;
                    info!("DCA order {}/{} executed", i + 1, dca_config.num_entries);
                }
                Err(e) => {
                    warn!("DCA order {}/{} failed: {}", i + 1, dca_config.num_entries, e);
                }
            }

            if i < dca_config.num_entries - 1 {
                sleep(Duration::from_secs(dca_config.time_between_entries as u64 * 3600)).await;
            }
        }

        let avg_price = if total_executed > 0.0 {
            total_cost / total_executed
        } else {
            0.0
        };

        Ok(ExecutionResult {
            token_address: decision.token_address.clone(),
            action: decision.action.clone(),
            amount_sol: total_executed,
            price: avg_price,
            slippage: 0.0,
            tx_signature: None,
            execution_time: chrono::Utc::now().timestamp() - start_time,
            execution_type: ExecutionType::DCA,
            order_status: if total_executed >= decision.size_in_sol * 0.9 {
                OrderStatus::Completed
            } else if total_executed > 0.0 {
                OrderStatus::Partial
            } else {
                OrderStatus::Failed
            },
        })
    }

    pub fn set_max_retries(&mut self, retries: u32) {
        self.max_retries = retries;
    }

    pub fn set_retry_delay(&mut self, delay: Duration) {
        self.retry_delay = delay;
    }
}