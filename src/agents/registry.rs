//! Agent注册表

use crate::agents::types::{
    Agent, AgentType, PricingModel, ProviderProfile, RegisterRequest, RegisterResponse,
    UpdateAgentRequest,
};
use crate::error::GaggleError;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Agent 行列常量
const AGENT_COLUMNS: &str =
    "id, agent_type, name, api_key, api_secret_hash, public_key, metadata, created_at, user_id, disabled_at, organization, callback_url";

fn map_agent(row: &Row<'_>) -> Result<Agent, rusqlite::Error> {
    let agent_type_str: String = row.get(1)?;
    let metadata_str: String = row.get(6)?;
    Ok(Agent {
        id: row.get(0)?,
        agent_type: serde_json::from_str(&agent_type_str).unwrap_or(AgentType::Consumer),
        name: row.get(2)?,
        api_key: row.get(3)?,
        api_secret_hash: row.get(4)?,
        public_key: row.get(5)?,
        metadata: serde_json::from_str(&metadata_str).unwrap_or(serde_json::json!({})),
        created_at: row.get(7)?,
        user_id: row.get(8)?,
        disabled_at: row.get(9)?,
        organization: row.get(10)?,
        callback_url: row.get(11)?,
    })
}

/// Agent注册表
pub struct AgentRegistry {
    db: Arc<Mutex<Connection>>,
}

impl AgentRegistry {
    /// 创建新的注册表
    pub fn new(db_path: &str) -> Result<Self, GaggleError> {
        let conn = Connection::open(db_path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                agent_type TEXT NOT NULL,
                name TEXT NOT NULL,
                api_key TEXT NOT NULL UNIQUE,
                api_secret_hash TEXT NOT NULL,
                public_key TEXT,
                metadata TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                user_id TEXT,
                disabled_at INTEGER DEFAULT NULL,
                organization TEXT DEFAULT NULL
            )",
            [],
        )?;

        // 迁移：旧数据库可能没有 disabled_at 列
        let _ = conn.execute("ALTER TABLE agents ADD COLUMN disabled_at INTEGER DEFAULT NULL", []);
        // 迁移：旧数据库可能没有 organization 列
        let _ = conn.execute("ALTER TABLE agents ADD COLUMN organization TEXT DEFAULT NULL", []);
        // 迁移：旧数据库可能没有 callback_url 列
        let _ = conn.execute("ALTER TABLE agents ADD COLUMN callback_url TEXT DEFAULT NULL", []);

