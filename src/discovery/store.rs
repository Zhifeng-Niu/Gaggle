//! Discovery 存储层

use crate::discovery::types::*;
use crate::error::GaggleError;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Discovery 行列常量
const PROFILE_COLUMNS: &str = "agent_id, display_name, description, skills, capabilities,
    pricing_model, availability_status, min_price, max_price, updated_at";

fn map_profile(row: &Row<'_>) -> Result<DiscoveryProfile, rusqlite::Error> {
    let skills_str: String = row.get(3)?;
    let capabilities_str: String = row.get(4)?;
    let pricing_model_str: String = row.get(5)?;
    let availability_str: String = row.get(6)?;

    Ok(DiscoveryProfile {
        agent_id: row.get(0)?,
        display_name: row.get(1)?,
        description: row.get(2)?,
        skills: serde_json::from_str(&skills_str).unwrap_or_default(),
        capabilities: serde_json::from_str(&capabilities_str).unwrap_or_else(|_| {
            ProviderCapabilities {
                category: "unknown".to_string(),
                tags: vec![],
            }
        }),
        pricing_model: serde_json::from_str(&pricing_model_str).unwrap_or_default(),
        availability_status: serde_json::from_str(&availability_str).unwrap_or_default(),
        min_price: row.get(7)?,
        max_price: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

/// Discovery Store
pub struct DiscoveryStore {
    db: Arc<Mutex<Connection>>,
}

impl DiscoveryStore {
    /// 创建新的 Discovery Store
    pub fn new(db_path: &str) -> Result<Self, GaggleError> {
        let conn = Connection::open(db_path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS provider_discovery (
                agent_id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                description TEXT,
                skills TEXT NOT NULL,
                capabilities TEXT NOT NULL,
                pricing_model TEXT NOT NULL,
                availability_status TEXT NOT NULL DEFAULT 'available',
                min_price REAL,
                max_price REAL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_discovery_skills ON provider_discovery(skills)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_discovery_category ON provider_discovery(capabilities)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_discovery_price ON provider_discovery(min_price, max_price)",
            [],
        )?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// 创建或更新 Provider Discovery Profile
    pub async fn upsert_profile(
        &self,
        agent_id: &str,
        req: UpdateProfileRequest,
    ) -> Result<DiscoveryProfile, GaggleError> {
        let updated_at = Utc::now().timestamp();

        let db = self.db.lock().await;
        db.execute(
            "INSERT INTO provider_discovery (
                agent_id, display_name, description, skills, capabilities,
                pricing_model, availability_status, min_price, max_price, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(agent_id) DO UPDATE SET
                display_name = excluded.display_name,
                description = excluded.description,
                skills = excluded.skills,
                capabilities = excluded.capabilities,
                pricing_model = excluded.pricing_model,
                availability_status = excluded.availability_status,
                min_price = excluded.min_price,
                max_price = excluded.max_price,
                updated_at = excluded.updated_at",
            params![
                agent_id,
                req.display_name,
                req.description,
                serde_json::to_string(&req.skills)?,
                serde_json::to_string(&req.capabilities)?,
                serde_json::to_string(&req.pricing_model)?,
                serde_json::to_string(&req.availability_status)?,
                req.min_price,
                req.max_price,
                updated_at,
            ],
        )?;

        Ok(DiscoveryProfile {
            agent_id: agent_id.to_string(),
            display_name: req.display_name,
            description: req.description,
            skills: req.skills,
            capabilities: req.capabilities,
            pricing_model: req.pricing_model,
            availability_status: req.availability_status,
            min_price: req.min_price,
            max_price: req.max_price,
            updated_at,
        })
    }

    /// 通过 agent_id 获取 Discovery Profile
    pub async fn get_profile(
        &self,
        agent_id: &str,
    ) -> Result<Option<DiscoveryProfile>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(&format!(
            "SELECT {PROFILE_COLUMNS} FROM provider_discovery WHERE agent_id = ?1"
        ))?;

        let profile = stmt.query_row(params![agent_id], map_profile).optional()?;

        Ok(profile)
    }

    /// 搜索 Provider（支持动态过滤）
    ///
    /// 对于 MVP 阶段，我们在内存中执行过滤以避免动态 SQL 参数的复杂性
    pub async fn search_providers(
        &self,
        query: &ProviderSearchQuery,
    ) -> Result<Vec<DiscoveryProfile>, GaggleError> {
        // 获取所有 profiles
        let all_profiles = self.list_all().await?;

        // 在内存中过滤
        let filtered: Vec<DiscoveryProfile> = all_profiles
            .into_iter()
            .filter(|profile| {
                // 技能过滤（逗号分隔，OR 逻辑）
                if let Some(skills_filter) = &query.skills {
                    let required_skills: Vec<&str> = skills_filter
                        .split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .collect();

                    if !required_skills.is_empty() {
                        let has_any_skill = required_skills
                            .iter()
                            .any(|skill| profile.skills.contains(&skill.to_string()));
                        if !has_any_skill {
                            return false;
                        }
                    }
                }

                // 分类过滤
                if let Some(category) = &query.category {
                    if profile.capabilities.category != *category {
                        return false;
                    }
                }

                // 价格范围过滤
                if let Some(min_price) = query.min_price {
                    if let Some(max_price) = profile.max_price {
                        if max_price < min_price {
                            return false;
                        }
                    }
                }
                if let Some(max_price) = query.max_price {
                    if let Some(min_price) = profile.min_price {
                        if min_price > max_price {
                            return false;
                        }
                    }
                }

                // 可用状态过滤
                if let Some(availability) = &query.availability {
                    let status_str =
                        serde_json::to_string(&profile.availability_status).unwrap_or_default();
                    if !status_str.contains(availability) {
                        return false;
                    }
                }

                true
            })
            .collect();

        Ok(filtered)
    }

    /// 列出所有 Provider Profiles
    pub async fn list_all(&self) -> Result<Vec<DiscoveryProfile>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(&format!(
            "SELECT {PROFILE_COLUMNS} FROM provider_discovery ORDER BY updated_at DESC"
        ))?;

        let profiles = stmt
            .query_map([], map_profile)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(profiles)
    }

    /// 删除 Discovery Profile
    pub async fn delete_profile(&self, agent_id: &str) -> Result<bool, GaggleError> {
        let db = self.db.lock().await;
        let rows_affected = db.execute(
            "DELETE FROM provider_discovery WHERE agent_id = ?1",
            params![agent_id],
        )?;

        Ok(rows_affected > 0)
    }
}
