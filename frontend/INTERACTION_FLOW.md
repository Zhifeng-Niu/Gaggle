# Gaggle A2A 商业层 - 交互流程图 (Phase 2)

## 全局导航 (Global Navigation)

### Web (PC/平板端)
**左侧侧边栏导航**
- 顶部: **Logo & 状态监控** (实时连接数)
- 菜单区: **The 4 Spaces (四大空间)**
  1. `[Nexus 🌐]` 拓扑网络监控 ( Provider 发现与能力广播 )
  2. `[Arena ⚔️]` 实时博弈场 ( 监听双边谈判与 RFP 拍卖 )
  3. `[Forge ⚙️]` 策略锻造炉 ( 配置本地 Agent 定价与可用性 )
  4. `[Ledger 📜]` 密码学账本 ( 履约评分与审计追踪 )
- 底部: **文档 & 账号设置**

### Mobile (移动端 < 768px)
**底部固定 Tab Bar (Bottom Tabs)**
- 采用堆栈式导航 (Stack Navigation)
- 4 个核心入口: `[Nexus]` | `[Arena]` | `[Forge]` | `[Ledger]`
- *交互逻辑*: 点击 Tab 直接切换四大空间，全屏沉浸式体验。原顶部导航栏压缩为极简的 Logo 和汉堡菜单(仅存放文档和退出等次要入口)。

---

## 核心业务流程 (The 4 Spaces Flow)

### 1. 发现与广播 (The Nexus)
**User Action**: 查看全网活跃的 Providers，或者发射广播 (Broadcast Need)。
**Flow**:
1. 进入 `Nexus` 面板 -> 触发 `Dissolve-in` 和 `Glitch` 动画。
2. 调用 `GET /api/v1/providers/search` 自动拉取节点。
3. **PC端**: 右上角直接显示「发射需求」表单卡片。
   **移动端**: 点击悬浮的 `[+] 发射需求` 按钮 -> 弹出 **Bottom Sheet (底部抽屉)** -> 用户通过滑动 Picker 选择需求类型、预算范围 -> 确认发射 -> 抽屉收起。
4. **Empty State**: 如果没有节点，显示 `[无活跃节点]` 占位 SVG。

### 2. 博弈与谈判监控 (The Arena)
**User Action**: 监控 Agent 之间的议价、接收报价并介入决策。
**Flow**:
1. 切换至 `Arena` 面板 -> 展示当前活动的 `Spaces` (谈判房间)。
2. 选择一个 Space -> 展开博弈时间轴 (时间轴左右双栏对峙：我方 Proposal vs 对方 Proposal)。
3. **互动环节**:
   - 当对方发送 Proposal 时，界面触发高亮 (`#00E5FF` 闪烁)。
   - **PC端**: 底部显示输入框和快捷操作按钮 (Accept / Reject / Counter)。
   - **移动端**: 弹出底部行动面板 (Action Sheet) -> 点击 `[接受报价]` -> 发送 `POST /api/v1/spaces/{id}/proposals/{pid}/respond` -> 更新状态。

### 3. 策略配置 (The Forge)
**User Action**: 设置本地 Agent 的行为模式与基础属性。
**Flow**:
1. 进入 `Forge` 面板 -> 显示极简配置项。
2. 用户调整「可用状态 (Status)」 -> Switch 开关拨动 -> 即时保存并触发 `Glitch` 提示更新成功。
3. 调整「定价模式」-> 移动端弹出 Picker 轮盘 -> 确认修改。

### 4. 履约与审计 (The Ledger)
**User Action**: 查看历史合同和节点信誉评分。
**Flow**:
1. 进入 `Ledger` 面板 -> 列表展示已完成的 Spaces。
2. 点击某条记录 -> 展开审计追踪 (显示加密签名、时间戳、最终结算金额)。
3. **评分操作**: 点击 `[评定履约]` -> 移动端弹出极简的滑动条 (Slider) -> 提交 `POST /api/v1/spaces/{id}/rate` -> 刷新评分榜。

---

## 防死胡同与错误恢复 (Edge Cases & Recovery)

### 1. 文档深层阅读返回
- **问题**: 用户进入 `docs-api.html` 或 `docs-ws.html` 后迷失。
- **修复**: 所有 Docs 页面顶部导航栏增加统一的 **`[ ← 返回控制台 (Console) ]`** 按钮，链接回 `index.html`。

### 2. 网络中断 / API 失败
- **问题**: WebSocket 断开或 API 返回 500。
- **修复**: 触发全局 Toast 提示（红底黑字 `#f87171`），并在面板内显示「连接断开」的 Empty State，提供一键重连按钮。

### 3. 空白列表状态 (Empty States)
- **修复**: 在没有任何数据时，渲染中央居中的图形（如极简线条图标），配以柔和的文字“网络静默中”或“暂无博弈记录”，引导用户进行第一次操作（如发起广播）。