        conn.execute(
            "CREATE TABLE IF NOT EXISTS provider_profiles (
                agent_id TEXT PRIMARY KEY,
                team TEXT NOT NULL,
                skills TEXT NOT NULL,
                pricing_model TEXT NOT NULL,
                FOREIGN KEY (agent_id) REFERENCES agents(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_agents_api_key ON agents(api_key)",
            [],
        )?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// 注册新Agent
    pub async fn register(
        &self,
        req: RegisterRequest,
        user_id: Option<String>,
    ) -> Result<RegisterResponse, GaggleError> {
        // 同 user_id 下 name 去重
        if let Some(ref uid) = user_id {
            let db = self.db.lock().await;
            let existing: Option<String> = db
                .query_row(
                    "SELECT id FROM agents WHERE name = ?1 AND user_id = ?2 AND disabled_at IS NULL",
                    params![req.name, uid],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            if let Some(existing_id) = existing {
                // 返回已有 agent 的信息（但不暴露 api_secret）
                let agent = {
                    let mut stmt = db.prepare(&format!(
                        "SELECT {AGENT_COLUMNS} FROM agents WHERE id = ?1"
                    ))?;
                    stmt.query_row(params![existing_id], map_agent)?
                };
                drop(db);
                return Ok(RegisterResponse {
                    id: agent.id,
                    agent_type: agent.agent_type,
                    name: agent.name,
                    api_key: agent.api_key,
                    api_secret: String::new(), // 已注册不返回 secret
                    created_at: agent.created_at,
                    organization: agent.organization,
                });
            }
        }

        let id = Uuid::new_v4().to_string();
        let api_key = format!("gag_{}", Uuid::new_v4().to_string().replace("-", ""));
        let api_secret = format!("gas_{}", Uuid::new_v4().to_string().replace("-", ""));
        let api_secret_hash = hash_secret(&api_secret);
        let created_at = Utc::now().timestamp();

        let agent = Agent {
            id: id.clone(),
            agent_type: req.agent_type.clone(),
            name: req.name.clone(),
            api_key: api_key.clone(),
            api_secret_hash,
            public_key: req.public_key.clone(),
            metadata: req.metadata.clone(),
            created_at,
            user_id: user_id.clone(),
            disabled_at: None,
            organization: req.organization.clone(),
            callback_url: req.callback_url.clone(),
        };

        let db = self.db.lock().await;
        db.execute(
            "INSERT INTO agents (id, agent_type, name, api_key, api_secret_hash, public_key, metadata, created_at, user_id, disabled_at, organization, callback_url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                agent.id,
                serde_json::to_string(&agent.agent_type)?,
                agent.name,
                agent.api_key,
                agent.api_secret_hash,
                agent.public_key,
                serde_json::to_string(&agent.metadata)?,
                agent.created_at,
                agent.user_id,
                agent.disabled_at,
                agent.organization,
                agent.callback_url,
            ],
        )?;

        if req.agent_type == AgentType::Provider {
            let profile = ProviderProfile {
                agent_id: id.clone(),
                team: vec![],
                skills: vec![],
                pricing_model: PricingModel::Negotiated,
            };

            db.execute(
                "INSERT INTO provider_profiles (agent_id, team, skills, pricing_model)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    profile.agent_id,
                    serde_json::to_string(&profile.team)?,
                    serde_json::to_string(&profile.skills)?,
                    serde_json::to_string(&profile.pricing_model)?,
                ],
            )?;
        }

        Ok(RegisterResponse {
            id,
            agent_type: req.agent_type,
            name: req.name,
            api_key,
            api_secret,
            created_at,
            organization: req.organization,
        })
    }

