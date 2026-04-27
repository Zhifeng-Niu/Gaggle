//! Discovery 存储层

use crate::discovery::types::*;
use crate::error::GaggleError;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

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

        // 创建 FTS5 独立虚拟表（不使用 content= 模式）
        // content= 模式要求内容表拥有 FTS5 中声明的所有列作为真实列，
        // 但 category 来自 JSON 字段 json_extract(capabilities, '$.category')，
        // 因此使用独立模式，由触发器显式同步数据。
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS provider_discovery_fts USING fts5(
                agent_id, display_name, description, skills, category
            )",
            [],
        )?;

        // INSERT 触发器：同步到 FTS5 表
        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS provider_discovery_fts_insert AFTER INSERT ON provider_discovery BEGIN
                INSERT INTO provider_discovery_fts(rowid, agent_id, display_name, description, skills, category)
                VALUES (new.rowid, new.agent_id, new.display_name, new.description, new.skills,
                        json_extract(new.capabilities, '$.category'));
            END",
            [],
        )?;

        // DELETE 触发器：从 FTS5 表删除
        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS provider_discovery_fts_delete AFTER DELETE ON provider_discovery BEGIN
                INSERT INTO provider_discovery_fts(provider_discovery_fts, rowid, agent_id, display_name, description, skills, category)
                VALUES ('delete', old.rowid, old.agent_id, old.display_name, old.description, old.skills,
                        json_extract(old.capabilities, '$.category'));
            END",
            [],
        )?;

        // UPDATE 触发器：先删除再插入（FTS5 不支持直接更新）
        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS provider_discovery_fts_update AFTER UPDATE ON provider_discovery BEGIN
                INSERT INTO provider_discovery_fts(provider_discovery_fts, rowid, agent_id, display_name, description, skills, category)
                VALUES ('delete', old.rowid, old.agent_id, old.display_name, old.description, old.skills,
                        json_extract(old.capabilities, '$.category'));
                INSERT INTO provider_discovery_fts(rowid, agent_id, display_name, description, skills, category)
                VALUES (new.rowid, new.agent_id, new.display_name, new.description, new.skills,
                        json_extract(new.capabilities, '$.category'));
            END",
            [],
        )?;

        // ── Need 广播表 ──────────────────────────────────
        conn.execute(
            "CREATE TABLE IF NOT EXISTS needs (
                id TEXT PRIMARY KEY,
                creator_id TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                category TEXT NOT NULL,
                required_skills TEXT NOT NULL DEFAULT '[]',
                budget_min REAL,
                budget_max REAL,
                deadline INTEGER,
                status TEXT NOT NULL DEFAULT 'open',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                matched_provider_count INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_needs_creator ON needs(creator_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_needs_status ON needs(status)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_needs_category ON needs(category)",
            [],
        )?;

        // Need FTS5 独立虚拟表（仅 category 做索引，不用 required_skills 避免 JSON 特殊字符问题）
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS needs_fts USING fts5(
                title, description, category
            )",
            [],
        )?;

        // Need FTS5 INSERT 触发器
        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS needs_fts_insert AFTER INSERT ON needs BEGIN
                INSERT INTO needs_fts(rowid, title, description, category)
                VALUES (new.rowid, new.title, COALESCE(new.description, ''), new.category);
            END",
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

    /// 搜索 Provider（FTS5 全文搜索 + 价格/可用性过滤）
    pub async fn search_providers(
        &self,
        query: &ProviderSearchQuery,
    ) -> Result<Vec<DiscoveryProfile>, GaggleError> {
        // 判断是否需要使用 FTS5 全文搜索
        let needs_fts = query.skills.is_some()
            || query.category.is_some()
            || query.query.as_ref().map(|q| !q.is_empty()).unwrap_or(false);

        let profiles = if needs_fts {
            // 构建 FTS5 MATCH 查询条件
            let mut match_conditions = Vec::new();

            // 技能搜索（OR 逻辑）
            if let Some(skills_filter) = &query.skills {
                let skills: Vec<&str> = skills_filter
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !skills.is_empty() {
                    let skills_query = skills.join(" OR ");
                    match_conditions.push(format!("skills:{}", escape_fts5(&skills_query)));
                }
            }

            // 分类搜索
            if let Some(category) = &query.category {
                match_conditions.push(format!("category:{}", escape_fts5(category)));
            }

            // 通用文本搜索
            if let Some(q) = &query.query {
                if !q.is_empty() {
                    match_conditions.push(format!("{}", escape_fts5(q)));
                }
            }

            let match_query = match_conditions.join(" OR ");

            // 执行 FTS5 查询
            let profiles = {
                let db = self.db.lock().await;
                let mut stmt = db.prepare(&format!(
                    "SELECT {PROFILE_COLUMNS} FROM provider_discovery
                     WHERE rowid IN (SELECT rowid FROM provider_discovery_fts WHERE provider_discovery_fts MATCH ?1)
                     ORDER BY updated_at DESC"
                ))?;

                let result = stmt
                    .query_map(params![match_query], map_profile)?
                    .collect::<Result<Vec<_>, _>>()?;
                drop(stmt); // 显式 drop stmt 以释放借用
                result
            };
            profiles
        } else {
            // 无 FTS5 条件，直接列出所有（带分页）
            self.list_all().await?
        };

        // 价格范围和可用状态过滤（在内存中执行，因为不是 FTS5 支持的）
        let filtered: Vec<DiscoveryProfile> = profiles
            .into_iter()
            .filter(|profile| {
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

    // ── Need 广播方法 ──────────────────────────────────

    /// 发布需求
    pub async fn publish_need(
        &self,
        creator_id: &str,
        req: PublishNeedRequest,
    ) -> Result<Need, GaggleError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let skills_json = serde_json::to_string(&req.required_skills)?;

        let db = self.db.lock().await;
        db.execute(
            "INSERT INTO needs (id, creator_id, title, description, category, required_skills,
                budget_min, budget_max, deadline, status, created_at, updated_at, matched_provider_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'open', ?10, ?10, 0)",
            params![
                id, creator_id, req.title, req.description, req.category, skills_json,
                req.budget_min, req.budget_max, req.deadline, now,
            ],
        )?;
        drop(db);

        Ok(Need {
            id,
            creator_id: creator_id.to_string(),
            title: req.title,
            description: req.description,
            category: req.category,
            required_skills: req.required_skills,
            budget_min: req.budget_min,
            budget_max: req.budget_max,
            deadline: req.deadline,
            status: NeedStatus::Open,
            created_at: now,
            updated_at: now,
            matched_provider_count: 0,
        })
    }

    /// 获取需求
    pub async fn get_need(&self, need_id: &str) -> Result<Option<Need>, GaggleError> {
        let db = self.db.lock().await;
        let need = db
            .query_row(
                "SELECT id, creator_id, title, description, category, required_skills,
                        budget_min, budget_max, deadline, status, created_at, updated_at, matched_provider_count
                 FROM needs WHERE id = ?1",
                params![need_id],
                map_need,
            )
            .optional()?;
        Ok(need)
    }

    /// 搜索开放需求
    pub async fn search_needs(
        &self,
        query: &NeedSearchQuery,
    ) -> Result<PaginatedResult<Need>, GaggleError> {
        let page = query.page.unwrap_or(1).max(1);
        let page_size = query.page_size.unwrap_or(20).min(100).max(1);

        let needs_fts = query.category.is_some()
            || query.skills.is_some()
            || query.query.as_ref().map(|q| !q.is_empty()).unwrap_or(false);

        let (needs, total) = if needs_fts {
            let mut match_conditions = Vec::new();
            if let Some(category) = &query.category {
                match_conditions.push(format!("category:{}", escape_fts5(category)));
            }
            if let Some(skills) = &query.skills {
                let skills_list: Vec<&str> = skills.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
                if !skills_list.is_empty() {
                    // skills 作为通用文本搜索（FTS5 表没有 required_skills 列）
                    match_conditions.push(escape_fts5(&skills_list.join(" OR ")));
                }
            }
            if let Some(q) = &query.query {
                if !q.is_empty() {
                    match_conditions.push(escape_fts5(q));
                }
            }
            let match_query = match_conditions.join(" OR ");

            let db = self.db.lock().await;
            let count: u32 = db.query_row(
                "SELECT COUNT(*) FROM needs WHERE status = 'open'
                 AND rowid IN (SELECT rowid FROM needs_fts WHERE needs_fts MATCH ?1)",
                params![match_query],
                |r| r.get(0),
            )?;

            let offset = (page - 1) * page_size;
            let mut stmt = db.prepare(
                "SELECT id, creator_id, title, description, category, required_skills,
                        budget_min, budget_max, deadline, status, created_at, updated_at, matched_provider_count
                 FROM needs WHERE status = 'open'
                 AND rowid IN (SELECT rowid FROM needs_fts WHERE needs_fts MATCH ?1)
                 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
            )?;
            let needs = stmt.query_map(params![match_query, page_size, offset], map_need)?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);
            drop(db);
            (needs, count)
        } else {
            let db = self.db.lock().await;
            let status_filter = query.status.as_deref().unwrap_or("open");
            let count: u32 = db.query_row(
                "SELECT COUNT(*) FROM needs WHERE status = ?1",
                params![status_filter],
                |r| r.get(0),
            )?;
            drop(db);

            let offset = (page - 1) * page_size;
            let db = self.db.lock().await;
            let mut stmt = db.prepare(
                "SELECT id, creator_id, title, description, category, required_skills,
                        budget_min, budget_max, deadline, status, created_at, updated_at, matched_provider_count
                 FROM needs WHERE status = ?1
                 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
            )?;
            let needs = stmt.query_map(params![status_filter, page_size, offset], map_need)?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);
            drop(db);
            (needs, count)
        };

        let total_pages = (total + page_size - 1) / page_size;
        Ok(PaginatedResult {
            items: needs,
            total,
            page,
            page_size,
            total_pages,
        })
    }

    /// 获取 Agent 发布的需求
    pub async fn get_my_needs(&self, creator_id: &str) -> Result<Vec<Need>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, creator_id, title, description, category, required_skills,
                    budget_min, budget_max, deadline, status, created_at, updated_at, matched_provider_count
             FROM needs WHERE creator_id = ?1 ORDER BY created_at DESC",
        )?;
        let needs = stmt.query_map(params![creator_id], map_need)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(needs)
    }

    /// 更新需求状态
    pub async fn update_need_status(
        &self,
        need_id: &str,
        status: &NeedStatus,
    ) -> Result<bool, GaggleError> {
        let now = Utc::now().timestamp_millis();
        let db = self.db.lock().await;
        let rows = db.execute(
            "UPDATE needs SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.as_str(), now, need_id],
        )?;
        Ok(rows > 0)
    }

    /// 更新需求匹配 Provider 数量
    pub async fn update_matched_count(
        &self,
        need_id: &str,
        count: i32,
    ) -> Result<(), GaggleError> {
        let db = self.db.lock().await;
        db.execute(
            "UPDATE needs SET matched_provider_count = ?1 WHERE id = ?2",
            params![count, need_id],
        )?;
        Ok(())
    }

    /// 根据 Need 匹配 Provider
    /// 匹配策略: category 精确匹配 + skills 加权 + 价格范围 + 可用状态
    pub async fn find_matching_providers(
        &self,
        need: &Need,
    ) -> Result<Vec<DiscoveryProfile>, GaggleError> {
        // 构建匹配查询: category 必须匹配
        let mut match_conditions = vec![format!("category:{}", escape_fts5(&need.category))];

        // skills 加权匹配
        if !need.required_skills.is_empty() {
            let skills_query = need.required_skills.iter()
                .map(|s| escape_fts5(s))
                .collect::<Vec<_>>()
                .join(" OR ");
            match_conditions.push(format!("skills:({})", skills_query));
        }

        let match_query = match_conditions.join(" AND ");

        let db = self.db.lock().await;
        let mut stmt = db.prepare(&format!(
            "SELECT {PROFILE_COLUMNS} FROM provider_discovery
             WHERE rowid IN (SELECT rowid FROM provider_discovery_fts WHERE provider_discovery_fts MATCH ?1)
             ORDER BY updated_at DESC"
        ))?;

        let profiles = stmt.query_map(params![match_query], map_profile)?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(db);

        // 价格范围过滤
        let filtered: Vec<DiscoveryProfile> = profiles.into_iter().filter(|p| {
            if let Some(budget_min) = need.budget_min {
                if let Some(max_price) = p.max_price {
                    if max_price < budget_min { return false; }
                }
            }
            if let Some(budget_max) = need.budget_max {
                if let Some(min_price) = p.min_price {
                    if min_price > budget_max { return false; }
                }
            }
            true
        }).collect();

        Ok(filtered)
    }
}

/// Need 行映射
fn map_need(row: &Row<'_>) -> Result<Need, rusqlite::Error> {
    let skills_str: String = row.get(5)?;
    let status_str: String = row.get(9)?;
    let status = match status_str.as_str() {
        "matched" => NeedStatus::Matched,
        "expired" => NeedStatus::Expired,
        "cancelled" => NeedStatus::Cancelled,
        _ => NeedStatus::Open,
    };

    Ok(Need {
        id: row.get(0)?,
        creator_id: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        category: row.get(4)?,
        required_skills: serde_json::from_str(&skills_str).unwrap_or_default(),
        budget_min: row.get(6)?,
        budget_max: row.get(7)?,
        deadline: row.get(8)?,
        status,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        matched_provider_count: row.get(12)?,
    })
}

/// FTS5 特殊字符转义
fn escape_fts5(s: &str) -> String {
    // FTS5 特殊字符: - " ( )
    s.chars()
        .map(|c| match c {
            '-' | '"' | '(' | ')' => format!("\\{}", c),
            _ => c.to_string(),
        })
        .collect::<Vec<_>>()
        .join("")
}
