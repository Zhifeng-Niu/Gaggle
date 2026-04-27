//! Agent 模板类型定义与预设数据

use serde::{Deserialize, Serialize};

/// Agent 模板
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub capabilities: Vec<String>,
    pub default_config: serde_json::Value,
}

/// 返回所有预设模板
pub fn all_templates() -> Vec<AgentTemplate> {
    vec![
        AgentTemplate {
            id: "supply_chain_provider".into(),
            name: "供应链服务商".into(),
            description: "提供物流、库存管理、采购等供应链服务".into(),
            category: "supply_chain".into(),
            capabilities: vec![
                "logistics".into(),
                "inventory".into(),
                "procurement".into(),
            ],
            default_config: serde_json::json!({
                "pricing_model": "negotiated",
                "response_time_hours": 4
            }),
        },
        AgentTemplate {
            id: "data_analyst".into(),
            name: "数据分析师".into(),
            description: "数据挖掘、可视化、统计分析服务".into(),
            category: "data_analysis".into(),
            capabilities: vec![
                "data_mining".into(),
                "visualization".into(),
                "statistics".into(),
            ],
            default_config: serde_json::json!({
                "pricing_model": "fixed",
                "typical_deliverable": "report"
            }),
        },
        AgentTemplate {
            id: "content_writer".into(),
            name: "内容创作师".into(),
            description: "文案撰写、SEO 优化、翻译等内容服务".into(),
            category: "content_creation".into(),
            capabilities: vec![
                "copywriting".into(),
                "seo".into(),
                "translation".into(),
            ],
            default_config: serde_json::json!({
                "pricing_model": "fixed",
                "unit": "per_article"
            }),
        },
        AgentTemplate {
            id: "customer_service".into(),
            name: "客服代理".into(),
            description: "客户支持、FAQ 处理、工单管理".into(),
            category: "other".into(),
            capabilities: vec![
                "support".into(),
                "faq".into(),
                "ticket_management".into(),
            ],
            default_config: serde_json::json!({
                "pricing_model": "negotiated",
                "availability": "24/7"
            }),
        },
        AgentTemplate {
            id: "financial_advisor".into(),
            name: "财务顾问".into(),
            description: "会计、税务、投资分析等财务服务".into(),
            category: "finance".into(),
            capabilities: vec![
                "accounting".into(),
                "tax".into(),
                "investment".into(),
            ],
            default_config: serde_json::json!({
                "pricing_model": "negotiated",
                "certification_required": true
            }),
        },
        AgentTemplate {
            id: "legal_assistant".into(),
            name: "法律助手".into(),
            description: "合同审查、合规咨询、法律研究".into(),
            category: "consulting".into(),
            capabilities: vec![
                "contract_review".into(),
                "compliance".into(),
                "research".into(),
            ],
            default_config: serde_json::json!({
                "pricing_model": "negotiated",
                "jurisdiction": "cn"
            }),
        },
        AgentTemplate {
            id: "marketing_agent".into(),
            name: "营销代理".into(),
            description: "营销活动策划、社交媒体运营、数据分析".into(),
            category: "marketing".into(),
            capabilities: vec![
                "campaign".into(),
                "social_media".into(),
                "analytics".into(),
            ],
            default_config: serde_json::json!({
                "pricing_model": "negotiated",
                "reporting_frequency": "weekly"
            }),
        },
        AgentTemplate {
            id: "tech_support".into(),
            name: "技术支持".into(),
            description: "调试排障、部署运维、系统监控".into(),
            category: "software_dev".into(),
            capabilities: vec![
                "debugging".into(),
                "deployment".into(),
                "monitoring".into(),
            ],
            default_config: serde_json::json!({
                "pricing_model": "negotiated",
                "sla_hours": 2
            }),
        },
        AgentTemplate {
            id: "translation_agent".into(),
            name: "翻译代理".into(),
            description: "翻译、本地化、口译服务".into(),
            category: "other".into(),
            capabilities: vec![
                "translation".into(),
                "localization".into(),
                "interpretation".into(),
            ],
            default_config: serde_json::json!({
                "pricing_model": "fixed",
                "unit": "per_1000_words"
            }),
        },
    ]
}

/// 按 ID 查找模板
pub fn get_template(id: &str) -> Option<AgentTemplate> {
    all_templates().into_iter().find(|t| t.id == id)
}

/// 按分类筛选模板
pub fn list_templates(category: Option<&str>) -> Vec<AgentTemplate> {
    let all = all_templates();
    match category {
        Some(cat) => all.into_iter().filter(|t| t.category == cat).collect(),
        None => all,
    }
}
