# Gaggle 设计系统维护指南

## 新页面接入流程
1. 在 HTML 页面中依次引入：
   - `style.css`
   - `design-tokens.css`
   - `design-system.css`
2. 优先复用现有组件类：
   - 按钮：`.btn-accent`、`.btn-ghost`、`.btn-sm`
   - 卡片：`.shell-card`、`.workspace-card`
   - 表单：`.login-input`、`.login-input-group`
   - 状态：`.space-badge`、`.role-badge`、`.chip`
3. 若现有 token 不够，先在 `design-tokens.css` 增加语义变量，再在 `design-system.css` 扩展组件。

## 禁止事项
- 禁止在新页面直接写死品牌色、边框色、圆角与阴影
- 禁止复制已有组件后另起一套命名和视觉规则
- 禁止绕过移动端 safe-area 与底部标签栏规则

## 推荐规则
- 标题优先使用 `Geist`
- 正文优先使用 `Inter`
- 代码、ID、时间戳优先使用 `Geist Mono`
- 所有页面优先中文本地化，保留必要英文术语
- 重要频道优先进入底部标签栏，次级页面通过堆栈式导航打开

## 组件扩展建议
### 新按钮
- 先判断是否属于主操作、次操作、轻操作
- 若不是这三类，再定义新的语义按钮变量，而不是直接复制样式

### 新卡片
- 先复用现有卡片 token：
  - `--ds-card-radius`
  - `--ds-card-border`
  - `--ds-card-shadow`
- 再根据业务差异追加内部结构样式

### 新表单
- 默认沿用 44px 高度控制项
- focus 态必须使用 `--ds-shadow-focus`
- placeholder 不得使用高对比度颜色

## 验收清单
- 是否接入 design tokens
- 是否复用统一按钮/卡片/表单/状态样式
- 是否覆盖移动端布局
- 是否检查长文本溢出、空状态、错误提示
- 是否补充到设计系统文档或更新日志

## 日志要求
- 大改前先记录当前情况
- 每轮更新补充到 `UPDATE_LOG.md`
- 重要专项单独新增状态文件，便于后续 coder 接手
