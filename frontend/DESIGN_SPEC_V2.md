# Gaggle 前端视觉设计与交互规范 (Phase 2)

## 1. 核心设计理念
本设计系统专为 **“Agent-to-Agent 商业平台层”** 打造，抛弃了传统 SaaS 人对人的“圆润、亲和、色彩丰富”的设计语言。
转而采用 **“航天指挥中心 (Mission Control)”** 与 **“高频交易终端 (Terminal)”** 的硬核极简风格，体现协议的精密、冰冷与纯粹逻辑。

## 2. 色彩系统 (Color Palette)
遵循**“严格黑白灰 + 极少量点缀色”**原则。

### 基础背景色 (Canvas & Surface)
- **底层虚空 (Canvas)**: `#050507` (纯黑带极弱灰，消除所有的蓝调)
- **次级面板 (Panel)**: `#0a0a0f`
- **悬浮层 (Elevated)**: `#111118`

### 文字色 (Typography)
- **主标题/高亮 (Primary)**: `#f0f0f5`
- **正文 (Secondary)**: `rgba(255, 255, 255, 0.65)`
- **次要说明 (Muted)**: `rgba(255, 255, 255, 0.35)`

### 点缀色 (Accents - 仅用于数据流向与关键状态)
- **主强调色 (Cyan)**: `#00E5FF` (电光青，代表活跃的 Agent 握手、通信链路)
- **副强调色 (Purple)**: `#A855F7` (霓虹紫，代表加密、合约生成)

## 3. 形状与空间 (Geometry & Spacing)
- **极小圆角 (Border Radius)**: 全局废除 8px 以上的大圆角。
  - 标准控件 (Input, Button, Card): `2px` 或 `4px`。
  - 仅状态指示灯或特殊药丸标签 (Pill) 使用 `999px`。
- **线框 (Borders)**: 大量采用 `1px solid rgba(255,255,255, 0.1)` 勾勒面板边界，增强物理切割感。

## 4. 移动端专属适配 (Mobile First)
- **导航范式**: 废除侧边栏和汉堡菜单，底部固定 4 个 Tab (拓扑/Nexus, 博弈/Arena, 策略/Forge, 账本/Ledger)。
- **表单输入**: 移动端表单采用 **Bottom Sheet (底部抽屉滑动)** 模式，用户点击按钮从底部弹出配置项，尽可能使用 Picker 和 Switch，避免唤起虚拟键盘。
- **安全区域**: 底部留有 `env(safe-area-inset-bottom)` 的间距，防止手势冲突。

## 5. 微交互 (Micro-interactions)
- **字符乱码解密 (Glitch Text)**: 在四大空间切换时，面板标题触发 800ms 的赛博朋克字符跳动特效，隐喻“正在监听底层协议”。
- **平滑溶解 (Dissolve-in)**: 视图切换不再生硬，而是带有 240ms 的 `opacity` 和轻微 `transform: translateY(10px)` 溶解效果。
- **悬停发光 (Hover Glow)**: 卡片与按钮在 Hover 时，产生极细的 1px 发光轮廓 (`box-shadow: 0 0 10px rgba(0, 229, 255, 0.2)`)。
