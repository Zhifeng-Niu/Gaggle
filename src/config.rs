//! Gaggle配置

use serde::{Deserialize, Serialize};

/// Gaggle服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 服务器监听地址
    pub host: String,
    /// 服务器监听端口
    pub port: u16,
    /// Solana RPC URL
    pub solana_rpc_url: String,
    /// 数据库路径
    pub database_path: String,
    /// 服务器API密钥（用于服务间认证，可选）
    pub server_api_key: Option<String>,
    /// 速率限制：每分钟请求数
    pub rate_limit_rpm: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
            solana_rpc_url: "https://api.devnet.solana.com".to_string(),
            database_path: "gaggle.db".to_string(),
            server_api_key: None,
            rate_limit_rpm: 120,
        }
    }
}

impl Config {
    /// 从环境变量加载配置
    pub fn from_env() -> Self {
        Self {
            host: std::env::var("GAGGLE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("GAGGLE_PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .unwrap_or(8080),
            solana_rpc_url: std::env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| "https://api.devnet.solana.com".to_string()),
            database_path: std::env::var("GAGGLE_DATABASE_PATH")
                .unwrap_or_else(|_| "gaggle.db".to_string()),
            server_api_key: std::env::var("GAGGLE_SERVER_API_KEY").ok(),
            rate_limit_rpm: std::env::var("GAGGLE_RATE_LIMIT_RPM")
                .unwrap_or_else(|_| "120".to_string())
                .parse()
                .unwrap_or(120),
        }
    }

    /// 获取服务器地址
    pub fn server_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "0.0.0.0");
    }

    #[test]
    fn test_server_addr() {
        let config = Config {
            host: "127.0.0.1".to_string(),
            port: 3000,
            ..Default::default()
        };
        assert_eq!(config.server_addr(), "127.0.0.1:3000");
    }
}
