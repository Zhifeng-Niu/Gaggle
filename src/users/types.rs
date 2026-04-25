//! 用户类型定义

use serde::{Deserialize, Serialize};

/// 用户主体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub display_name: String,
    #[serde(skip_serializing)]
    pub api_key: String,
    #[serde(skip_serializing)]
    pub api_secret_hash: String,
    pub created_at: i64,
}

/// 用户注册请求
#[derive(Debug, Deserialize)]
pub struct UserRegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

/// 用户注册响应
#[derive(Debug, Serialize)]
pub struct UserRegisterResponse {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub api_key: String,
    pub api_secret: String,
}

/// 用户登录请求
#[derive(Debug, Deserialize)]
pub struct UserLoginRequest {
    pub email: String,
    pub password: String,
}

/// 用户登录响应
#[derive(Debug, Serialize)]
pub struct UserLoginResponse {
    pub api_key: String,
}
