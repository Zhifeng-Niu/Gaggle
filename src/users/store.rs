//! 用户存储层

use crate::error::GaggleError;
use crate::users::types::*;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct UserStore {
    db: Arc<Mutex<Connection>>,
}

impl UserStore {
    pub fn new(db_path: &str) -> Result<Self, GaggleError> {
        let conn = Connection::open(db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                email TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                display_name TEXT NOT NULL,
                api_key TEXT NOT NULL UNIQUE,
                api_secret_hash TEXT NOT NULL,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_users_api_key ON users(api_key)",
            [],
        )?;
        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// 注册新用户
    pub async fn register(
        &self,
        req: UserRegisterRequest,
    ) -> Result<UserRegisterResponse, GaggleError> {
        // Input validation
        let email = req.email.trim();
        if email.is_empty() || !email.contains('@') || email.len() > 254 {
            return Err(GaggleError::ValidationError("Invalid email address".to_string()));
        }
        if req.password.len() < 8 {
            return Err(GaggleError::ValidationError("Password must be at least 8 characters".to_string()));
        }
        if req.password.len() > 128 {
            return Err(GaggleError::ValidationError("Password too long (max 128)".to_string()));
        }
        let display_name = req.display_name.trim();
        if display_name.is_empty() || display_name.len() > 64 {
            return Err(GaggleError::ValidationError("Display name must be 1-64 characters".to_string()));
        }

        // Registration rate limit: max 5 registrations per hour globally
        {
            let db = self.db.lock().await;
            let one_hour_ago = Utc::now().timestamp() - 3600;
            let recent: u32 = db
                .query_row(
                    "SELECT COUNT(*) FROM users WHERE created_at > ?1",
                    params![one_hour_ago],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            drop(db);
            if recent >= 5 {
                return Err(GaggleError::RateLimitExceeded(
                    "Registration rate limit exceeded. Try again later.".to_string(),
                ));
            }
        }

        let id = Uuid::new_v4().to_string();
        let api_key = format!("usr_{}", Uuid::new_v4().to_string().replace("-", ""));
        let api_secret = format!("uss_{}", Uuid::new_v4().to_string().replace("-", ""));
        let api_secret_hash = hash_secret(&api_secret);
        let password_hash = hash_password(&req.password)?;
        let created_at = Utc::now().timestamp();

        let db = self.db.lock().await;
        db.execute(
            "INSERT INTO users (id, email, password_hash, display_name, api_key, api_secret_hash, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, email, password_hash, display_name, api_key, api_secret_hash, created_at],
        ).map_err(|e| {
            if e.to_string().contains("UNIQUE constraint failed: users.email") {
                GaggleError::ValidationError("Email already registered".to_string())
            } else {
                GaggleError::from(e)
            }
        })?;

        Ok(UserRegisterResponse {
            id,
            email: email.to_string(),
            display_name: display_name.to_string(),
            api_key,
            api_secret,
        })
    }

    /// 用户登录
    pub async fn login(&self, req: UserLoginRequest) -> Result<UserLoginResponse, GaggleError> {
        let result = {
            let db = self.db.lock().await;
            let mut stmt = db.prepare("SELECT password_hash, api_key FROM users WHERE email = ?1")?;
            stmt.query_row(params![req.email], |row| {
                let pw_hash: String = row.get(0)?;
                let api_key: String = row.get(1)?;
                Ok((pw_hash, api_key))
            }).optional()?
        };
        // Lock released before potential sleep

        match result {
            Some((pw_hash, api_key)) => {
                if verify_password(&req.password, &pw_hash)? {
                    Ok(UserLoginResponse { api_key })
                } else {
                    // Delay on failure to slow brute-force attacks
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    Err(GaggleError::Unauthorized("Invalid credentials".to_string()))
                }
            }
            None => {
                // Same delay for non-existent users to prevent enumeration
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                Err(GaggleError::Unauthorized("Invalid credentials".to_string()))
            }
        }
    }

    /// 通过 API Key 获取用户
    pub async fn get_by_api_key(&self, api_key: &str) -> Result<Option<User>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, email, password_hash, display_name, api_key, api_secret_hash, created_at
             FROM users WHERE api_key = ?1",
        )?;

        let user = stmt
            .query_row(params![api_key], |row| {
                Ok(User {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    password_hash: row.get(2)?,
                    display_name: row.get(3)?,
                    api_key: row.get(4)?,
                    api_secret_hash: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })
            .optional()?;

        Ok(user)
    }

    /// 通过 ID 获取用户
    pub async fn get_by_id(&self, id: &str) -> Result<Option<User>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, email, password_hash, display_name, api_key, api_secret_hash, created_at
             FROM users WHERE id = ?1",
        )?;

        let user = stmt
            .query_row(params![id], |row| {
                Ok(User {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    password_hash: row.get(2)?,
                    display_name: row.get(3)?,
                    api_key: row.get(4)?,
                    api_secret_hash: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })
            .optional()?;

        Ok(user)
    }

    /// Count agents belonging to a user
    pub async fn count_agents(&self, user_id: &str) -> Result<usize, GaggleError> {
        let db = self.db.lock().await;
        let count: usize = db
            .query_row(
                "SELECT COUNT(*) FROM agents WHERE user_id = ?1",
                rusqlite::params![user_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(count)
    }
}

/// Argon2 密码哈希
fn hash_password(password: &str) -> Result<String, GaggleError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| GaggleError::Internal(format!("Password hash failed: {}", e)))?;
    Ok(hash.to_string())
}

/// 验证密码
fn verify_password(password: &str, hash: &str) -> Result<bool, GaggleError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| GaggleError::Internal(format!("Invalid hash format: {}", e)))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// SHA-256 哈希（用于 API Secret）
fn hash_secret(secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    format!("{:x}", hasher.finalize())
}
