use crate::config::AppConfig;
use crate::storage::storage_strategy::StrategyMetadata;
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use anyhow::Result;
use crate::error::TradingError;

pub struct TelegramAlerter {
    bot: Bot,
    chat_id: ChatId,
}

impl TelegramAlerter {
    pub fn new() -> Self {
        let config = AppConfig::load().unwrap();
        let bot = Bot::new(config.monitoring.telegram_bot_token);
        let chat_id = config.monitoring.telegram_chat_id;

        Self {
            bot,
            chat_id: ChatId(chat_id),
        }
    }


    /// Send a comprehensive strategy error alert to Telegram
    pub async fn send_strategy_error_alert(
        &self,
        strategy: &StrategyMetadata,
        error: &TradingError,
    ) -> Result<()> {
        let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        let error_msg = format!("{}", error);
        
        // Escape special characters for MarkdownV2
        let escaped_symbol = Self::escape_markdown(&strategy.token_symbol);
        let escaped_error = Self::escape_markdown(&error_msg);
        let escaped_time = Self::escape_markdown(&timestamp.to_string());
        
        let message = format!(
            "🚨 *Strategy Error Alert*\n\n\
            ⏰ *Time:* {}\n\
            🪙 *Token:* `{}`\n\
            💱 *Strategy:* {}\n\
            📊 *Status:* Failed\n\n\
            ❌ *Error Details:*\n\
            {}\n\n\
            *ACTION IS REQUIRED*\n\
            *PLEASE CLOSE THE POSITIONS MANUALLY AND REOPEN THE STRATEGY\\.*",
            escaped_time,
            escaped_symbol,
            Self::escape_markdown(&format!("{:?}", strategy.status)),
            escaped_error
        );
        
        self.send_message(&message).await
    }

    /// Escape special characters for Telegram MarkdownV2
    fn escape_markdown(text: &str) -> String {
        text.chars()
            .map(|c| match c {
                '_' | '*' | '[' | ']' | '(' | ')' | '~' | '`' | '>' | '#' | '+' | '-' | '=' | '|' | '{' | '}' | '.' | '!' => {
                    format!("\\{}", c)
                }
                _ => c.to_string(),
            })
            .collect()
    }

    pub async fn send_message(&self, message: &str) -> Result<()> {
        self.bot
            .send_message(self.chat_id, message)
            .parse_mode(ParseMode::MarkdownV2)
            .await?;
        Ok(())
    }

}
