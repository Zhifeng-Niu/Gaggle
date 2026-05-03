# Gaggle 浏览器兼容性报告

## 目标矩阵
- Chrome 最新版
- Safari 最新版
- Firefox 最新版
- Edge 最新版
- iOS Safari
- Android Chrome / WebView

## 当前实现依赖
- HTML5 语义结构
- CSS Variables
- `backdrop-filter`
- `position: sticky`
- `env(safe-area-inset-*)`
- `IntersectionObserver`
- `prefers-reduced-motion`

## 风险评估
### 低风险
- CSS Variables、Grid、Flex、Sticky
- 现代桌面浏览器与移动浏览器普遍支持

### 中风险
- `backdrop-filter`
  - 在部分 Android WebView 或低版本浏览器上效果可能降级
  - 当前设计在降级后仍可接受，卡片边框与底色仍可保持层级

### 中风险
- `env(safe-area-inset-*)`
  - 在不支持的浏览器中会回退到默认 `max()` 设定

### 低风险
- `IntersectionObserver`
  - 已在脚本中提供降级：不可用时直接显示内容

## 已完成验证
- 本地静态访问：
  - `/`
  - `/docs.html`
  - `/design-system.html`
- 编辑器诊断无新增错误
- `app.js` / `landing-bg.js` 语法检查通过

## 建议执行的人工回归
- Chrome：视觉基线与交互主验收
- Safari：导航吸顶、玻璃态、表单 focus
- Firefox：字体渲染、代码块、表格边框
- Edge：登录弹层、工作台切换、RFP 表单
- iOS Safari：底部标签栏、安全区、长文本折行
- Android Chrome：控制台表单、侧栏折叠、卡片 hover 降级

## 结论
- 当前实现适合现代浏览器环境。
- 若要达到更严格的发布标准，建议下一轮补充真实设备截图对比与多浏览器人工验收记录。
