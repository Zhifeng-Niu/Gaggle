//! 执行引擎 SQLite 存储

use super::types::*;
use crate::error::GaggleError;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ExecutionStore {
    db: Arc<Mutex<Connection>>,
}

impl ExecutionStore {
    pub fn new(db_path: &str) -> Result<Self, GaggleError> {
        let conn = Connection::open(db_path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS contracts (
                id TEXT PRIMARY KEY,
                space_id TEXT NOT NULL UNIQUE,
                buyer_id TEXT NOT NULL,
                seller_id TEXT NOT NULL,
                terms TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                deadline INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS milestones (
                id TEXT PRIMARY KEY,
                contract_id TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                deliverable_url TEXT,
                amount REAL,
                due_date INTEGER,
                submitted_at INTEGER,
                accepted_at INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (contract_id) REFERENCES contracts(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_contracts_space_id ON contracts(space_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_contracts_buyer_id ON contracts(buyer_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_contracts_seller_id ON contracts(seller_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_milestones_contract_id ON milestones(contract_id)",
            [],
        )?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// 从共享的数据库连接创建（用于与其他 Store 共享同一 DB 文件）
    pub fn from_conn(conn: Arc<Mutex<Connection>>) -> Result<Self, GaggleError> {
        let store = Self { db: conn };
        store.ensure_schema()?;
        Ok(store)
    }

    fn ensure_schema(&self) -> Result<(), GaggleError> {
        // Schema 在 new() 中已创建，from_conn 场景下确保表存在
        let conn = self.db.blocking_lock();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS contracts (
                id TEXT PRIMARY KEY,
                space_id TEXT NOT NULL UNIQUE,
                buyer_id TEXT NOT NULL,
                seller_id TEXT NOT NULL,
                terms TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                deadline INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS milestones (
                id TEXT PRIMARY KEY,
                contract_id TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                deliverable_url TEXT,
                amount REAL,
                due_date INTEGER,
                submitted_at INTEGER,
                accepted_at INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (contract_id) REFERENCES contracts(id)
            )",
            [],
        )?;
        Ok(())
    }

    /// 创建合同 + 里程碑
    pub async fn create_contract(
        &self,
        space_id: &str,
        buyer_id: &str,
        seller_id: &str,
        terms: serde_json::Value,
        req: &CreateContractRequest,
        deadline: Option<i64>,
    ) -> Result<Contract, GaggleError> {
        let conn = self.db.lock().await;
        let now = Utc::now().timestamp();
        let contract_id = format!("ctr_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));

        conn.execute(
            "INSERT INTO contracts (id, space_id, buyer_id, seller_id, terms, status, deadline, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                contract_id,
                space_id,
                buyer_id,
                seller_id,
                terms.to_string(),
                ContractStatus::Active.as_str(),
                deadline,
                now,
                now,
            ],
        )?;

        let mut milestones = Vec::new();
        for m in &req.milestones {
            let mid = format!("mst_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
            conn.execute(
                "INSERT INTO milestones (id, contract_id, title, description, status, amount, due_date, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    mid,
                    contract_id,
                    m.title,
                    m.description,
                    MilestoneStatus::Pending.as_str(),
                    m.amount,
                    m.due_date,
                    now,
                    now,
                ],
            )?;
            milestones.push(Milestone {
                id: mid,
                contract_id: contract_id.clone(),
                title: m.title.clone(),
                description: m.description.clone(),
                status: MilestoneStatus::Pending,
                deliverable_url: None,
                amount: m.amount,
                due_date: m.due_date,
                submitted_at: None,
                accepted_at: None,
                created_at: now,
                updated_at: now,
            });
        }

        Ok(Contract {
            id: contract_id,
            space_id: space_id.to_string(),
            buyer_id: buyer_id.to_string(),
            seller_id: seller_id.to_string(),
            terms,
            milestones,
            status: ContractStatus::Active,
            deadline,
            created_at: now,
            updated_at: now,
        })
    }

    /// 获取合同详情
    pub async fn get_contract(&self, id: &str) -> Result<Option<Contract>, GaggleError> {
        let conn = self.db.lock().await;
        let contract = conn
            .query_row(
                "SELECT id, space_id, buyer_id, seller_id, terms, status, deadline, created_at, updated_at
                 FROM contracts WHERE id = ?1",
                params![id],
                |row| {
                    Ok(raw_contract_from_row(row))
                },
            )
            .optional()?;

        match contract {
            Some(mut c) => {
                c.milestones = self.query_milestones(&conn, &c.id)?;
                Ok(Some(c))
            }
            None => Ok(None),
        }
    }

    /// 通过 space_id 获取合同
    pub async fn get_contract_by_space(
        &self,
        space_id: &str,
    ) -> Result<Option<Contract>, GaggleError> {
        let conn = self.db.lock().await;
        let contract = conn
            .query_row(
                "SELECT id, space_id, buyer_id, seller_id, terms, status, deadline, created_at, updated_at
                 FROM contracts WHERE space_id = ?1",
                params![space_id],
                |row| Ok(raw_contract_from_row(row)),
            )
            .optional()?;

        match contract {
            Some(mut c) => {
                c.milestones = self.query_milestones(&conn, &c.id)?;
                Ok(Some(c))
            }
            None => Ok(None),
        }
    }

    /// 获取 Agent 参与的所有合同
    pub async fn get_agent_contracts(
        &self,
        agent_id: &str,
    ) -> Result<Vec<Contract>, GaggleError> {
        let conn = self.db.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, space_id, buyer_id, seller_id, terms, status, deadline, created_at, updated_at
             FROM contracts WHERE buyer_id = ?1 OR seller_id = ?1
             ORDER BY updated_at DESC",
        )?;

        let contracts = stmt
            .query_map(params![agent_id], |row| Ok(raw_contract_from_row(row)))?
            .collect::<Result<Vec<_>, _>>()?;

        // 加载每个合同的里程碑
        let mut result = Vec::new();
        for mut c in contracts {
            c.milestones = self.query_milestones(&conn, &c.id)?;
            result.push(c);
        }
        Ok(result)
    }

    /// 提交里程碑交付物
    pub async fn submit_milestone(
        &self,
        milestone_id: &str,
        deliverable_url: &str,
    ) -> Result<Milestone, GaggleError> {
        let conn = self.db.lock().await;
        let now = Utc::now().timestamp();

        // 验证当前状态为 pending 或 rejected
        let current_status: String = conn
            .query_row(
                "SELECT status FROM milestones WHERE id = ?1",
                params![milestone_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| GaggleError::NotFound("Milestone not found".to_string()))?;

        let status = MilestoneStatus::from_str(&current_status)
            .ok_or_else(|| GaggleError::Internal("Invalid milestone status".to_string()))?;

        if status != MilestoneStatus::Pending && status != MilestoneStatus::Rejected {
            return Err(GaggleError::ValidationError(
                "Milestone can only be submitted from pending or rejected state".to_string(),
            ));
        }

        conn.execute(
            "UPDATE milestones SET status = ?1, deliverable_url = ?2, submitted_at = ?3, updated_at = ?4
             WHERE id = ?5",
            params![
                MilestoneStatus::Submitted.as_str(),
                deliverable_url,
                now,
                now,
                milestone_id,
            ],
        )?;

        self.get_milestone_inner(&conn, milestone_id)
    }

    /// 验收/拒绝里程碑
    pub async fn accept_milestone(
        &self,
        milestone_id: &str,
        accepted: bool,
    ) -> Result<Milestone, GaggleError> {
        let conn = self.db.lock().await;
        let now = Utc::now().timestamp();

        let current_status: String = conn
            .query_row(
                "SELECT status FROM milestones WHERE id = ?1",
                params![milestone_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| GaggleError::NotFound("Milestone not found".to_string()))?;

        let status = MilestoneStatus::from_str(&current_status)
            .ok_or_else(|| GaggleError::Internal("Invalid milestone status".to_string()))?;

        if status != MilestoneStatus::Submitted {
            return Err(GaggleError::ValidationError(
                "Milestone must be in submitted state to accept/reject".to_string(),
            ));
        }

        let new_status = if accepted {
            MilestoneStatus::Accepted
        } else {
            MilestoneStatus::Rejected
        };

        let accepted_at = if accepted { Some(now) } else { None };

        conn.execute(
            "UPDATE milestones SET status = ?1, accepted_at = ?2, updated_at = ?3 WHERE id = ?4",
            params![new_status.as_str(), accepted_at, now, milestone_id],
        )?;

        let milestone = self.get_milestone_inner(&conn, milestone_id)?;

        // 检查是否所有里程碑都已 accepted
        if accepted {
            let contract_id = &milestone.contract_id;
            let pending: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM milestones WHERE contract_id = ?1 AND status != 'accepted'",
                    params![contract_id],
                    |row| row.get(0),
                )?;

            if pending == 0 {
                // 自动标记合同为 Completed
                conn.execute(
                    "UPDATE contracts SET status = ?1, updated_at = ?2 WHERE id = ?3",
                    params![ContractStatus::Completed.as_str(), now, contract_id],
                )?;
            }
        }

        Ok(milestone)
    }

    /// 发起争议
    pub async fn dispute_contract(&self, contract_id: &str) -> Result<Contract, GaggleError> {
        let conn = self.db.lock().await;
        let now = Utc::now().timestamp();

        conn.execute(
            "UPDATE contracts SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![ContractStatus::Disputed.as_str(), now, contract_id],
        )?;

        drop(conn);
        self.get_contract(contract_id)
            .await?
            .ok_or_else(|| GaggleError::NotFound("Contract not found".to_string()))
    }

    /// 获取合同的里程碑列表
    pub async fn get_milestones(
        &self,
        contract_id: &str,
    ) -> Result<Vec<Milestone>, GaggleError> {
        let conn = self.db.lock().await;
        self.query_milestones(&conn, contract_id)
    }

    // ── 内部辅助 ──

    fn query_milestones(
        &self,
        conn: &Connection,
        contract_id: &str,
    ) -> Result<Vec<Milestone>, GaggleError> {
        let mut stmt = conn.prepare(
            "SELECT id, contract_id, title, description, status, deliverable_url, amount, due_date,
                    submitted_at, accepted_at, created_at, updated_at
             FROM milestones WHERE contract_id = ?1 ORDER BY created_at ASC",
        )?;

        let milestones = stmt
            .query_map(params![contract_id], |row| {
                Ok(Milestone {
                    id: row.get(0)?,
                    contract_id: row.get(1)?,
                    title: row.get(2)?,
                    description: row.get(3)?,
                    status: MilestoneStatus::from_str(&row.get::<_, String>(4)?)
                        .unwrap_or(MilestoneStatus::Pending),
                    deliverable_url: row.get(5)?,
                    amount: row.get(6)?,
                    due_date: row.get(7)?,
                    submitted_at: row.get(8)?,
                    accepted_at: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(milestones)
    }

    fn get_milestone_inner(
        &self,
        conn: &Connection,
        milestone_id: &str,
    ) -> Result<Milestone, GaggleError> {
        conn.query_row(
            "SELECT id, contract_id, title, description, status, deliverable_url, amount, due_date,
                    submitted_at, accepted_at, created_at, updated_at
             FROM milestones WHERE id = ?1",
            params![milestone_id],
            |row| {
                Ok(Milestone {
                    id: row.get(0)?,
                    contract_id: row.get(1)?,
                    title: row.get(2)?,
                    description: row.get(3)?,
                    status: MilestoneStatus::from_str(&row.get::<_, String>(4)?)
                        .unwrap_or(MilestoneStatus::Pending),
                    deliverable_url: row.get(5)?,
                    amount: row.get(6)?,
                    due_date: row.get(7)?,
                    submitted_at: row.get(8)?,
                    accepted_at: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                GaggleError::NotFound("Milestone not found".to_string())
            }
            other => GaggleError::from(other),
        })
    }
}

fn raw_contract_from_row(
    row: &rusqlite::Row<'_>,
) -> Contract {
    Contract {
        id: row.get("id").unwrap(),
        space_id: row.get("space_id").unwrap(),
        buyer_id: row.get("buyer_id").unwrap(),
        seller_id: row.get("seller_id").unwrap(),
        terms: serde_json::from_str(&row.get::<_, String>("terms").unwrap_or_default())
            .unwrap_or(serde_json::Value::Null),
        status: ContractStatus::from_str(
            &row.get::<_, String>("status").unwrap_or_default(),
        )
        .unwrap_or(ContractStatus::Active),
        deadline: row.get("deadline").unwrap_or(None),
        created_at: row.get("created_at").unwrap(),
        updated_at: row.get("updated_at").unwrap(),
        milestones: Vec::new(), // 稍后填充
    }
}
