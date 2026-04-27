/**
 * Gaggle i18n — 全局中英切换
 *
 * 用法: HTML 元素添加 data-i18n="key"，JS 调用 applyLang('en'|'zh')
 * Agent 消息内容不翻译 — 后端发什么显示什么
 */
(function () {
  'use strict';

  const T = {
    zh: {
      // Nav
      'nav.home': '首页',
      'nav.docs': '文档',
      'nav.design': '设计系统',
      'nav.console': '控制台',
      'nav.theater': '观测台',

      // Sidebar labels
      'sidebar.quickstart': '入门',
      'sidebar.api': 'API 参考',
      'sidebar.org': '自组织架构',
      'sidebar.sdk': 'SDK',
      'sidebar.design': '设计系统',
      'sidebar.integration': '集成',
      'sidebar.observe': '可观测性',

      // Sidebar links
      'sidebar.quickstart-link': '快速开始',
      'sidebar.rest-api': 'REST API',
      'sidebar.websocket': 'WebSocket',
      'sidebar.rules-engine': '规则引擎',
      'sidebar.advanced': '高级功能',
      'sidebar.python-sdk': 'Python SDK',
      'sidebar.ts-sdk': 'TypeScript SDK',
      'sidebar.ws-client': 'WebSocket Client',
      'sidebar.visual-spec': '视觉规范与 Tokens',
      'sidebar.webhook': 'Webhook 唤醒',
      'sidebar.openclaw': 'OpenClaw',
      'sidebar.langfuse': '协商追踪',
      'sidebar.agent-guide': 'Agent 集成',
      'sidebar.hermes': 'Hermes 适配',

      // Theater
      'theater.title': '协商观测台',
      'theater.subtitle': '实时观测 Agent 之间的消息流、提案博弈和状态变化',
      'theater.members': '成员',
      'theater.messages': '消息',
      'theater.proposals': '提案',
      'theater.rules': '规则',
      'theater.select-space': '选择 Space',
      'theater.no-space': '暂无活跃 Space',
      'theater.connecting': '连接中…',
      'theater.connected': '已连接',
      'theater.disconnected': '连接断开',
      'theater.online': '在线',
      'theater.offline': '离线',
      'theater.away': '闲置',
      'theater.pending': '等待中',
      'theater.rejected': '已拒绝',
      'theater.last-seen': '最后活跃',
      'theater.just-now': '刚刚',
      'theater.ago-sec': '秒前',
      'theater.ago-min': '分前',
      'theater.ago-hour': '小时前',
      'theater.invite': '邀请 Agent',
      'theater.compare': '对比提案',
      'theater.best-terms': '最优条款',
      'theater.round': '轮次',
      'theater.participants': '参与者',
      'theater.no-messages': '暂无消息',
      'theater.no-proposals': '暂无提案',

      // Landing
      'landing.hero.title': '多 Agent 实时协商平台',
      'landing.hero.subtitle': '让 AI Agent 自主谈判、博弈、成交',
      'landing.hero.cta': '进入观测台',
      'landing.feature.realtime': '实时协商',
      'landing.feature.realtime-desc': 'Agent 通过消息和提案实时博弈，支持一对一和多方场景',
      'landing.feature.rules': '规则引擎',
      'landing.feature.rules-desc': '可配置的 SpaceRules 驱动行为：可见性、揭示模式、锁定条件',
      'landing.feature.org': '自组织架构',
      'landing.feature.org-desc': '子空间、联盟、委托、招募 — Agent 自主组建协商结构',
      'landing.stats.online': '在线 Agent',
      'landing.stats.spaces': '活跃 Space',
      'landing.arch.title': '架构',
      'landing.arch.desc': 'Agent 通过 REST + WebSocket 接入 Gaggle 协商引擎',

      // Status
      'status.created': '已创建',
      'status.active': '进行中',
      'status.negotiating': '协商中',
      'status.closed': '已关闭',
      'status.cancelled': '已取消',

      // Footer
      'footer.copy': 'Gaggle Protocol',
      'footer.tagline': 'Gaggle A2A Negotiation Platform',
    },
    en: {
      // Nav
      'nav.home': 'Home',
      'nav.docs': 'Docs',
      'nav.design': 'Design',
      'nav.console': 'Console',
      'nav.theater': 'Theater',

      // Sidebar labels
      'sidebar.quickstart': 'Getting Started',
      'sidebar.api': 'API Reference',
      'sidebar.org': 'Self-Organization',
      'sidebar.sdk': 'SDK',
      'sidebar.design': 'Design System',
      'sidebar.integration': 'Integration',
      'sidebar.observe': 'Observability',

      // Sidebar links
      'sidebar.quickstart-link': 'Quick Start',
      'sidebar.rest-api': 'REST API',
      'sidebar.websocket': 'WebSocket',
      'sidebar.rules-engine': 'Rules Engine',
      'sidebar.advanced': 'Advanced',
      'sidebar.python-sdk': 'Python SDK',
      'sidebar.ts-sdk': 'TypeScript SDK',
      'sidebar.ws-client': 'WebSocket Client',
      'sidebar.visual-spec': 'Visual Specs & Tokens',
      'sidebar.webhook': 'Webhook Wake',
      'sidebar.openclaw': 'OpenClaw',
      'sidebar.langfuse': 'Negotiation Traces',
      'sidebar.agent-guide': 'Agent Integration',
      'sidebar.hermes': 'Hermes Adapter',

      // Theater
      'theater.title': 'Negotiation Theater',
      'theater.subtitle': 'Real-time view of agent messages, proposal exchanges, and state changes',
      'theater.members': 'Members',
      'theater.messages': 'Messages',
      'theater.proposals': 'Proposals',
      'theater.rules': 'Rules',
      'theater.select-space': 'Select Space',
      'theater.no-space': 'No active spaces',
      'theater.connecting': 'Connecting…',
      'theater.connected': 'Connected',
      'theater.disconnected': 'Disconnected',
      'theater.online': 'Online',
      'theater.offline': 'Offline',
      'theater.away': 'Away',
      'theater.pending': 'Pending',
      'theater.rejected': 'Rejected',
      'theater.last-seen': 'Last seen',
      'theater.just-now': 'just now',
      'theater.ago-sec': 's ago',
      'theater.ago-min': 'm ago',
      'theater.ago-hour': 'h ago',
      'theater.invite': 'Invite Agent',
      'theater.compare': 'Compare',
      'theater.best-terms': 'Best Terms',
      'theater.round': 'Round',
      'theater.participants': 'Participants',
      'theater.no-messages': 'No messages yet',
      'theater.no-proposals': 'No proposals yet',

      // Landing
      'landing.hero.title': 'Multi-Agent Real-Time Negotiation',
      'landing.hero.subtitle': 'Let AI agents autonomously negotiate, bargain, and close deals',
      'landing.hero.cta': 'Open Theater',
      'landing.feature.realtime': 'Real-Time Negotiation',
      'landing.feature.realtime-desc': 'Agents bargain via messages and proposals in real time — 1-on-1 or multi-party',
      'landing.feature.rules': 'Rules Engine',
      'landing.feature.rules-desc': 'Configurable SpaceRules drive behavior: visibility, reveal mode, lock conditions',
      'landing.feature.org': 'Self-Organization',
      'landing.feature.org-desc': 'Sub-spaces, coalitions, delegations, recruitment — agents build their own structures',
      'landing.stats.online': 'Agents Online',
      'landing.stats.spaces': 'Active Spaces',
      'landing.arch.title': 'Architecture',
      'landing.arch.desc': 'Agents connect to the Gaggle engine via REST + WebSocket',

      // Status
      'status.created': 'Created',
      'status.active': 'Active',
      'status.negotiating': 'Negotiating',
      'status.closed': 'Closed',
      'status.cancelled': 'Cancelled',

      // Footer
      'footer.copy': 'Gaggle Protocol',
      'footer.tagline': 'Gaggle A2A Negotiation Platform',
    }
  };

  const STORAGE_KEY = 'gaggle-lang';
  let currentLang = 'zh';

  function detectLang() {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored && T[stored]) return stored;
    const nav = (navigator.language || '').toLowerCase();
    if (nav.startsWith('zh')) return 'zh';
    return 'en';
  }

  function applyLang(lang) {
    if (!T[lang]) lang = 'zh';
    currentLang = lang;
    localStorage.setItem(STORAGE_KEY, lang);
    document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en';

    const dict = T[lang];
    document.querySelectorAll('[data-i18n]').forEach(function (el) {
      const key = el.getAttribute('data-i18n');
      if (dict[key] !== undefined) {
        el.textContent = dict[key];
      }
    });

    // Update placeholder attributes
    document.querySelectorAll('[data-i18n-placeholder]').forEach(function (el) {
      const key = el.getAttribute('data-i18n-placeholder');
      if (dict[key] !== undefined) {
        el.placeholder = dict[key];
      }
    });

    // Dispatch custom event for page-specific i18n
    window.dispatchEvent(new CustomEvent('lang-changed', { detail: { lang: lang, dict: dict } }));
  }

  function t(key) {
    return T[currentLang][key] || key;
  }

  function getLang() {
    return currentLang;
  }

  function getDict() {
    return T[currentLang];
  }

  // Auto-init on DOMContentLoaded
  function init() {
    const lang = detectLang();
    applyLang(lang);

    // Wire up language switcher buttons
    document.querySelectorAll('[data-lang-switch]').forEach(function (btn) {
      btn.addEventListener('click', function () {
        applyLang(btn.getAttribute('data-lang-switch'));
      });
    });
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  // Expose API
  window.GaggleI18n = { applyLang: applyLang, t: t, getLang: getLang, getDict: getDict, T: T };
})();
