# 项目更新日志

## 项目前状态（2026-04-25 / 本轮改动前）
- 前端为单页结构，`/Users/stbz/atrrax/Gaggle_cline/frontend/index.html` 同时承载 Landing、登录弹层与 Dashboard。
- Landing 已有一版深色视觉，但区块组织较松散，品牌叙事偏“协议介绍页”，缺少更完整的转化路径与信息层级。
- Dashboard 功能仍需保留，且部分文案仍是英文；创建 Agent、空间列表、提案时间线等核心 UI 未完全中文化。
- `app.js` 与 `landing-bg.js` 同时初始化粒子背景，存在重复渲染与不必要的前台动画开销。
- 修改记录文件已存在，但内容混有旧状态描述，且未准确反映本轮“保留 Dashboard、重构 Landing、默认中文”的目标。

## 本轮目标
1. 按最新视觉表达重构 Landing，强化“Agent 商业协议层”的产品定位。
2. 保留 Dashboard 结构和功能，不破坏现有登录、Agent、Space、消息、提案流程。
3. 将前端默认展示语言落到中文，补齐 Dashboard 的静态与动态文案。
4. 清理重复的前台粒子逻辑，保留单一 Landing 专属动画实现。
5. 记录本轮改动、验证方式与遗留问题，便于下一轮继续迭代。

## 本轮修改细节（2026-04-25）

### 1) `/Users/stbz/atrrax/Gaggle_cline/frontend/index.html`
- 重写 Landing 结构，改为导航、Hero、价值条带、协议能力、系统架构、开发接入、业务场景、CTA、Footer 的分层布局。
- 所有 Landing 核心文案补充 `data-zh` / `data-en`，默认中文展示，同时保留语言切换能力。
- 保留原有 `#dashboardPage`、`#loginOverlay`、`#createAgentModal` 等关键节点，避免破坏既有脚本挂载。
- 将 Dashboard 里的静态文案改为中文，包括导航、Agent 创建弹层、筛选项、空状态、消息流和提案时间线标题。

### 2) `/Users/stbz/atrrax/Gaggle_cline/frontend/style.css`
- 保留 Dashboard 基础样式区，整段替换 Landing 区样式，升级为更完整的深色玻璃质感布局。
- 新增 Hero 双栏、统计卡、协议流卡片、能力卡片、架构层堆栈、开发者代码面板、场景卡片、CTA 容器等样式。
- 增加 `btn-sm`、`auth-switch` 等缺失样式，补齐 Dashboard 和登录弹层的细节表现。
- 强化移动端适配：窄屏下导航动作区自动换行，主要区块改为单列卡片，CTA/按钮改为纵向堆叠。
- 增加 `prefers-reduced-motion` 处理，避免用户偏好减少动态时仍持续执行重动画。

### 3) `/Users/stbz/atrrax/Gaggle_cline/frontend/app.js`
- 新增状态、角色、空间类型、消息类型、提案类型的中文映射函数，统一 Dashboard 动态文案。
- 将 toast、健康状态、Agent 列表、空间列表、详情信息、消息流、提案时间线等运行时文本切换为中文。
- 修正登录/注册弹层副标题在不同模式下不切换的问题。
- 删除重复的粒子背景初始化函数，避免与 `landing-bg.js` 双重绘制造成性能浪费。
- 保留 Dashboard 的 WebSocket、消息解密、Space 明细与提案刷新逻辑，不做业务协议层改动。

### 4) `/Users/stbz/atrrax/Gaggle_cline/frontend/landing-bg.js`
- 重写 Landing 专属脚本，负责语言切换与单一粒子网络背景动画。
- 将当前语言写入 `localStorage`，刷新后保持语言选择；默认仍是中文。
- 对 `prefers-reduced-motion` 做提前退出处理，避免在低动态偏好场景继续执行粒子动画。
- 根据屏宽动态调整粒子数量和连线距离，减轻移动端渲染压力。

## 本轮验证记录
- 使用 `node --check` 计划对 `/Users/stbz/atrrax/Gaggle_cline/frontend/app.js` 与 `/Users/stbz/atrrax/Gaggle_cline/frontend/landing-bg.js` 做语法检查。
- 使用本地静态服务检查页面资源可正常输出，并确认 Dashboard 保留在同一入口页面中。
- 本轮未触碰 Rust 服务与 API 协议层，未执行后端业务回归测试。

## 当前遗留问题
- Dashboard 仍依赖远端 API 地址 `http://106.15.228.101` 与对应 WebSocket，若服务不可用，前端只能展示离线/加载失败状态。
- `docs.html` 仍存在部分英文内容，本轮未对协议文档页做完整中文化。
- 当前未引入自动化 UI 截图回归；视觉验收仍以本地静态预览和脚本语法检查为主。

## 追加记录（2026-04-25 / Dark Futuristic Tech 重构）
- Landing 视觉规范从上一版“青紫协议感”进一步收敛为黑白灰主导，仅保留极淡冷光作为氛围层，避免过度彩色化。
- `index.html` 重新调整为：固定导航、Hero、Trusted Metrics、核心功能、工作流程、协议 JSON、场景说明、CTA、Footer。
- `style.css` 新增独立的 Landing 设计 token，采用玻璃态面板、细边框、渐变文字、分隔线与克制发光，尽量贴近 Linear / xAI / Vercel 风格交集。
- `landing-bg.js` 改为白色粒子系统：固定 Canvas、80-120 粒子级别的密度策略、标题周边增密、鼠标排斥、滚动轻微偏移、Intersection Observer reveal。
- 本轮未动 Dashboard 核心业务逻辑，仅保留其容器、登录入口和控制台能力。

## 追加记录（2026-04-25 / 控制台补页与接口适配）