    /// 通过API Key获取Agent
    pub async fn get_by_api_key(&self, api_key: &str) -> Result<Option<Agent>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents WHERE api_key = ?1"
        ))?;

        let agent = stmt.query_row(params![api_key], map_agent).optional()?;

        Ok(agent)
    }

    /// 通过ID获取Agent
    pub async fn get_by_id(&self, id: &str) -> Result<Option<Agent>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(&format!("SELECT {AGENT_COLUMNS} FROM agents WHERE id = ?1"))?;

        let agent = stmt.query_row(params![id], map_agent).optional()?;

        Ok(agent)
    }

    /// 获取Provider详情
    pub async fn get_provider_profile(
        &self,
        agent_id: &str,
    ) -> Result<Option<ProviderProfile>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT agent_id, team, skills, pricing_model FROM provider_profiles WHERE agent_id = ?1"
        )?;

        let profile = stmt
            .query_row(params![agent_id], |row| {
                let team_str: String = row.get(1)?;
                let skills_str: String = row.get(2)?;
                let pricing_model_str: String = row.get(3)?;

                Ok(ProviderProfile {
                    agent_id: row.get(0)?,
                    team: serde_json::from_str(&team_str).unwrap_or_default(),
                    skills: serde_json::from_str(&skills_str).unwrap_or_default(),
                    pricing_model: serde_json::from_str(&pricing_model_str)
                        .unwrap_or(PricingModel::Negotiated),
                })
            })
            .optional()?;

        Ok(profile)
    }

    /// 列出所有未禁用的Agent
    pub async fn list_agents(&self) -> Result<Vec<Agent>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents WHERE disabled_at IS NULL ORDER BY created_at DESC"
        ))?;

        let agents = stmt
            .query_map([], map_agent)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(agents)
    }

    /// 获取 Agent 总数
    pub async fn count(&self) -> Result<usize, GaggleError> {
        let db = self.db.lock().await;
        let count: i64 = db.query_row("SELECT COUNT(*) FROM agents", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// 列出指定用户未禁用的Agent
    pub async fn list_user_agents(&self, user_id: &str) -> Result<Vec<Agent>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents WHERE user_id = ?1 AND disabled_at IS NULL ORDER BY created_at DESC"
        ))?;

        let agents = stmt
            .query_map(params![user_id], map_agent)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(agents)
    }

    /// 软删除 Agent（设置 disabled_at）
    pub async fn disable(&self, agent_id: &str) -> Result<Agent, GaggleError> {
        let db = self.db.lock().await;
        let now = Utc::now().timestamp();
        let rows = db.execute(
            "UPDATE agents SET disabled_at = ?1 WHERE id = ?2 AND disabled_at IS NULL",
            params![now, agent_id],
        )?;
        if rows == 0 {
            // 可能不存在或已禁用
            let existing = db
                .query_row(
                    &format!("SELECT {AGENT_COLUMNS} FROM agents WHERE id = ?1"),
                    params![agent_id],
                    map_agent,
                )
                .optional()?;
            return match existing {
                None => Err(GaggleError::NotFound(format!(
                    "Agent not found: {}",
                    agent_id
                ))),
                Some(a) if a.disabled_at.is_some() => Err(GaggleError::ValidationError(
                    "Agent already disabled".to_string(),
                )),
                Some(_) => Err(GaggleError::Internal("Unexpected disable failure".to_string())),
            };
        }
        // 返回更新后的 Agent
        let agent = db
            .query_row(
                &format!("SELECT {AGENT_COLUMNS} FROM agents WHERE id = ?1"),
                params![agent_id],
                map_agent,
            )
            .optional()?
            .ok_or_else(|| GaggleError::NotFound(format!("Agent not found: {}", agent_id)))?;
        Ok(agent)
    }

    /// 更新 Agent 信息（name / metadata）
    pub async fn update(
        &self,
        agent_id: &str,
        req: &UpdateAgentRequest,
    ) -> Result<Agent, GaggleError> {
        // 先确认存在且未禁用
        {
            let db = self.db.lock().await;
            let agent = db
                .query_row(
                    &format!("SELECT {AGENT_COLUMNS} FROM agents WHERE id = ?1"),
                    params![agent_id],
                    map_agent,
                )
                .optional()?;
            match agent {
                None => return Err(GaggleError::NotFound(format!("Agent not found: {}", agent_id))),
                Some(a) if a.disabled_at.is_some() => {
                    return Err(GaggleError::ValidationError("Agent is disabled".to_string()));
                }
                Some(_) => {}
            }
        }

        // 动态构建 UPDATE
        let mut sets = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql + Send>> = Vec::new();

        if let Some(ref name) = req.name {
            sets.push("name = ?".to_string());
            param_values.push(Box::new(name.clone()));
        }
        if let Some(ref metadata) = req.metadata {
            sets.push("metadata = ?".to_string());
            param_values.push(Box::new(serde_json::to_string(metadata)?));
        }
        if let Some(ref organization) = req.organization {
            sets.push("organization = ?".to_string());
            param_values.push(Box::new(organization.clone()));
        }
        if let Some(ref callback_url) = req.callback_url {
            sets.push("callback_url = ?".to_string());
            param_values.push(Box::new(callback_url.clone()));
        }

        if sets.is_empty() {
            return self.get_by_id(agent_id).await?.ok_or_else(|| {
                GaggleError::NotFound(format!("Agent not found: {}", agent_id))
            });
        }

        let db = self.db.lock().await;
        // WHERE id = ?
        param_values.push(Box::new(agent_id.to_string()));

        let sql = format!("UPDATE agents SET {} WHERE id = ? AND disabled_at IS NULL", sets.join(", "));
        let params: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref() as &dyn rusqlite::types::ToSql).collect();
        db.execute(&sql, params.as_slice())?;

        let agent = db
            .query_row(
                &format!("SELECT {AGENT_COLUMNS} FROM agents WHERE id = ?1"),
                params![agent_id],
                map_agent,
            )
            .optional()?
            .ok_or_else(|| GaggleError::NotFound(format!("Agent not found: {}", agent_id)))?;
        Ok(agent)
    }
}

fn hash_secret(secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    format!("{:x}", hasher.finalize())
}
