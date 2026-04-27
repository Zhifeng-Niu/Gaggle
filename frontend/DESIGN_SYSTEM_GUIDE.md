# Gaggle 设计系统指南

## 概述
- 本设计系统基于现有 Landing Page 的视觉语言提炼而成，统一服务于 `Landing`、`Console`、`Docs` 与后续新增页面。
- 设计目标：深色协议质感、冷色高光、玻璃态卡片、强结构层级、低噪声交互。

## Design Tokens
- 文件：`frontend/design-tokens.css`
- 颜色：使用 `--ds-color-*` 语义色，不直接在组件中硬编码十六进制。
- 字体：`--ds-font-display`、`--ds-font-sans`、`--ds-font-mono`
- 间距：`--ds-space-1` 到 `--ds-space-12`，基于 4px 递增
- 圆角：`--ds-radius-xs/sm/md/lg/xl/pill`
- 阴影：`--ds-shadow-xs/sm/md/lg/glow/focus`
- 动效：`--ds-duration-fast/base/slow` + `--ds-ease-standard/emphasized/exit`
- 断点：`--ds-breakpoint-xs/md/lg/xl`
- 栅格：`--ds-grid-columns-*`、`--ds-grid-gutter-*`、`--ds-grid-margin-*`

## 字体系统
- Display：`Geist`
  - Hero：`56px / 1.05 / 300`
  - Section：`40px / 1.15 / 500`
- Body：`Inter`
  - 正文：`16px / 1.72 / 400`
  - 辅助：`14px / 1.55 / 400`
- Mono：`Geist Mono`
  - 代码块、ID、时间戳、消息原文

## 间距规范
- 基线：`4px`
- 常用间距：
  - XS：`4px`
  - SM：`8px`
  - MD：`16px`
  - LG：`24px`
  - XL：`32px`
  - 2XL：`48px`
  - Section：`80px`

## 组件规范
### 按钮
- 默认：
  - 主按钮：`--ds-button-primary-bg`
  - 次按钮：`--ds-button-secondary-bg`
- Hover：
  - 主按钮：`--ds-button-primary-bg-hover`
  - 次按钮：`--ds-button-secondary-bg-hover`
- Active：
  - 主按钮：`--ds-button-primary-bg-active`
  - 次按钮：`--ds-button-secondary-bg-active`
- Disabled：
  - 使用 `--ds-button-disabled-bg/border/text`

### 卡片
- 背景：`var(--ds-gradient-panel), rgba(11, 18, 32, 0.72)`
- 边框：`var(--ds-card-border)`
- 圆角：`var(--ds-card-radius)`
- 阴影：`var(--ds-card-shadow)`
- Hover 阴影：`var(--ds-card-shadow-hover)`

### 表单
- 输入框高度：`44px`
- 圆角：`12px`
- Focus：`var(--ds-shadow-focus)`
- 占位符：`--ds-color-text-disabled`

## 布局系统
- 桌面端：12 列栅格
- 平板端：8 列栅格
- 手机端：4 列栅格
- 推荐容器：
  - 页面容器：`--ds-container-page`
  - 文档容器：`--ds-container-docs`

## 图标规范
- 线性图标优先
- 默认线宽：`1.5px`
- 不使用强饱和彩色填充
- 在深色背景下使用浅描边或语义状态色

## 动效规范
- 默认过渡：`240ms` + `cubic-bezier(0.2, 0.8, 0.2, 1)`
- Hover 位移：不超过 `1px ~ 4px`
- Reduced Motion：关闭非必要动画与位移

## 响应式约束
- `320px` 起保障单手操作可读性
- 手机端优先底部标签栏
- 关键按钮避开 `safe-area`
- 双栏布局在 `900px` 以下折叠为单列

## 使用指南
- 新页面必须同时接入：
  - `style.css`
  - `design-tokens.css`
  - `design-system.css`
- 新组件应先补 token，再写组件层，不得直接写死颜色与间距。