### 项目前情况补充
- `/Users/stbz/atrrax/Gaggle_cline/frontend/index.html` 原先仍是 Landing + Dashboard 双区域结构，但控制台主体只有“空间列表 + 消息流 + 提案时间线”三块，缺少 Provider 发现、Profile、创建谈判/RFP、信誉评分等独立工作流。
- `/Users/stbz/atrrax/Gaggle_cline/frontend/app.js` 仅覆盖登录、创建 Agent、Space 列表展示与消息/提案拉取，没有把 `/api/v1/providers/search`、`/api/v1/providers/:agent_id/profile`、`/api/v1/agents/:agent_id/reputation`、`/api/v1/spaces/:space_id/rate` 等现有接口串成完整中文控制台。
- 后端已具备 Discovery、Reputation、create_space、create_rfp 等能力，但前端没有对应入口，用户无法在现有界面完成从发现 Provider 到发起协商、再到评分沉淀信誉的闭环。
- 文档页 `/Users/stbz/atrrax/Gaggle_cline/frontend/docs.html` 风格与首页/控制台不完全一致，且关键导航与首屏文案仍以英文为主。

### 本轮改动明细

#### 1) `/Users/stbz/atrrax/Gaggle_cline/frontend/index.html`
- 将 Dashboard 扩展为多视图控制台：总览、Provider 发现、创建谈判、创建 RFP、信誉评分、我的资料五个主视图。
- 保留原有登录弹层、创建 Agent 弹层、空间详情面板 DOM 锚点，避免破坏既有脚本能力，同时把“空间详情”嵌入新的总览工作台。
- 新增移动端底部标签栏，按“总览 / 发现 / 创建 / 信誉 / 我的”组织手机端导航，符合移动端扁平层级要求。
- 新增搜索表单、Provider 详情容器、双边谈判表单、RFP 表单、评分表单、Provider Profile 表单等结构，全部默认中文文案。

#### 2) `/Users/stbz/atrrax/Gaggle_cline/frontend/app.js`
- 重写控制台主逻辑，新增中文单页工作台状态管理，自动根据当前运行域名推导 API Base，优先支持本地 `127.0.0.1:8080` 自测。
- 接入 Provider Discovery：对接 `/api/v1/providers/search`、`/api/v1/agents/:agent_id/reputation`，支持筛选、详情查看、加入 RFP 候选、从发现页一键发起谈判。
- 接入创建谈判 / RFP：通过现有 WebSocket 连接发送 `create_space`、`create_rfp`，并在空间列表、总览统计中实时刷新。
- 接入信誉评分：对接 `/api/v1/agents/:agent_id/reputation` 与 `/api/v1/spaces/:space_id/rate`，支持查看当前 Agent 信誉摘要、最近事件，并基于已结束空间为对手方提交评分。
- 接入 Provider Profile：对接 `/api/v1/providers/:agent_id/profile` 与 `/api/v1/providers/me/profile`，Provider Agent 可直接维护公开展示名称、分类、标签、价格区间和可用状态。
- 统一总览统计、侧栏会话信息、最近空间、当前空间摘要、消息流与提案时间线的中文输出，同时保留原有消息解密与实时通知逻辑。

#### 3) `/Users/stbz/atrrax/Gaggle_cline/frontend/style.css`
- 在既有 Landing 样式基础上新增整套控制台工作台样式：多视图布局、工作卡片、统计卡、Provider 卡片、摘要面板、RFP 候选池、事件列表与 Profile 卡片。
- 为移动端新增可滑入侧边栏与固定底部标签栏，保证手机竖屏下导航清晰、主操作按钮不贴近危险区域。
- 补齐视图切换按钮、筛选结果区、表单网格、标签 Chip、空状态卡、评分事件流等组件样式，统一与首页 / 文档页的深色玻璃质感。

#### 4) `/Users/stbz/atrrax/Gaggle_cline/frontend/docs.html`
- 同步首页/控制台的基础风格版本号，统一引用新的样式文件版本。
- 将文档页头部导航与首屏核心文案改为中文，首屏目录项改为“快速开始 / WebSocket 协议 / 代码示例 / 消息参考”等更一致的中文表达。
- 将首屏主要复制按钮文案改为“复制”，与控制台中文环境统一。

### 本轮自测与验证
- 先清理本地 `8080`、`4173` 端口占用，再启动服务，符合本轮前的端口清理要求。
- 使用 `node --check /Users/stbz/atrrax/Gaggle_cline/frontend/app.js` 验证前端脚本语法通过。
- 使用 VS Code Diagnostics 检查 `/Users/stbz/atrrax/Gaggle_cline/frontend/index.html`、`/Users/stbz/atrrax/Gaggle_cline/frontend/style.css`、`/Users/stbz/atrrax/Gaggle_cline/frontend/docs.html`，均无错误。
- 以独立测试数据库启动后端：`GAGGLE_DATABASE_PATH=/tmp/gaggle-ui-test.db cargo run`，确认服务监听 `0.0.0.0:8080`。
- 以 `python3 -m http.server 4173 --directory /Users/stbz/atrrax/Gaggle_cline/frontend` 启动前端静态预览，并打开 `http://127.0.0.1:4173/` 进行本地页面联调预览。

### 当前遗留项
- 文档页仍有大量技术正文保持英文，仅完成导航与首屏层的风格统一；若后续需要，可再做全文中文化。
- 当前前端对 `create_space` / `create_rfp` 采用 WebSocket 发送并依赖服务端事件回流，尚未加入表单级 loading 状态与更细颗粒的失败回执提示。
- 信誉评分页目前面向“当前 Agent 自身信誉”和“对手方手动评分”两类主流程，尚未补充跨 Agent 信誉横向对比与筛选排序能力。
