use anyhow::Result;
use serde::{Deserialize, Serialize};
use ta::{
    indicators::{RelativeStrengthIndex, ExponentialMovingAverage, MovingAverageConvergenceDivergence},
    Next,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalAnalysis {
    pub trend_direction: TrendDirection,
    pub trend_strength: f64,
    pub support_levels: Vec<f64>,
    pub resistance_levels: Vec<f64>,
    pub rsi_signal: RSISignal,
    pub macd_signal: MACDSignal,
    pub volatility_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrendDirection {
    Strong_Up,
    Up,
    Sideways,
    Down,
    Strong_Down,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RSISignal {
    Overbought,
    Bullish,
    Neutral,
    Bearish,
    Oversold,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MACDSignal {
    StrongBuy,
    Buy,
    Neutral,
    Sell,
    StrongSell,
}

pub struct TechnicalAnalyzer {
    rsi: RelativeStrengthIndex,
    fast_ema: ExponentialMovingAverage,
    slow_ema: ExponentialMovingAverage,
    macd: MovingAverageConvergenceDivergence,
    price_history: Vec<f64>,
}

impl TechnicalAnalyzer {
    pub fn new() -> Self {
        Self {
            rsi: RelativeStrengthIndex::new(14).unwrap(),
            fast_ema: ExponentialMovingAverage::new(12).unwrap(),
            slow_ema: ExponentialMovingAverage::new(26).unwrap(),
            macd: MovingAverageConvergenceDivergence::new(12, 26, 9).unwrap(),
            price_history: Vec::new(),
        }
    }

    pub fn analyze(&mut self, prices: &[f64]) -> Result<TechnicalAnalysis> {
        self.price_history = prices.to_vec();
        
        // Calculate indicators
        let rsi = self.calculate_rsi();
        let (macd_value, macd_signal) = self.calculate_macd();
        let trend = self.analyze_trend();
        let (support, resistance) = self.find_support_resistance();
        let volatility = self.calculate_volatility();

        Ok(TechnicalAnalysis {
            trend_direction: trend.0,
            trend_strength: trend.1,
            support_levels: support,
            resistance_levels: resistance,
            rsi_signal: self.interpret_rsi(rsi),
            macd_signal: self.interpret_macd(macd_value, macd_signal),
            volatility_score: volatility,
        })
    }

    fn calculate_rsi(&self) -> f64 {
        let mut rsi = self.rsi.clone();
        self.price_history.iter()
            .fold(0.0, |_, &price| rsi.next(price))
    }

    fn calculate_macd(&self) -> (f64, f64) {
        let mut macd = self.macd.clone();
        let (mut last_value, mut last_signal) = (0.0, 0.0);
        
        for &price in self.price_history.iter() {
            let (value, signal) = macd.next(price);
            last_value = value;
            last_signal = signal;
        }
        
        (last_value, last_signal)
    }

    fn analyze_trend(&self) -> (TrendDirection, f64) {
        let mut fast_ema = self.fast_ema.clone();
        let mut slow_ema = self.slow_ema.clone();
        
        // Calculate EMAs
        let fast_values: Vec<f64> = self.price_history.iter()
            .map(|&price| fast_ema.next(price))
            .collect();
        let slow_values: Vec<f64> = self.price_history.iter()
            .map(|&price| slow_ema.next(price))
            .collect();

        // Calculate trend strength and direction
        let last_fast = fast_values.last().unwrap();
        let last_slow = slow_values.last().unwrap();
        let diff_pct = (last_fast - last_slow) / last_slow;
        
        let direction = match diff_pct {
            x if x > 0.05 => TrendDirection::Strong_Up,
            x if x > 0.02 => TrendDirection::Up,
            x if x < -0.05 => TrendDirection::Strong_Down,
            x if x < -0.02 => TrendDirection::Down,
            _ => TrendDirection::Sideways,
        };
        
        let strength = diff_pct.abs().min(1.0);
        (direction, strength)
    }

    fn find_support_resistance(&self) -> (Vec<f64>, Vec<f64>) {
        let window_size = 20;
        let threshold = 0.02; // 2% price difference
        let mut support = Vec::new();
        let mut resistance = Vec::new();

        for i in window_size..self.price_history.len() - window_size {
            let current = self.price_history[i];
            let left_min = self.price_history[i-window_size..i].iter().fold(f64::INFINITY, |a, &b| a.min(b));
            let right_min = self.price_history[i+1..i+window_size].iter().fold(f64::INFINITY, |a, &b| a.min(b));
            let left_max = self.price_history[i-window_size..i].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
            let right_max = self.price_history[i+1..i+window_size].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

            // Support level
            if current < left_min && current < right_min {
                let price = current * (1.0 - threshold);
                if !support.contains(&price) {
                    support.push(price);
                }
            }

            // Resistance level
            if current > left_max && current > right_max {
                let price = current * (1.0 + threshold);
                if !resistance.contains(&price) {
                    resistance.push(price);
                }
            }
        }

        support.sort_by(|a, b| a.partial_cmp(b).unwrap());
        resistance.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        (support, resistance)
    }

    fn calculate_volatility(&self) -> f64 {
        if self.price_history.len() < 2 {
            return 0.0;
        }

        // Calculate returns
        let returns: Vec<f64> = self.price_history.windows(2)
            .map(|w| (w[1] - w[0]) / w[0])
            .collect();

        // Calculate standard deviation of returns
        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns.iter()
            .map(|r| (r - mean).powi(2))
            .sum::<f64>() / returns.len() as f64;
        
        variance.sqrt()
    }

    fn interpret_rsi(&self, rsi: f64) -> RSISignal {
        match rsi {
            x if x >= 70.0 => RSISignal::Overbought,
            x if x <= 30.0 => RSISignal::Oversold,
            x if x > 60.0 => RSISignal::Bullish,
            x if x < 40.0 => RSISignal::Bearish,
            _ => RSISignal::Neutral,
        }
    }

    fn interpret_macd(&self, value: f64, signal: f64) -> MACDSignal {
        let diff = value - signal;
        let threshold = 0.0002; // Adjust based on asset volatility

        match diff {
            x if x > threshold * 2.0 => MACDSignal::StrongBuy,
            x if x > threshold => MACDSignal::Buy,
            x if x < -threshold * 2.0 => MACDSignal::StrongSell,
            x if x < -threshold => MACDSignal::Sell,
            _ => MACDSignal::Neutral,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_technical_analysis() {
        let mut analyzer = TechnicalAnalyzer::new();
        let prices = vec![10.0, 11.0, 10.5, 11.5, 12.0, 11.8, 11.9, 12.1, 12.3, 12.2];
        let analysis = analyzer.analyze(&prices).unwrap();
        
        assert!(analysis.volatility_score > 0.0);
        assert!(!analysis.support_levels.is_empty());
        assert!(!analysis.resistance_levels.is_empty());
    }
}