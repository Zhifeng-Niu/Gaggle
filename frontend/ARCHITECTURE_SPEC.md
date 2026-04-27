# Gaggle 前端重构架构规范 (基于后端真实能力对齐)

## 一、 核心概念与后端映射

为了将“上帝视角终端”的概念与 Gaggle 后端真实的 Rust API 严密对齐，我们对四大核心空间（The 4 Spaces）的底层逻辑进行了重构映射：

### 1. 🌐 The Nexus (拓扑监控 / 发现网络)
*   **功能定位**：全网 Provider 搜索、需求广播与实时匹配监听。
*   **对接后端能力**：
    *   `GET /api/v1/providers/search`：执行拓扑图中的节点发现。
    *   `POST /api/v1/needs`：向网络发射广播（视觉表现为光束）。
    *   `WS: need_matched`：监听服务端推送的匹配成功事件，在 UI 上形成节点间的连接连线。
    *   `GET /api/v1/market/:category`：在拓扑图角落显示实时大盘行情。

### 2. ⚔️ The Arena (实时博弈场 / 谈判空间)
*   **功能定位**：双边谈判与 RFP 多方竞标的实时可视化面板。
*   **对接后端能力**：
    *   **核心载体**：`Space` 和 `RFP Context` (`POST /api/v1/spaces` / `POST /api/v1/spaces/rfp`)。
    *   **结构化报价引擎**：不再是纯文本聊天。UI 必须展示基于 `ProposalDimensions`（Price, Days, Quality）的结构化表单或图表。
    *   **WebSocket 实时动作**：映射为终端控制台的指令操作：
        *   发出报价：`WS: submit_proposal`
        *   对手响应：`WS: respond_to_proposal` (Accept / Reject / Counter)
        *   RFP 轮次推进：`WS: advance_round`
    *   **UI 表现**：左侧为多维度参数滑块（价格/时间/质量），中间为多轮还价的“博弈收敛曲线”，右侧滚动 WebSocket JSON 原始报文。

### 3. ⚙️ The Forge (策略锻造炉 / Agent 配置)
*   **功能定位**：深度配置 Agent 的技能、定价模型与评估策略。
*   **对接后端能力**：
    *   不再是任意 Python 脚本编辑器，而是基于 `ProviderProfile` 的高级 JSON/表单配置器。
    *   `POST /api/v1/agents/register`：注入技能标签 (`skills`) 和底层定价模式 (`pricing_model`)。
    *   `GET /api/v1/templates`：允许用户一键加载预设的行业 Agent 模板。

### 4. 📜 The Ledger (密码学账本 / 合同与信誉)
*   **功能定位**：交易后链路追踪，包括合同生成、里程碑执行与信誉沉淀。
*   **对接后端能力**：
    *   `POST /api/v1/spaces/:space_id/contract`：将 Arena 中的最终 Proposal 转换为不可篡改的合同。
    *   `POST /api/v1/contracts/:id/milestones/:mid/submit`：可视化里程碑的进度条。
    *   `GET /api/v1/agents/:id/reputation` & `POST /api/v1/spaces/:id/rate`：驱动信誉仪表盘（展示 Outcome 评分）。

## 二、 交互范式重构

1. **结构化数据优先**：彻底抛弃原来类似 ChatGPT 的“聊天对话框”形式，谈判过程必须呈现为**维度对比表**和**折线图**（如价格/时间的妥协轨迹）。
2. **命令驱动 (Cmd+K)**：全局搜索框不仅用于路由跳转，还用于快速执行后端 API（例如输入 `> search providers --skill python` 直接调用 API 并渲染结果）。
3. **后台事件队列 (Offline Event Queue)**：利用 UI 右侧的系统日志面板，渲染由于 Agent 离线而缓存的事件，当 WebSocket 重连时实现“数据倾泻”的视觉效果。

## 三、 移动端降维策略

*   **导航**：保留底部的 4 个 Tab (Nexus, Arena, Forge, Ledger)。
*   **Arena (谈判页)**：在手机端隐藏 JSON 日志流，仅保留“博弈曲线”和“当前最新报价卡片”。利用 BottomSheet 进行 Accept/Counter 决策。
*   **Forge (配置页)**：将复杂的 JSON 树形编辑器转换为原生的分段选择器 (Segmented Control) 和步进器 (Stepper)。
