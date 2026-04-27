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

## 追加记录（2026-04-26 / 设计系统统一与 Tokens 抽离）

### 项目前情况补充
- `/Users/stbz/atrrax/Gaggle_cline/frontend/style.css` 已经同时承载 Landing、控制台、文档样式，但变量分散在多个 `:root` 块中，旧变量与 Landing 专用变量并行，长期维护成本高。
- `/Users/stbz/atrrax/Gaggle_cline/frontend/index.html`、`/Users/stbz/atrrax/Gaggle_cline/frontend/docs.html` 虽已完成中文化和功能补页，但组件视觉仍有不一致：按钮样式、圆角体系、卡片边框、阴影层级、文档块与控制台卡片的语言不同。
- 项目缺少独立的设计系统说明页，后续新增页面或组件时没有统一的 token 文档和复用约束。

### 本轮改动明细

#### 1) `/Users/stbz/atrrax/Gaggle_cline/frontend/design-tokens.css`
- 新增设计基础层，统一定义字体、颜色、描边、阴影、半径、间距、尺寸、时长、容器宽度与 safe-area token。
- 保留旧命名兼容别名，例如 `--bg-card`、`--accent`、`--landing-*`、`--space-*`，确保现有页面无需大范围重写类名即可接入新视觉基线。
- 将状态色抽象成 created / active / concluded / cancelled / expired 语义变量，供徽标、消息类型、时间线等组件复用。

#### 2) `/Users/stbz/atrrax/Gaggle_cline/frontend/design-system.css`
- 新增组件层覆盖样式，统一 Landing、Console、Docs 的导航、按钮、输入框、卡片、消息气泡、代码块、表格、底部标签栏、响应式断点表现。
- 对移动端继续遵守“底部标签栏 + 单列卡片 + safe-area”规则，避免控制台在手机端出现多级复杂布局。
- 用统一的玻璃面板、冷色高光与语义边框替换原先分裂的组件视觉，同时保持现有 DOM 与业务逻辑不变。

#### 3) `/Users/stbz/atrrax/Gaggle_cline/frontend/design-system.html`
- 新增设计系统文档页，沉淀设计原则、核心 token、字体层级、组件规范、布局系统、移动端约束与开发用法。
- 该页面本身复用了 docs 页面布局与新样式，作为后续前端扩展的视觉规范入口。

#### 4) `/Users/stbz/atrrax/Gaggle_cline/frontend/index.html`
- 在首页引入 `design-tokens.css` 与 `design-system.css`。
- 为 Landing 顶部导航、控制台头部导航与页脚新增“设计系统”入口，形成首页 / 文档 / 设计系统统一信息架构。
- 保留最近一轮工作台、Provider 发现、RFP、信誉与我的资料结构，不动既有业务表单与数据流。

#### 5) `/Users/stbz/atrrax/Gaggle_cline/frontend/docs.html`
- 同步引入 `design-tokens.css` 与 `design-system.css`，让接入文档页与首页、控制台共享同一套设计系统。
- 在顶部导航、侧栏目录和首屏 TOC 中新增“设计系统”入口，增强文档间的可发现性。

#### 6) `/Users/stbz/atrrax/Gaggle_cline/PROJECT_STATUS_20260426_DESIGN_SYSTEM.txt`
- 新建本轮专项状态文件，记录改动前情况、目标、实际修改与验证预期，便于后续 coder 轮次追踪。

#### 7) 追加交付文档
- 新增 `/Users/stbz/atrrax/Gaggle_cline/frontend/DESIGN_SYSTEM_GUIDE.md`
  - 汇总颜色板、字体系统、间距规范、组件状态、栅格与使用约定，作为代码外的设计系统说明书。
- 新增 `/Users/stbz/atrrax/Gaggle_cline/frontend/design-tokens.json`
  - 以 JSON 形式导出核心设计令牌，便于后续对接 Figma Tokens、Style Dictionary 或其他设计资产同步工具。
- 新增 `/Users/stbz/atrrax/Gaggle_cline/frontend/BROWSER_COMPATIBILITY_REPORT.md`
  - 记录浏览器目标矩阵、已知风险点、已完成验证与后续建议。
- 新增 `/Users/stbz/atrrax/Gaggle_cline/frontend/PERFORMANCE_REPORT.md`
  - 记录本轮样式重构的策略、性能影响判断与后续优化方向。
- 新增 `/Users/stbz/atrrax/Gaggle_cline/frontend/MAINTENANCE_GUIDE.md`
  - 约束后续新增页面如何接入 tokens、复用组件并记录日志。
- 新增 `/Users/stbz/atrrax/Gaggle_cline/frontend/VISUAL_REGRESSION_CHECKLIST.md`
  - 建立 Landing / Console / Docs / Design System 的视觉回归验收清单。

### 本轮自测与验证结果
- 已检查 `frontend/index.html`、`frontend/docs.html`、`frontend/design-system.html`、`frontend/design-tokens.css`、`frontend/design-system.css` 的编辑器诊断，均无新增错误。
- 已执行 `node --check /Users/stbz/atrrax/Gaggle_cline/frontend/app.js`，确认控制台脚本未受样式重构影响。
- 已先清理本地 `8080`、`4173` 端口占用，再重启后端与前端静态服务。
- 后端以 `GAGGLE_DATABASE_PATH=/tmp/gaggle-ui-design-system.db cargo run` 启动，并验证 `http://127.0.0.1:8080/health` 返回 `200`。
- 前端以 `python3 -m http.server 4173 --directory /Users/stbz/atrrax/Gaggle_cline/frontend` 启动，并验证以下页面均返回 `200`：
  - `http://127.0.0.1:4173/`
  - `http://127.0.0.1:4173/docs.html`
  - `http://127.0.0.1:4173/design-system.html`
- 已补充显式断点（320 / 768 / 1024 / 1440）、12 列栅格、按钮四态与可访问性说明到设计系统文档与 token 文件中。

### 当前遗留项
- `style.css` 仍保留历史样式实现，当前通过新增 tokens 与 design-system 覆盖层完成统一；后续若继续深度整理，可把结构层与组件层再拆分得更细。
- docs 正文主体仍保留部分英文技术内容，本轮重点是视觉与规范统一，没有做全文翻译。
