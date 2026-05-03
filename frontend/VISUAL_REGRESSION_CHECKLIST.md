# Gaggle 视觉回归测试清单

## 页面基线
- Landing 首页
- 控制台工作台
- Provider 发现页
- 创建谈判 / 创建 RFP 页
- 信誉评分页
- 我的资料页
- 接入文档页
- 设计系统页

## 视觉检查项
- 颜色对比度符合 WCAG 2.1 AA
- Display 标题、正文、代码字体渲染一致
- 按钮默认 / 悬停 / 点击 / 禁用四态一致
- 输入框 focus ring 清晰且不刺眼
- 卡片圆角、边框、阴影层级一致
- 空状态、错误提示、Toast 视觉一致
- 长文本、长 ID、长 URL 不溢出
- 移动端底部标签栏与安全区留白正常

## 交互检查项
- 导航吸顶正常
- 语言切换正常
- 控制台 view tab 切换正常
- 登录 / 注册 / 创建 Agent 模态框状态正常
- Provider 搜索、Profile 保存、评分提交交互反馈正常
- Reduced Motion 下动画正确降级

## 浏览器矩阵
- Chrome
- Safari
- Firefox
- Edge
- iOS Safari
- Android Chrome / WebView

## 自动化建议
- Percy：用于静态页面截图回归
- Chromatic：若后续组件化到 React/Vue，可用于组件级视觉回归
- 每次提交至少保留一组：
  - Landing
  - Console
  - Docs
  - Design System

## 验收结论模板
- 版本：
- 检查时间：
- 检查环境：
- 发现问题：
- 是否阻塞发布：
