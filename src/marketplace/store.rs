//! 市场价格信息中心数据存储层

use super::types::{MarketContribution, MarketPrice, SharePriceRequest};
use crate::error::GaggleError;
use chrono::Utc;
use rusqlite::{params, Connection};
use std::sync::Arc;
use tokio::sync::Mutex;

/// 市场存储
pub struct MarketplaceStore {
    db: Arc<Mutex<Connection>>,
}

impl MarketplaceStore {
    /// 创建新的 MarketplaceStore
    pub fn new(db_path: &str) -> Result<Self, GaggleError> {
        let conn = Connection::open(db_path)?;

        // 市场价格汇总表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS market_prices (
                id TEXT PRIMARY KEY,
                category TEXT NOT NULL,
                service_type TEXT NOT NULL,
                avg_price REAL NOT NULL,
                min_price REAL NOT NULL,
                max_price REAL NOT NULL,
                sample_count INTEGER NOT NULL DEFAULT 1,
                period TEXT NOT NULL DEFAULT 'all',
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;

        // 市场价格贡献表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS market_contributions (
                id TEXT PRIMARY KEY,
                contributor_id TEXT NOT NULL,
                category TEXT NOT NULL,
                service_type TEXT NOT NULL,
                price REAL NOT NULL,
                description TEXT,
                anonymous INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;

        // 索引
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_market_contributions_category
             ON market_contributions(category)",
            [],
        )?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// 从 Space 自动记录价格（用于聚合市场数据）
    pub async fn record_price_from_space(
        &self,
        space_id: &str,
        category: &str,
        price: f64,
    ) -> Result<(), GaggleError> {
        // 使用 space_id 作为 contributor_id 标记系统自动收集
        let contributor_id = format!("system:space:{}", space_id);
        let req = SharePriceRequest {
            category: category.to_string(),
            service_type: "negotiated".to_string(),
            price,
            description: Some(format!("Auto-collected from space {}", space_id)),
            anonymous: true,
        };
        self.share_price(&contributor_id, req).await?;
        Ok(())
    }

    /// 获取指定分类的市场价格
    pub async fn get_market_prices(
        &self,
        category: &str,
    ) -> Result<Vec<MarketPrice>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, category, service_type, avg_price, min_price, max_price,
                    sample_count, period, updated_at
             FROM market_prices
             WHERE category = ?1",
        )?;

        let prices = stmt.query_map(params![category], |row| {
            Ok(MarketPrice {
                id: row.get(0)?,
                category: row.get(1)?,
                service_type: row.get(2)?,
                avg_price: row.get(3)?,
                min_price: row.get(4)?,
                max_price: row.get(5)?,
                sample_count: row.get(6)?,
                period: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;

        let mut result = Vec::new();
        for price in prices {
            result.push(price?);
        }
        Ok(result)
    }

    /// 获取所有市场价格
    pub async fn get_all_market_prices(&self) -> Result<Vec<MarketPrice>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, category, service_type, avg_price, min_price, max_price,
                    sample_count, period, updated_at
             FROM market_prices",
        )?;

        let prices = stmt.query_map([], |row| {
            Ok(MarketPrice {
                id: row.get(0)?,
                category: row.get(1)?,
                service_type: row.get(2)?,
                avg_price: row.get(3)?,
                min_price: row.get(4)?,
                max_price: row.get(5)?,
                sample_count: row.get(6)?,
                period: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;

        let mut result = Vec::new();
        for price in prices {
            result.push(price?);
        }
        Ok(result)
    }

    /// 手动贡献价格
    pub async fn share_price(
        &self,
        contributor_id: &str,
        req: SharePriceRequest,
    ) -> Result<MarketContribution, GaggleError> {
        let contribution_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        let db = self.db.lock().await;

        // 1. 插入贡献记录
        db.execute(
            "INSERT INTO market_contributions (id, contributor_id, category, service_type, price, description, anonymous, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                contribution_id,
                contributor_id,
                req.category,
                req.service_type,
                req.price,
                req.description,
                req.anonymous as i32,
                now,
            ],
        )?;

        // 2. 重新聚合更新 market_prices
        self.upsert_market_price(&db, &req.category, &req.service_type, &req.price)?;

        Ok(MarketContribution {
            id: contribution_id,
            contributor_id: contributor_id.to_string(),
            category: req.category,
            service_type: req.service_type,
            price: req.price,
            description: req.description,
            anonymous: req.anonymous,
            created_at: now,
        })
    }

    /// 获取最近的贡献记录
    pub async fn get_recent_contributions(
        &self,
        category: &str,
        limit: usize,
    ) -> Result<Vec<MarketContribution>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, contributor_id, category, service_type, price, description, anonymous, created_at
             FROM market_contributions
             WHERE category = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )?;

        let contributions = stmt.query_map(params![category, limit as i32], |row| {
            Ok(MarketContribution {
                id: row.get(0)?,
                contributor_id: row.get(1)?,
                category: row.get(2)?,
                service_type: row.get(3)?,
                price: row.get(4)?,
                description: row.get(5)?,
                anonymous: row.get::<_, i32>(6)? != 0,
                created_at: row.get(7)?,
            })
        })?;

        let mut result = Vec::new();
        for contrib in contributions {
            result.push(contrib?);
        }
        Ok(result)
    }

    /// 内部方法：聚合并更新市场价格的统计信息
    fn upsert_market_price(
        &self,
        db: &Connection,
        category: &str,
        service_type: &str,
        new_price: &f64,
    ) -> Result<(), GaggleError> {
        // 查询现有统计数据
        let (avg, min, max, mut count) = db.query_row(
            "SELECT avg_price, min_price, max_price, sample_count
             FROM market_prices
             WHERE category = ?1 AND service_type = ?2 AND period = 'all'",
            params![category, service_type],
            |row| {
                Ok((
                    row.get::<_, f64>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, i32>(3)?,
                ))
            },
        )
        .unwrap_or((0.0, *new_price, *new_price, 0));

        // 更新统计（递推公式）
        count += 1;
        let new_avg = (avg * (count - 1) as f64 + new_price) / count as f64;
        let new_min = min.min(*new_price);
        let new_max = max.max(*new_price);

        let now = Utc::now().timestamp();
        let price_id = format!("{}_{}_all", category, service_type);

        // Upsert
        db.execute(
            "INSERT INTO market_prices (id, category, service_type, avg_price, min_price, max_price, sample_count, period, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'all', ?8)
             ON CONFLICT(id) DO UPDATE SET
                avg_price = ?4,
                min_price = ?5,
                max_price = ?6,
                sample_count = ?7,
                updated_at = ?8",
            params![
                price_id,
                category,
                service_type,
                new_avg,
                new_min,
                new_max,
                count,
                now,
            ],
        )?;

        Ok(())
    }
}
