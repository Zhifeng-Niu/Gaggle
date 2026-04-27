// ── Gaggle — app.js ────────────────────────────────────
// Landing + Docs + 中文控制台（Provider 发现 / Profile / 创建谈判 / RFP / 信誉）

window.openBottomSheet = function(id) {
  var sheet = document.getElementById(id);
  if (!sheet) return;
  sheet.classList.add('open');
  var overlay = document.getElementById('bottomSheetOverlay');
  if (!overlay) {
    overlay = document.createElement('div');
    overlay.id = 'bottomSheetOverlay';
    overlay.className = 'bottom-sheet-overlay';
    document.body.appendChild(overlay);
    overlay.addEventListener('click', function() {
      document.querySelectorAll('.bottom-sheet.open').forEach(function(s) {
        s.classList.remove('open');
      });
      overlay.classList.remove('open');
    });
  }
  setTimeout(function() { overlay.classList.add('open'); }, 10);
};

window.closeBottomSheet = function(id) {
  var sheet = document.getElementById(id);
  if (sheet) sheet.classList.remove('open');
  var overlay = document.getElementById('bottomSheetOverlay');
  if (overlay) overlay.classList.remove('open');
};

(function () {
  'use strict';

  var REMOTE_BASE = 'http://106.15.228.101';
  var HEALTH_INTERVAL = 10000;

  var REFRESH_INTERVAL = 8000;

  var state = {
    apiBase: resolveApiBase(),
    wsBase: '',
    spaces: [],
    selectedSpaceId: null,
    currentFilter: 'all',
    ws: null,
    wsRetries: 0,
    currentUser: null,
    currentAgentId: null,
    userAgents: [],
    agentStatusInterval: null,
    healthTimer: null,
    refreshTimer: null,
    currentView: 'arena',
    discoveryResults: [],
    selectedProvider: null,
    providerReputation: null,
    rfpCandidates: [],
    currentReputation: null,
    currentProfile: null,
    agentNames: {},
    pendingMessages: {},
    needs: [],
    myNeeds: [],
    // Phase 3: RFP visualization
    currentRounds: null,
    rfpProposals: [],
    // Phase 4: 合同管理
    contracts: [],
    currentContract: null
  };

  state.wsBase = state.apiBase.replace(/^http/i, 'ws') + '/ws/v1/agents';

  var dom = {
    landingPage: document.getElementById('landingPage'),
    dashboardPage: document.getElementById('dashboardPage'),
    authModal: document.getElementById('loginOverlay'),
    loginForm: document.getElementById('loginForm'),
    registerForm: document.getElementById('registerForm'),
    loginEmail: document.getElementById('loginEmail'),
    loginPassword: document.getElementById('loginPassword'),
    loginBtn: document.getElementById('loginBtn'),
    loginSpinner: document.getElementById('loginSpinner'),
    loginError: document.getElementById('loginError'),
    authSubtitle: document.getElementById('authSubtitle'),
    regBtn: document.getElementById('regBtn'),
    regSpinner: document.getElementById('regSpinner'),
    regError: document.getElementById('regError'),
    showRegister: document.getElementById('showRegister'),
    showLogin: document.getElementById('showLogin'),
    createAgentModal: document.getElementById('createAgentModal'),
    createAgentForm: document.getElementById('createAgentForm'),
    closeAgentModal: document.getElementById('closeAgentModal'),
    agentResult: document.getElementById('agentResult'),
    agentSelector: document.getElementById('agentSelector'),
    agentSelect: document.getElementById('agentSelect'),
    agentStatusDot: document.getElementById('agentStatusDot'),
    logoutBtn: document.getElementById('logoutBtn'),
    healthDot: document.getElementById('healthDot'),
    healthText: document.getElementById('healthText'),
    spaceList: document.getElementById('spaceList'),
    emptyState: document.getElementById('emptyState'),
    detailView: document.getElementById('detailView'),
    detailTitle: document.getElementById('detailTitle'),
    detailInfo: document.getElementById('detailInfo'),
    detailClose: document.getElementById('detailClose'),
    messageList: document.getElementById('messageList'),
    msgCount: document.getElementById('msgCount'),
    proposalTimeline: document.getElementById('proposalTimeline'),
    proposalCount: document.getElementById('proposalCount'),
    toastContainer: document.getElementById('toastContainer'),
    mobileToggle: document.getElementById('mobileToggle'),
    navLinks: document.getElementById('navLinks'),
    createAgentBtn2: document.getElementById('createAgentBtn2'),
    sidebar: document.getElementById('sidebar'),
    viewButtons: Array.prototype.slice.call(document.querySelectorAll('[data-view]')),
    sidebarUserName: document.getElementById('sidebarUserName'),
    sidebarUserMeta: document.getElementById('sidebarUserMeta'),
    sidebarAgentSummary: document.getElementById('sidebarAgentSummary'),
    statTotalSpaces: document.getElementById('statTotalSpaces'),
    statActiveSpaces: document.getElementById('statActiveSpaces'),
    statProviders: document.getElementById('statProviders'),
    statReputation: document.getElementById('statReputation'),
    overviewSpaces: document.getElementById('overviewSpaces'),
    selectedSpaceSummary: document.getElementById('selectedSpaceSummary'),
    quickCreateNegotiation: document.getElementById('quickCreateNegotiation'),
    quickCreateRfp: document.getElementById('quickCreateRfp'),
    quickDiscovery: document.getElementById('quickDiscovery'),
    overviewRefreshSpaces: document.getElementById('overviewRefreshSpaces'),
    focusCreateRating: document.getElementById('focusCreateRating'),
    providerSearchForm: document.getElementById('providerSearchForm'),
    providerSkills: document.getElementById('providerSkills'),
    providerCategory: document.getElementById('providerCategory'),
    providerAvailability: document.getElementById('providerAvailability'),
    providerMinPrice: document.getElementById('providerMinPrice'),
    providerMaxPrice: document.getElementById('providerMaxPrice'),
    loadAllProviders: document.getElementById('loadAllProviders'),
    providerResultCount: document.getElementById('providerResultCount'),
    providerResults: document.getElementById('providerResults'),
    providerDetail: document.getElementById('providerDetail'),
    createSpaceForm: document.getElementById('createSpaceForm'),
    createSpaceName: document.getElementById('createSpaceName'),
    bilateralInviteeId: document.getElementById('bilateralInviteeId'),
    myRole: document.getElementById('myRole'),
    createSpaceBudget: document.getElementById('createSpaceBudget'),
    createSpaceDescription: document.getElementById('createSpaceDescription'),
    createSpaceTerms: document.getElementById('createSpaceTerms'),
    useSelectedProvider: document.getElementById('useSelectedProvider'),
    createRfpForm: document.getElementById('createRfpForm'),
    createRfpName: document.getElementById('createRfpName'),
    rfpRounds: document.getElementById('rfpRounds'),
    rfpProviderIds: document.getElementById('rfpProviderIds'),
    rfpCriteria: document.getElementById('rfpCriteria'),
    rfpDeadline: document.getElementById('rfpDeadline'),
    rfpBudget: document.getElementById('rfpBudget'),
    rfpRequirements: document.getElementById('rfpRequirements'),
    rfpShareBestTerms: document.getElementById('rfpShareBestTerms'),
    syncRfpCandidates: document.getElementById('syncRfpCandidates'),
    rfpCandidateList: document.getElementById('rfpCandidateList'),
    reputationScoreValue: document.getElementById('reputationScoreValue'),
    reputationRatingValue: document.getElementById('reputationRatingValue'),
    reputationFulfillmentValue: document.getElementById('reputationFulfillmentValue'),
    reputationTotalValue: document.getElementById('reputationTotalValue'),
    reputationEvents: document.getElementById('reputationEvents'),
    rateAgentForm: document.getElementById('rateAgentForm'),
    rateSpaceSelect: document.getElementById('rateSpaceSelect'),
    rateTargetSelect: document.getElementById('rateTargetSelect'),
    rateEventType: document.getElementById('rateEventType'),
    rateOutcome: document.getElementById('rateOutcome'),
    rateValue: document.getElementById('rateValue'),
    rateHint: document.getElementById('rateHint'),
    profileUserCard: document.getElementById('profileUserCard'),
    profileAgentCard: document.getElementById('profileAgentCard'),
    providerProfileNotice: document.getElementById('providerProfileNotice'),
    providerProfileForm: document.getElementById('providerProfileForm'),
    profileDisplayName: document.getElementById('profileDisplayName'),
    profileCategory: document.getElementById('profileCategory'),
    profileDescription: document.getElementById('profileDescription'),
    profileSkills: document.getElementById('profileSkills'),
    profileTags: document.getElementById('profileTags'),
    profilePricing: document.getElementById('profilePricing'),
    profileAvailability: document.getElementById('profileAvailability'),
    profileMinPrice: document.getElementById('profileMinPrice'),
    profileMaxPrice: document.getElementById('profileMaxPrice'),
    publishNeedForm: document.getElementById('publishNeedForm'),
    needTitle: document.getElementById('needTitle'),
    needCategory: document.getElementById('needCategory'),
    needDescription: document.getElementById('needDescription'),
    needSkills: document.getElementById('needSkills'),
    needDeadline: document.getElementById('needDeadline'),
    needBudgetMin: document.getElementById('needBudgetMin'),
    needBudgetMax: document.getElementById('needBudgetMax'),
    refreshMyNeeds: document.getElementById('refreshMyNeeds'),
    myNeedsList: document.getElementById('myNeedsList'),
    needSearchForm: document.getElementById('needSearchForm'),
    needSearchCategory: document.getElementById('needSearchCategory'),
    needSearchSkills: document.getElementById('needSearchSkills'),
    loadAllNeeds: document.getElementById('loadAllNeeds'),
    needResultCount: document.getElementById('needResultCount'),
    needResults: document.getElementById('needResults'),
    // Phase 3: RFP Negotiation Visualization
    rfpVizSection: document.getElementById('rfpVizSection'),
    roundProgressFill: document.getElementById('roundProgressFill'),
    roundProgressLabel: document.getElementById('roundProgressLabel'),
    roundStatusLabel: document.getElementById('roundStatusLabel'),
    roundDeadlineInfo: document.getElementById('roundDeadlineInfo'),
    advanceRoundBtn: document.getElementById('advanceRoundBtn'),
    rfpProposalTableBody: document.getElementById('rfpProposalTableBody'),
    rfpProposalTableWrap: document.getElementById('rfpProposalTableWrap'),
    weightPrice: document.getElementById('weightPrice'),
    weightTimeline: document.getElementById('weightTimeline'),
    weightQuality: document.getElementById('weightQuality'),
    weightPriceValue: document.getElementById('weightPriceValue'),
    weightTimelineValue: document.getElementById('weightTimelineValue'),
    weightQualityValue: document.getElementById('weightQualityValue'),
    evaluateBtn: document.getElementById('evaluateBtn'),
    evaluateResultTableWrap: document.getElementById('evaluateResultTableWrap'),
    evaluateResultTableBody: document.getElementById('evaluateResultTableBody'),
    createRfpFromNeedModal: document.getElementById('createRfpFromNeedModal'),
    createRfpFromNeedForm: document.getElementById('createRfpFromNeedForm'),
    closeCreateRfpFromNeed: document.getElementById('closeCreateRfpFromNeed'),
    cancelRfpFromNeed: document.getElementById('cancelRfpFromNeed'),
    rfpFromNeedId: document.getElementById('rfpFromNeedId'),
    rfpFromNeedProviders: document.getElementById('rfpFromNeedProviders'),
    rfpFromNeedRounds: document.getElementById('rfpFromNeedRounds'),
    rfpFromNeedBudget: document.getElementById('rfpFromNeedBudget'),
    // Phase 4: 合同管理面板
    contractsModal: document.getElementById('contractsModal'),
    closeContractsModal: document.getElementById('closeContractsModal'),
    contractsList: document.getElementById('contractsList'),
    contractDetailPanel: document.getElementById('contractDetailPanel'),
    contractDetailClose: document.getElementById('contractDetailClose'),
    milestoneEditorModal: document.getElementById('milestoneEditorModal'),
    closeMilestoneEditor: document.getElementById('closeMilestoneEditor'),
    milestoneEditorForm: document.getElementById('milestoneEditorForm'),
    milestoneEditorList: document.getElementById('milestoneEditorList'),
    addMilestoneRow: document.getElementById('addMilestoneRow'),
    disputeModal: document.getElementById('disputeModal'),
    closeDisputeModal: document.getElementById('closeDisputeModal'),
    disputeForm: document.getElementById('disputeForm'),
    disputeReason: document.getElementById('disputeReason'),
    createContractBtn: document.getElementById('createContractBtn')
  };

  function resolveApiBase() {
    var saved = localStorage.getItem('gaggle_api_base');
    if (saved) return saved.replace(/\/$/, '');
    var host = window.location.hostname;
    if (host === 'localhost' || host === '127.0.0.1') return 'http://127.0.0.1:8080';
    if (/^https?:/.test(window.location.origin) && window.location.origin !== 'null') {
      return window.location.origin.replace(/\/$/, '');
    }
    return REMOTE_BASE;
  }

  function isConsolePage() {
    return !!dom.dashboardPage && !!dom.landingPage;
  }

  function safeText(value) {
    return value == null || value === '' ? '—' : String(value);
  }

  function shortId(id) {
    if (!id) return '—';
    return id.length > 12 ? id.slice(0, 8) + '…' : id;
  }

  function formatTime(ts) {
    if (!ts) return '—';
    var date = new Date(ts);
    if (isNaN(date.getTime())) return '—';
    return date.toLocaleString('zh-CN', {
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit'
    });
  }

  function formatScore(value) {
    return typeof value === 'number' && !isNaN(value) ? value.toFixed(1) : '0.0';
  }

  function formatPercent(value) {
    return typeof value === 'number' && !isNaN(value) ? Math.round(value * 100) + '%' : '0%';
  }

  function formatPrice(min, max) {
    if (min == null && max == null) return '价格面议';
    if (min != null && max != null) return min + ' - ' + max;
    if (min != null) return '≥ ' + min;
    return '≤ ' + max;
  }

  function translateAgentType(type) {
    return type === 'provider' ? '卖方' : type === 'consumer' ? '买方' : safeText(type);
  }

  function translateRole(role) {
    return role === 'buyer' ? '买方' : role === 'seller' ? '卖方' : safeText(role);
  }

  function translateSpaceType(type) {
    return type === 'rfp' ? '招标空间' : '双边协商';
  }

  function translateStatus(status) {
    var map = {
      created: '已创建',
      active: '进行中',
      concluded: '已完成',
      cancelled: '已取消',
      expired: '已过期',
      pending: '待处理',
      accepted: '已接受',
      rejected: '已拒绝',
      superseded: '已替代'
    };
    return map[status] || safeText(status);
  }

  function translateMessageType(type) {
    var map = {
      text: '文本',
      proposal: '提案',
      counter_proposal: '还价',
      acceptance: '接受',
      rejection: '拒绝',
      withdrawal: '撤回',
      attachment: '附件',
      system: '系统'
    };
    return map[type] || safeText(type || 'text');
  }

  function translateProposalType(type) {
    var map = {
      initial: '初始提案',
      counter: '还价提案',
      best_and_final: '最终报价'
    };
    return map[type] || safeText(type || 'initial');
  }

  function translateAvailability(status) {
    var map = {
      available: '可接单',
      busy: '忙碌',
      offline: '离线',
      unknown: '未知'
    };
    return map[status] || safeText(status);
  }

  function translateOutcome(outcome) {
    var map = { success: '成功', partial: '部分完成', failure: '失败' };
    return map[outcome] || safeText(outcome);
  }

  function translateEventType(type) {
    var map = { concluded: '达成', cancelled: '取消', breach: '违约' };
    return map[type] || safeText(type);
  }

  function translateNeedCategory(category) {
    var map = {
      supply_chain: '供应链',
      data_analysis: '数据分析',
      content_creation: '内容创作',
      software_dev: '软件开发',
      marketing: '营销推广',
      finance: '金融服务',
      logistics: '物流仓储',
      manufacturing: '制造加工',
      consulting: '咨询服务',
      other: '其他'
    };
    return map[category] || safeText(category);
  }

  function translateNeedStatus(status) {
    var map = { open: '开放', matched: '已匹配', expired: '已过期', cancelled: '已取消' };
    return map[status] || safeText(status);
  }

  function currentAgent() {
    return state.userAgents.find(function (agent) {
      return agent.id === state.currentAgentId;
    }) || null;
  }

  function el(tag, attrs, children) {
    var node = document.createElement(tag);
    if (attrs) {
      Object.keys(attrs).forEach(function (key) {
        var value = attrs[key];
        if (key === 'className') node.className = value;
        else if (key === 'textContent') node.textContent = value;
        else if (key === 'innerHTML') node.innerHTML = value;
        else if (key === 'dataset') Object.keys(value).forEach(function (dataKey) { node.dataset[dataKey] = value[dataKey]; });
        else if (key.indexOf('on') === 0) node.addEventListener(key.slice(2).toLowerCase(), value);
        else node.setAttribute(key, value);
      });
    }
    if (children) {
      (Array.isArray(children) ? children : [children]).forEach(function (child) {
        if (typeof child === 'string') node.appendChild(document.createTextNode(child));
        else if (child && child.nodeType) node.appendChild(child);
      });
    }
    return node;
  }

  function toast(message, type) {
    if (!dom.toastContainer) return;
    var item = el('div', { className: 'toast ' + (type || 'info'), textContent: message });
    dom.toastContainer.appendChild(item);
    setTimeout(function () {
      item.style.opacity = '0';
      item.style.transform = 'translateX(20px)';
      setTimeout(function () { item.remove(); }, 300);
    }, 3500);
  }

  function request(path, opts) {
    opts = opts || {};
    var headers = Object.assign({ 'Content-Type': 'application/json' }, opts.headers || {});
    if (opts.token) headers.Authorization = 'Bearer ' + opts.token;
    else {
      var userToken = localStorage.getItem('gaggle_api_key');
      if (userToken) headers.Authorization = 'Bearer ' + userToken;
    }
    return fetch(state.apiBase + path, {
      method: opts.method || 'GET',
      headers: headers,
      body: opts.body
    }).then(function (res) {
      if (res.status === 401 && !opts.allowUnauthorized) {
        showLanding();
        throw new Error('未授权，请重新登录');
      }
      return res.text().then(function (text) {
        var data = null;
        try { data = text ? JSON.parse(text) : null; } catch (_) { data = text; }
        if (!res.ok) throw new Error(typeof data === 'string' ? data : (data && data.message) || ('请求失败：' + res.status));
        return data;
      });
    });
  }

  function notifyNegotiation(eventType, data) {
    var spaceName = data && data.payload && data.payload.space ? data.payload.space.name : shortId(data && data.space_id);
    var messages = {
      space_created: '已创建协商空间：' + spaceName,
      rfp_created: '已发布 RFP：' + spaceName,
      space_joined: '有 Agent 加入空间 ' + shortId(data.space_id),
      space_left: '有 Agent 离开空间 ' + shortId(data.space_id),
      new_message: '空间 ' + shortId(data.space_id) + ' 收到新消息',
      new_proposal: '空间 ' + shortId(data.space_id) + ' 收到新提案',
      proposal_update: '空间 ' + shortId(data.space_id) + ' 的提案状态已更新',
      best_terms_shared: '空间 ' + shortId(data.space_id) + ' 已同步最佳条款',
      space_status_changed: '空间状态更新：' + translateStatus(data.payload && data.payload.old_status) + ' → ' + translateStatus(data.payload && data.payload.new_status),
      space_closed: '空间已关闭：' + translateStatus(data.payload && data.payload.conclusion),
      need_published: '新需求发布：' + ((data.payload && data.payload.need && data.payload.need.title) || '未知需求'),
      need_matched: '需求已匹配 Provider',
      need_cancelled: '需求已取消'
    };
    if (messages[eventType]) toast(messages[eventType], eventType === 'error' ? 'error' : 'success');
  }

  function showLanding() {
    if (dom.landingPage) dom.landingPage.style.display = '';
    if (dom.dashboardPage) dom.dashboardPage.style.display = 'none';
    if (state.ws) {
      state.ws.onopen = null;
      state.ws.onclose = null;
      state.ws.onmessage = null;
      state.ws.onerror = null;
      state.ws.close();
      state.ws = null;
    }
    if (state.agentStatusInterval) clearInterval(state.agentStatusInterval);
    stopAutoRefresh();
  }

  function showDashboard() {
    dom.landingPage.style.display = 'none';
    dom.dashboardPage.style.display = '';
  }

  function openAuthModal(mode) {
    dom.authModal.style.display = 'flex';
    if (mode === 'register') {
      dom.loginForm.style.display = 'none';
      dom.registerForm.style.display = '';
      dom.authSubtitle.textContent = '创建您的账号';
    } else {
      dom.loginForm.style.display = '';
      dom.registerForm.style.display = 'none';
      dom.authSubtitle.textContent = '登录您的账号';
    }
    dom.loginError.textContent = '';
    dom.regError.textContent = '';
  }

  function closeAuthModal() {
    dom.authModal.style.display = 'none';
  }

  function handleLogin(event) {
    event.preventDefault();
    var email = dom.loginEmail.value.trim();
    var password = dom.loginPassword.value;
    if (!email || !password) return;
    dom.loginBtn.disabled = true;
    dom.loginSpinner.style.display = 'inline-block';
    dom.loginError.textContent = '';
    request('/api/v1/users/login', {
      method: 'POST',
      body: JSON.stringify({ email: email, password: password }),
      allowUnauthorized: true
    }).then(function (data) {
      localStorage.setItem('gaggle_api_key', data.api_key);
      closeAuthModal();
      completeAuth();
    }).catch(function () {
      dom.loginError.textContent = '邮箱或密码错误';
    }).finally(function () {
      dom.loginBtn.disabled = false;
      dom.loginSpinner.style.display = 'none';
    });
  }

  function handleRegister(event) {
    event.preventDefault();
    var displayName = document.getElementById('regDisplayName').value.trim();
    var email = document.getElementById('regEmail').value.trim();
    var password = document.getElementById('regPassword').value;
    if (!displayName || !email || !password) return;
    dom.regBtn.disabled = true;
    dom.regSpinner.style.display = 'inline-block';
    dom.regError.textContent = '';
    request('/api/v1/users/register', {
      method: 'POST',
      body: JSON.stringify({ display_name: displayName, email: email, password: password }),
      allowUnauthorized: true
    }).then(function (data) {
      localStorage.setItem('gaggle_api_key', data.api_key);
      closeAuthModal();
      completeAuth();
    }).catch(function (err) {
      dom.regError.textContent = err.message || '注册失败';
    }).finally(function () {
      dom.regBtn.disabled = false;
      dom.regSpinner.style.display = 'none';
    });
  }

  function completeAuth() {
    request('/api/v1/users/me').then(function (user) {
      state.currentUser = user;
      showDashboard();
      renderSessionSummary();
      loadUserAgents();
    }).catch(function () {
      localStorage.removeItem('gaggle_api_key');
      showLanding();
    });
  }

  function checkAuth() {
    var key = localStorage.getItem('gaggle_api_key');
    if (!key) return showLanding();
    completeAuth();
  }

  function handleLogout() {
    localStorage.removeItem('gaggle_api_key');
    state.currentUser = null;
    state.currentAgentId = null;
    state.userAgents = [];
    showLanding();
  }

  function renderSessionSummary() {
    if (!state.currentUser) return;
    dom.sidebarUserName.textContent = state.currentUser.display_name || 'Gaggle 用户';
    dom.sidebarUserMeta.textContent = (state.currentUser.email || '') + ' · API ' + state.apiBase;
  }

  function loadUserAgents() {
    request('/api/v1/users/me/agents').then(function (agents) {
      state.userAgents = Array.isArray(agents) ? agents : [];
      dom.agentSelect.textContent = '';
      if (!state.userAgents.length) {
        dom.agentSelector.style.display = 'none';
        dom.spaceList.innerHTML = '<p style="padding:16px;color:var(--text-muted)">暂无 Agent，请先创建。</p>';
        dom.sidebarAgentSummary.innerHTML = '<span class="space-badge created">请先创建 Agent</span>';
        renderProfileCards();
        return;
      }
      dom.agentSelector.style.display = '';
      state.userAgents.forEach(function (agent) {
        dom.agentSelect.appendChild(el('option', {
          value: agent.id,
          textContent: agent.name + '（' + translateAgentType(agent.agent_type) + '）'
        }));
      });
      selectAgent(state.currentAgentId || state.userAgents[0].id);
    }).catch(function (err) {
      toast('加载 Agent 列表失败：' + err.message, 'error');
    });
  }

  function selectAgent(agentId) {
    state.currentAgentId = agentId;
    dom.agentSelect.value = agentId;
    var agent = currentAgent();
    renderProfileCards();
    renderSessionSummary();
    if (agent) {
      dom.sidebarAgentSummary.innerHTML = '';
      dom.sidebarAgentSummary.appendChild(el('span', {
        className: 'space-badge ' + (agent.agent_type === 'provider' ? 'active' : 'created'),
        textContent: translateAgentType(agent.agent_type)
      }));
      dom.sidebarAgentSummary.appendChild(el('span', {
        className: 'sidebar-agent-name',
        textContent: agent.name + ' · ' + shortId(agent.id)
      }));
    }
    if (state.agentStatusInterval) clearInterval(state.agentStatusInterval);
    updateAgentStatus();
    state.agentStatusInterval = setInterval(updateAgentStatus, 15000);
    connectWS();
    loadSpaces();
    loadDiscoveryProviders();
    loadCurrentReputation();
    loadProviderProfile();
    loadNeeds();
    loadMyNeeds();
    startAutoRefresh();
  }

  function updateAgentStatus() {
    if (!state.currentAgentId) return;
    request('/api/v1/agents/' + state.currentAgentId + '/status', { allowUnauthorized: true })
      .then(function (status) {
        dom.agentStatusDot.className = 'agent-status-dot ' + (status.online ? 'online' : 'offline');
      })
      .catch(function () {
        dom.agentStatusDot.className = 'agent-status-dot offline';
      });
  }

  function handleCreateAgent(event) {
    event.preventDefault();
    var name = document.getElementById('agentName').value.trim();
    var type = document.getElementById('agentType').value;
    var desc = document.getElementById('agentDesc').value.trim();
    var org = document.getElementById('agentOrg').value.trim();
    if (!name) return;
    request('/api/v1/agents/register', {
      method: 'POST',
      body: JSON.stringify({
        name: name,
        agent_type: type,
        organization: org || null,
        metadata: desc ? { description: desc } : {}
      })
    }).then(function (data) {
      dom.agentResult.style.display = '';
      dom.agentResult.querySelector('.login-success').textContent = 'Agent 创建成功！';
      document.getElementById('resultAgentId').textContent = data.id;
      document.getElementById('resultApiKey').textContent = data.api_key;
      document.getElementById('resultApiSecret').textContent = data.api_secret;
      document.getElementById('resultPlatformUrl').textContent = state.apiBase;
      toast('已创建 Agent「' + name + '」，请复制凭证后再关闭', 'success');
      loadUserAgents();
    }).catch(function (err) {
      toast('创建 Agent 失败：' + err.message, 'error');
    });
  }

  function setHealth(status) {
    dom.healthDot.className = 'health-dot ' + (status === 'connected' ? 'connected' : status === 'connecting' ? 'connecting' : '');
    dom.healthText.textContent = status === 'connected' ? '已连接' : status === 'connecting' ? '连接中...' : '离线';
  }

  function checkHealth() {
    fetch(state.apiBase + '/health')
      .then(function (res) { setHealth(res.ok ? 'connected' : 'disconnected'); })
      .catch(function () { setHealth('disconnected'); });
  }

  function loadSpaces() {
    if (!state.currentAgentId) return;
    request('/api/v1/agents/' + state.currentAgentId + '/spaces').then(function (spaces) {
      state.spaces = Array.isArray(spaces) ? spaces : [];
      renderSpaceList();
      renderOverview();
      populateRateOptions();
      if (state.selectedSpaceId) {
        var exists = state.spaces.some(function (space) { return space.id === state.selectedSpaceId; });
        if (!exists) closeSpaceDetail();
      }
    }).catch(function (err) {
      dom.spaceList.innerHTML = '<p style="padding:16px;color:var(--text-muted)">加载空间失败：' + err.message + '</p>';
    });
  }

  function filteredSpaces() {
    return state.spaces.filter(function (space) {
      if (state.currentFilter === 'all') return true;
      if (state.currentFilter === 'buyer') return space.buyer_id === state.currentAgentId;
      if (state.currentFilter === 'seller') return space.seller_id === state.currentAgentId;
      return space.status === state.currentFilter;
    });
  }

  function renderSpaceList() {
    dom.spaceList.textContent = '';
    var list = filteredSpaces();
    if (!list.length) {
      dom.spaceList.appendChild(el('p', {
        textContent: state.spaces.length ? '没有符合筛选条件的空间。' : '当前还没有协商空间。',
        style: 'padding:16px;color:var(--text-muted)'
      }));
      return;
    }
    list.sort(function (a, b) { return (b.updated_at || 0) - (a.updated_at || 0); });
    list.forEach(function (space) {
      var card = el('div', {
        className: 'space-card' + (space.id === state.selectedSpaceId ? ' selected' : ''),
        onclick: function () { selectSpace(space.id); }
      });
      var header = el('div', { className: 'space-card-header' });
      header.appendChild(el('span', { className: 'space-name', textContent: space.name || '未命名空间' }));
      header.appendChild(el('span', {
        className: 'space-badge ' + (space.status || 'created'),
        textContent: translateStatus(space.status || 'created')
      }));
      card.appendChild(header);
      if (space.buyer_id === state.currentAgentId || space.seller_id === state.currentAgentId) {
        card.appendChild(el('div', {
          className: 'space-meta',
          innerHTML: '<span class="space-type">' + translateSpaceType(space.space_type) + '</span><span class="space-time">' + formatTime(space.updated_at || space.created_at) + '</span>'
        }));
      }
      var role = space.buyer_id === state.currentAgentId ? 'buyer' : (space.seller_id === state.currentAgentId ? 'seller' : null);
      if (role) {
        card.appendChild(el('div', { className: 'space-agents' }, [
          el('span', { className: 'role-badge ' + role, textContent: translateRole(role) })
        ]));
      }
      dom.spaceList.appendChild(card);
    });
  }

  function renderOverview() {
    var active = state.spaces.filter(function (space) { return space.status === 'active'; }).length;
    dom.statTotalSpaces.textContent = String(state.spaces.length);
    dom.statActiveSpaces.textContent = String(active);
    dom.statProviders.textContent = String(state.discoveryResults.length);
    dom.statReputation.textContent = state.currentReputation && state.currentReputation.summary ? formatScore(state.currentReputation.summary.reputation_score) : '0.0';
    dom.overviewSpaces.textContent = '';
    var list = state.spaces.slice().sort(function (a, b) {
      return (b.updated_at || b.created_at || 0) - (a.updated_at || a.created_at || 0);
    }).slice(0, 5);
    if (!list.length) {
      dom.overviewSpaces.appendChild(el('p', { textContent: '暂无空间，可先从发现页选择 Provider 后创建谈判。' }));
      return;
    }
    list.forEach(function (space) {
      var item = el('button', {
        className: 'overview-row' + (space.id === state.selectedSpaceId ? ' active' : ''),
        type: 'button',
        onclick: function () { selectSpace(space.id); }
      });
      item.appendChild(el('span', { className: 'overview-row-title', textContent: space.name || '未命名空间' }));
      item.appendChild(el('span', { className: 'overview-row-meta', textContent: translateSpaceType(space.space_type) + ' · ' + translateStatus(space.status) + ' · ' + formatTime(space.updated_at || space.created_at) }));
      dom.overviewSpaces.appendChild(item);
    });
  }

  function selectSpace(spaceId) {
    state.selectedSpaceId = spaceId;
    renderSpaceList();
    renderOverview();
    var space = state.spaces.find(function (item) { return item.id === spaceId; });
    if (!space) return;
    dom.emptyState.style.display = 'none';
    dom.detailView.classList.add('visible');
    dom.detailTitle.textContent = space.name || '未命名空间';
    renderSelectedSpaceSummary(space);
    dom.detailInfo.textContent = '';
    [
      { label: '类型', value: translateSpaceType(space.space_type) },
      { label: '状态', value: translateStatus(space.status) },
      { label: '创建时间', value: formatTime(space.created_at) },
      { label: '参与 Agent', value: (space.agent_ids || []).map(shortId).join('、') },
      { label: '创建者', value: shortId(space.creator_id) }
    ].forEach(function (item) {
      var wrapper = el('div', { className: 'detail-info-item' });
      wrapper.appendChild(el('span', { className: 'detail-info-label', textContent: item.label + '：' }));
      wrapper.appendChild(el('span', { textContent: item.value }));
      dom.detailInfo.appendChild(wrapper);
    });
    // 预加载 space 参与者名称
    loadSpaceParticipantNames(space);
    loadMessages(spaceId);
    loadProposals(spaceId);
    loadRfpVisualization(spaceId, space);
    flushPendingMessages(spaceId);
    if (window.innerWidth <= 768) dom.sidebar.classList.remove('open');
  }

  function renderSelectedSpaceSummary(space) {
    var contextText = typeof space.context === 'string' ? space.context : JSON.stringify(space.context || {}, null, 2);
    dom.selectedSpaceSummary.className = 'selected-space-summary';
    dom.selectedSpaceSummary.innerHTML =
      '<div class="summary-grid">' +
      '<div><span>空间 ID</span><strong>' + shortId(space.id) + '</strong></div>' +
      '<div><span>角色</span><strong>' + translateRole(space.buyer_id === state.currentAgentId ? 'buyer' : (space.seller_id === state.currentAgentId ? 'seller' : '')) + '</strong></div>' +
      '<div><span>更新时间</span><strong>' + formatTime(space.updated_at || space.created_at) + '</strong></div>' +
      '<div><span>参与成员</span><strong>' + (space.agent_ids || []).length + ' 个</strong></div>' +
      '</div>' +
      '<pre class="summary-context">' + contextText + '</pre>';
  }

  function closeSpaceDetail() {
    state.selectedSpaceId = null;
    state.currentRounds = null;
    if (dom.rfpVizSection) dom.rfpVizSection.style.display = 'none';
    dom.detailView.classList.remove('visible');
    dom.emptyState.style.display = '';
    dom.selectedSpaceSummary.className = 'selected-space-summary empty';
    dom.selectedSpaceSummary.innerHTML = '<p>从左侧选择一个空间，查看成员、上下文、消息流与提案时间线。</p>';
    renderSpaceList();
    renderOverview();
  }

  function resolveContent(content) {
    if (!content) return '[无内容]';
    if (typeof content === 'string') return content;
    // 旧格式（加密对象）向后兼容
    if (typeof content === 'object' && content.cipher) return '[历史加密消息]';
    return JSON.stringify(content, null, 2);
  }

  function isScrolledToBottom() {
    var el = dom.messageList;
    return el.scrollHeight - el.scrollTop - el.clientHeight < 60;
  }

  function loadMessages(spaceId, incremental) {
    request('/api/v1/spaces/' + spaceId + '/messages?limit=200').then(function (messages) {
      var list = Array.isArray(messages) ? messages : [];
      if (!incremental || !dom.messageList.querySelector('.message-bubble')) {
        renderMessages(list);
        return;
      }
      var existingIds = {};
      dom.messageList.querySelectorAll('.message-bubble').forEach(function (node) {
        if (node.dataset.msgId) existingIds[node.dataset.msgId] = true;
      });
      var newOnes = list.filter(function (m) { return !existingIds[m.id]; });
      if (newOnes.length) appendNewMessages(newOnes, spaceId, true);
      dom.msgCount.textContent = '(' + list.length + ')';
    }).catch(function (err) {
      if (!incremental) {
        dom.messageList.innerHTML = '<div class="empty-state"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/></svg><h3>加载失败</h3><p>' + safeText(err.message) + '</p></div>';
      }
    });
  }

  function renderMessages(messages) {
    dom.msgCount.textContent = '(' + messages.length + ')';
    dom.messageList.textContent = '';
    if (!messages.length) {
      dom.messageList.innerHTML = '<div class="empty-state"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg><h3>暂无消息</h3><p>该协商空间暂无消息。</p></div>';
      return;
    }
    var selfId = state.currentAgentId;
    messages.forEach(function (message) {
      var text = resolveContent(message.content);
      var bubble = buildBubble(message, text, selfId);
      bubble.dataset.msgId = message.id || '';
      dom.messageList.appendChild(bubble);
    });
    dom.messageList.scrollTop = dom.messageList.scrollHeight;
  }

  function appendNewMessages(messages, spaceId, animate) {
    var selfId = state.currentAgentId || '';
    var atBottom = isScrolledToBottom();
    messages.forEach(function (message) {
      var text = resolveContent(message.content);
      var bubble = buildBubble(message, text, selfId);
      bubble.dataset.msgId = message.id || '';
      if (animate) bubble.classList.add('msg-pop-in');
      dom.messageList.appendChild(bubble);
    });
    if (atBottom) {
      dom.messageList.scrollTop = dom.messageList.scrollHeight;
    } else {
      showNewMsgBadge();
    }
    var total = dom.messageList.querySelectorAll('.message-bubble').length;
    dom.msgCount.textContent = '(' + total + ')';
  }

  function showNewMsgBadge() {
    var existing = document.getElementById('newMsgBadge');
    if (existing) return;
    var badge = el('div', {
      id: 'newMsgBadge',
      className: 'new-msg-badge',
      textContent: '有新消息',
      onclick: function () {
        dom.messageList.scrollTop = dom.messageList.scrollHeight;
        badge.remove();
      }
    });
    dom.messageList.parentElement.style.position = 'relative';
    dom.messageList.parentElement.appendChild(badge);
    setTimeout(function () { if (badge.parentElement) badge.remove(); }, 8000);
  }

  function buildBubble(message, text, selfId) {
    var senderId = message.sender_id || 'system';
    var isSystem = message.msg_type === 'system';
    var isSelf = senderId === selfId;
    var bubble = el('div', { className: 'message-bubble ' + (isSystem ? 'system' : (isSelf ? 'self' : 'other')) });

    // 发送者信息行：名称 + ID + 行为类型
    if (!isSystem) {
      var senderRow = el('div', { className: 'message-sender-row' });
      var senderName = resolveSenderName(senderId);
      senderRow.appendChild(el('span', { className: 'message-sender-name', textContent: senderName }));
      senderRow.appendChild(el('span', { className: 'message-sender-id', textContent: shortId(senderId) }));
      senderRow.appendChild(el('span', {
        className: 'message-type-badge ' + (message.msg_type || 'text'),
        textContent: translateMessageType(message.msg_type || 'text')
      }));
      bubble.appendChild(senderRow);
    }

    // 消息全文（尝试 JSON 美化）
    var contentEl = el('div', { className: 'message-content' });
    try {
      var parsed = JSON.parse(text);
      contentEl.appendChild(el('pre', { className: 'message-content-json', textContent: JSON.stringify(parsed, null, 2) }));
    } catch (_) {
      contentEl.textContent = text;
    }
    bubble.appendChild(contentEl);

    // 元信息行
    var meta = el('div', { className: 'message-meta' });
    meta.appendChild(el('span', { textContent: '第 ' + (message.round || '?') + ' 轮' }));
    meta.appendChild(el('span', { textContent: formatTime(message.timestamp) }));
    bubble.appendChild(meta);
    return bubble;
  }

  function resolveSenderName(senderId) {
    // 从缓存查找
    if (state.agentNames && state.agentNames[senderId]) return state.agentNames[senderId];
    // 从当前用户的 agent 列表查找
    var agent = state.userAgents.find(function (a) { return a.id === senderId; });
    if (agent) return agent.name + (agent.organization ? '（' + agent.organization + '）' : '');
    return shortId(senderId);
  }

  function loadSpaceParticipantNames(space) {
    var ids = (space.agent_ids || []).filter(function (id) {
      return !state.agentNames[id] && !state.userAgents.find(function (a) { return a.id === id; });
    });
    ids.forEach(function (agentId) {
      request('/api/v1/agents/' + agentId, { allowUnauthorized: true }).then(function (data) {
        var name = (data.name || shortId(agentId)) + (data.organization ? '（' + data.organization + '）' : '');
        state.agentNames[agentId] = name;
      }).catch(function () {
        state.agentNames[agentId] = shortId(agentId);
      });
    });
  }

  function loadProposals(spaceId) {
    request('/api/v1/spaces/' + spaceId + '/proposals').then(function (proposals) {
      renderProposals(Array.isArray(proposals) ? proposals : []);
    }).catch(function (err) {
      dom.proposalTimeline.innerHTML = '<p style="padding:16px;color:var(--text-muted)">加载提案失败：' + err.message + '</p>';
    });
  }

  function renderProposals(proposals) {
    dom.proposalCount.textContent = '(' + proposals.length + ')';
    dom.proposalTimeline.textContent = '';
    if (!proposals.length) {
      dom.proposalTimeline.innerHTML = '<div class="empty-state"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M9 5H7a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2"/><rect x="9" y="3" width="6" height="4" rx="1"/><path d="M9 14l2 2 4-4"/></svg><h3>暂无提案</h3><p>该协商空间暂无提案。</p></div>';
      return;
    }
    proposals.forEach(function (proposal) {
      var item = el('div', { className: 'timeline-item' });
      item.appendChild(el('div', { className: 'timeline-dot ' + (proposal.status || 'pending') }));
      item.appendChild(el('div', { className: 'timeline-label', textContent: translateProposalType(proposal.proposal_type) }));
      var content = el('div', { className: 'timeline-content' });
      var detail = el('div', { className: 'timeline-detail' });
      detail.appendChild(el('span', { className: 'timeline-sender', textContent: shortId(proposal.sender_id) }));
      detail.appendChild(el('span', { className: 'timeline-time', textContent: formatTime(proposal.created_at) }));
      content.appendChild(detail);
      var metrics = [];
      if (proposal.dimensions && proposal.dimensions.price != null) metrics.push('价格 ' + proposal.dimensions.price);
      if (proposal.dimensions && proposal.dimensions.timeline_days != null) metrics.push('工期 ' + proposal.dimensions.timeline_days + ' 天');
      if (proposal.dimensions && proposal.dimensions.quality_tier) metrics.push('质量 ' + proposal.dimensions.quality_tier);
      content.appendChild(el('div', {
        textContent: '第 ' + (proposal.round || '?') + ' 轮 · ' + translateStatus(proposal.status || 'pending'),
        style: 'margin-top:4px;font-size:0.78rem;color:var(--text-muted)'
      }));
      if (metrics.length) {
        content.appendChild(el('div', {
          textContent: metrics.join(' · '),
          style: 'margin-top:6px;font-size:0.8rem;color:var(--text-secondary)'
        }));
      }
      item.appendChild(content);
      dom.proposalTimeline.appendChild(item);
    });
  }

  function connectWS() {
    if (state.ws) {
      state.ws.onopen = null;
      state.ws.onmessage = null;
      state.ws.onclose = null;
      state.ws.onerror = null;
      state.ws.close();
      state.ws = null;
    }
    var agent = currentAgent();
    if (!agent) return;
    setHealth('connecting');
    try {
      state.ws = new WebSocket(state.wsBase + '/' + agent.id + '?token=' + agent.api_key);
    } catch (_) {
      setHealth('disconnected');
      return;
    }
    state.ws.onopen = function () {
      state.wsRetries = 0;
      setHealth('connected');
    };
    state.ws.onmessage = function (event) {
      try {
        var data = JSON.parse(event.data);
        notifyNegotiation(data.type, data);
        handleWSMessage(data);
      } catch (_) {}
    };
    state.ws.onclose = function () {
      setHealth('disconnected');
      state.wsRetries += 1;
      var wait = Math.min(2000 * Math.pow(2, state.wsRetries), 30000);
      setTimeout(function () {
        if (state.currentAgentId) connectWS();
      }, wait);
    };
    state.ws.onerror = function () {
      setHealth('disconnected');
    };
  }

  function handleWSMessage(data) {
    if (data.type === 'error') {
      toast('服务端错误：' + ((data.payload && data.payload.message) || '未知错误'), 'error');
      return;
    }
    if (/space_|rfp_created/.test(data.type) || data.type === 'space_left') loadSpaces();
    if (data.type === 'need_matched') {
      toast('需求已匹配 Provider！', 'success');
      loadMyNeeds();
      loadNeeds();
    }
    if (data.type === 'new_message' && data.space_id) {
      var msg = (data.payload && data.payload.message) || data;
      if (data.space_id === state.selectedSpaceId) {
        appendMessage(msg, data.space_id);
      } else {
        if (!state.pendingMessages[data.space_id]) state.pendingMessages[data.space_id] = [];
        state.pendingMessages[data.space_id].push(msg);
      }
    }
    if (data.space_id === state.selectedSpaceId) {
      if (data.type === 'new_proposal' || data.type === 'proposal_update' || data.type === 'best_terms_shared') loadProposals(data.space_id);
    }
  }

  function flushPendingMessages(spaceId) {
    var pending = state.pendingMessages[spaceId];
    if (!pending || !pending.length) return;
    appendNewMessages(pending, spaceId, true);
    delete state.pendingMessages[spaceId];
  }

  function startAutoRefresh() {
    stopAutoRefresh();
    state.refreshTimer = setInterval(function () {
      if (!state.currentAgentId) return;
      request('/api/v1/agents/' + state.currentAgentId + '/spaces').then(function (spaces) {
        state.spaces = Array.isArray(spaces) ? spaces : [];
        renderSpaceList();
        renderOverview();
        populateRateOptions();
        if (state.selectedSpaceId) {
          var exists = state.spaces.some(function (s) { return s.id === state.selectedSpaceId; });
          if (!exists) closeSpaceDetail();
          else {
            loadMessages(state.selectedSpaceId, true);
            loadProposals(state.selectedSpaceId);
            var sp = state.spaces.find(function(s) { return s.id === state.selectedSpaceId; });
            if (sp && sp.space_type === 'rfp') loadRfpVisualization(state.selectedSpaceId, sp);
          }
        }
      }).catch(function () {});
    }, REFRESH_INTERVAL);
  }

  function stopAutoRefresh() {
    if (state.refreshTimer) {
      clearInterval(state.refreshTimer);
      state.refreshTimer = null;
    }
  }

  function appendMessage(message, spaceId) {
    if (!dom.messageList.querySelector('.message-bubble')) {
      dom.messageList.textContent = '';
    }
    var dup = message.id && dom.messageList.querySelector('[data-msg-id="' + message.id + '"]');
    if (dup) return;
    appendNewMessages([message], spaceId, true);
  }

  function setView(view) {
    state.currentView = view;
    Array.prototype.slice.call(document.querySelectorAll('.app-view')).forEach(function (section) {
      section.classList.toggle('active', section.id === view + 'View');
      if (section.id === view + 'View') {
        // Trigger dissolve micro-interaction
        section.classList.remove('panel-dissolve');
        void section.offsetWidth; // Trigger reflow
        section.classList.add('panel-dissolve');
        
        // Trigger glitch effect on headings
        var headings = section.querySelectorAll('h2, h3');
        headings.forEach(function(h) {
          if (!h.hasAttribute('data-text')) {
            h.setAttribute('data-text', h.textContent);
          }
          h.classList.add('glitch-text');
          setTimeout(function() {
            h.classList.remove('glitch-text');
          }, 800); // Glitch duration
        });
      }
    });
    dom.viewButtons.forEach(function (button) {
      button.classList.toggle('active', button.dataset.view === view);
    });
    if (window.innerWidth <= 768) dom.sidebar.classList.remove('open');
  }

  function loadDiscoveryProviders() {
    if (!state.currentAgentId) return;
    var params = [];
    if (dom.providerSkills.value.trim()) params.push('skills=' + encodeURIComponent(dom.providerSkills.value.trim()));
    if (dom.providerCategory.value.trim()) params.push('category=' + encodeURIComponent(dom.providerCategory.value.trim()));
    if (dom.providerAvailability.value) params.push('availability=' + encodeURIComponent(dom.providerAvailability.value));
    if (dom.providerMinPrice.value) params.push('min_price=' + encodeURIComponent(dom.providerMinPrice.value));
    if (dom.providerMaxPrice.value) params.push('max_price=' + encodeURIComponent(dom.providerMaxPrice.value));
    var query = params.length ? ('?' + params.join('&')) : '';
    request('/api/v1/providers/search' + query, { allowUnauthorized: true }).then(function (results) {
      state.discoveryResults = (Array.isArray(results) ? results : []).filter(function (profile) {
        return profile.agent_id !== state.currentAgentId;
      });
      renderDiscoveryResults();
      renderOverview();
      if (state.selectedProvider) {
        var next = state.discoveryResults.find(function (item) { return item.agent_id === state.selectedProvider.agent_id; });
        if (next) selectProvider(next);
      }
    }).catch(function (err) {
      dom.providerResults.innerHTML = '<div class="empty-state"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/></svg><h3>加载失败</h3><p>' + safeText(err.message) + '</p></div>';
    });
  }

  function renderDiscoveryResults() {
    dom.providerResultCount.textContent = '共 ' + state.discoveryResults.length + ' 个 Provider';
    dom.providerResults.textContent = '';
    if (!state.discoveryResults.length) {
      dom.providerResults.innerHTML = '<div class="empty-state"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/></svg><h3>暂无 Provider</h3><p>未发现匹配的 Provider，可尝试放宽筛选条件。</p></div>';
      return;
    }
    state.discoveryResults.forEach(function (profile) {
      var selected = state.selectedProvider && state.selectedProvider.agent_id === profile.agent_id;
      var candidate = state.rfpCandidates.indexOf(profile.agent_id) >= 0;
      var card = el('article', { className: 'provider-card' + (selected ? ' selected' : '') });
      card.appendChild(el('div', {
        className: 'provider-card-head',
        innerHTML: '<div><h3>' + safeText(profile.display_name) + '</h3><p>' + shortId(profile.agent_id) + '</p></div><span class="space-badge ' + (profile.availability_status || 'created') + '">' + translateAvailability(profile.availability_status) + '</span>'
      }));
      card.appendChild(el('p', { className: 'provider-desc', textContent: profile.description || '暂无简介' }));
      card.appendChild(el('div', {
        className: 'provider-meta-line',
        textContent: '分类：' + safeText(profile.capabilities && profile.capabilities.category) + ' · 价格：' + formatPrice(profile.min_price, profile.max_price)
      }));
      var tags = el('div', { className: 'chip-list compact' });
      (profile.skills || []).slice(0, 5).forEach(function (skill) {
        tags.appendChild(el('span', { className: 'chip', textContent: skill }));
      });
      card.appendChild(tags);
      var actions = el('div', { className: 'inline-actions' });
      actions.appendChild(el('button', {
        className: 'btn-sm',
        type: 'button',
        textContent: '查看详情',
        onclick: function () { selectProvider(profile); }
      }));
      actions.appendChild(el('button', {
        className: 'btn-sm',
        type: 'button',
        textContent: '创建谈判',
        onclick: function () {
          setView('arena');
          dom.bilateralInviteeId.value = profile.agent_id;
          dom.createSpaceName.value = profile.display_name + ' 协商';
        }
      }));
      actions.appendChild(el('button', {
        className: 'btn-sm',
        type: 'button',
        textContent: candidate ? '移出 RFP' : '加入 RFP',
        onclick: function () { toggleRfpCandidate(profile.agent_id); }
      }));
      card.appendChild(actions);
      dom.providerResults.appendChild(card);
    });
    renderRfpCandidates();
  }

  function selectProvider(profile) {
    state.selectedProvider = profile;
    renderDiscoveryResults();
    dom.providerDetail.innerHTML = '<div class="spinner"></div>';
    request('/api/v1/agents/' + profile.agent_id + '/reputation', { allowUnauthorized: true }).then(function (reputation) {
      state.providerReputation = reputation;
      renderProviderDetail();
    }).catch(function () {
      state.providerReputation = null;
      renderProviderDetail();
    });
  }

  function renderProviderDetail() {
    var profile = state.selectedProvider;
    if (!profile) {
      dom.providerDetail.innerHTML = '<p>选择一个 Provider，查看 Profile、技能、价格区间与信誉详情。</p>';
      return;
    }
    var reputation = state.providerReputation && state.providerReputation.summary;
    dom.providerDetail.innerHTML =
      '<div class="provider-profile-panel">' +
      '<div class="provider-profile-top">' +
      '<div><h3>' + safeText(profile.display_name) + '</h3><p>' + shortId(profile.agent_id) + ' · ' + safeText(profile.capabilities && profile.capabilities.category) + '</p></div>' +
      '<span class="space-badge ' + (profile.availability_status || 'created') + '">' + translateAvailability(profile.availability_status) + '</span>' +
      '</div>' +
      '<p class="provider-desc">' + safeText(profile.description || '暂无简介') + '</p>' +
      '<div class="summary-grid">' +
      '<div><span>价格区间</span><strong>' + formatPrice(profile.min_price, profile.max_price) + '</strong></div>' +
      '<div><span>定价模式</span><strong>' + safeText(profile.pricing_model) + '</strong></div>' +
      '<div><span>信誉分</span><strong>' + (reputation ? formatScore(reputation.reputation_score) : '0.0') + '</strong></div>' +
      '<div><span>平均评分</span><strong>' + (reputation && reputation.avg_rating != null ? reputation.avg_rating.toFixed(1) : '—') + '</strong></div>' +
      '</div>' +
      '<div class="chip-list">' + (profile.skills || []).map(function (skill) { return '<span class="chip">' + skill + '</span>'; }).join('') + '</div>' +
      '<div class="inline-actions">' +
      '<button class="btn-accent" type="button" id="detailCreateSpaceBtn">用此 Provider 创建谈判</button>' +
      '<button class="btn-ghost" type="button" id="detailAddRfpBtn">' + (state.rfpCandidates.indexOf(profile.agent_id) >= 0 ? '移出 RFP' : '加入 RFP') + '</button>' +
      '</div>' +
      '</div>';
    document.getElementById('detailCreateSpaceBtn').addEventListener('click', function () {
      setView('arena');
      dom.bilateralInviteeId.value = profile.agent_id;
      dom.createSpaceName.value = profile.display_name + ' 协商';
    });
    document.getElementById('detailAddRfpBtn').addEventListener('click', function () {
      toggleRfpCandidate(profile.agent_id);
      renderProviderDetail();
    });
  }

  function toggleRfpCandidate(agentId) {
    var index = state.rfpCandidates.indexOf(agentId);
    if (index >= 0) state.rfpCandidates.splice(index, 1);
    else state.rfpCandidates.push(agentId);
    syncRfpCandidateInput();
    renderRfpCandidates();
    renderDiscoveryResults();
  }

  function syncRfpCandidateInput() {
    var manual = parseCommaList(dom.rfpProviderIds.value);
    var merged = state.rfpCandidates.slice();
    manual.forEach(function (id) {
      if (merged.indexOf(id) < 0) merged.push(id);
    });
    dom.rfpProviderIds.value = merged.join(', ');
  }

  function renderRfpCandidates() {
    dom.rfpCandidateList.textContent = '';
    if (!state.rfpCandidates.length) {
      dom.rfpCandidateList.classList.add('empty');
      dom.rfpCandidateList.innerHTML = '<p>在发现页点击“加入 RFP”，候选会同步到这里。</p>';
      return;
    }
    dom.rfpCandidateList.classList.remove('empty');
    state.rfpCandidates.forEach(function (id) {
      var chip = el('button', {
        className: 'chip removable',
        type: 'button',
        textContent: shortId(id) + ' ×',
        onclick: function () { toggleRfpCandidate(id); }
      });
      dom.rfpCandidateList.appendChild(chip);
    });
  }

  function parseCommaList(value) {
    return String(value || '')
      .split(',')
      .map(function (item) { return item.trim(); })
      .filter(Boolean);
  }

  function sendWsMessage(payload) {
    if (!state.ws || state.ws.readyState !== 1) throw new Error('当前 Agent WebSocket 尚未连接');
    state.ws.send(JSON.stringify(payload));
  }

  function parseOptionalJson(text) {
    if (!text || !text.trim()) return null;
    return JSON.parse(text);
  }

  function handleCreateSpace(event) {
    event.preventDefault();
    try {
      var invitee = dom.bilateralInviteeId.value.trim();
      if (!invitee) throw new Error('请输入对手方 Agent ID');
      var context = {
        description: dom.createSpaceDescription.value.trim() || null,
        budget: dom.createSpaceBudget.value ? Number(dom.createSpaceBudget.value) : null,
        terms: parseOptionalJson(dom.createSpaceTerms.value)
      };
      sendWsMessage({
        type: 'create_space',
        payload: {
          name: dom.createSpaceName.value.trim() || '未命名协商',
          invitee_ids: [invitee],
          my_role: dom.myRole.value || 'buyer',
          context: context
        }
      });
      toast('已发送创建谈判请求', 'success');
      dom.createSpaceForm.reset();
      if (window.closeBottomSheet) window.closeBottomSheet('createSpaceSheet');
      dom.myRole.value = 'buyer';
      setView('arena');
    } catch (err) {
      toast('创建谈判失败：' + err.message, 'error');
    }
  }

  function handleCreateRfp(event) {
    event.preventDefault();
    try {
      syncRfpCandidateInput();
      var providerIds = parseCommaList(dom.rfpProviderIds.value);
      if (!providerIds.length) throw new Error('请至少提供一个 Provider ID');
      sendWsMessage({
        type: 'create_rfp',
        payload: {
          name: dom.createRfpName.value.trim() || '未命名 RFP',
          provider_ids: providerIds,
          allowed_rounds: dom.rfpRounds.value ? Number(dom.rfpRounds.value) : null,
          evaluation_criteria: parseCommaList(dom.rfpCriteria.value),
          deadline: dom.rfpDeadline.value ? new Date(dom.rfpDeadline.value).getTime() : null,
          share_best_terms: !!dom.rfpShareBestTerms.checked,
          context: {
            requirements: dom.rfpRequirements.value.trim() || null,
            budget: dom.rfpBudget.value ? Number(dom.rfpBudget.value) : null
          }
        }
      });
      toast('已发送创建 RFP 请求', 'success');
      dom.createRfpForm.reset();
      if (window.closeBottomSheet) window.closeBottomSheet('createRfpSheet');
      state.rfpCandidates = [];
      renderRfpCandidates();
      setView('arena');
    } catch (err) {
      toast('创建 RFP 失败：' + err.message, 'error');
    }
  }

  function loadCurrentReputation() {
    if (!state.currentAgentId) return;
    request('/api/v1/agents/' + state.currentAgentId + '/reputation', { allowUnauthorized: true }).then(function (detail) {
      state.currentReputation = detail;
      renderReputation();
      renderOverview();
    }).catch(function () {
      state.currentReputation = {
        summary: {
          reputation_score: 0,
          avg_rating: null,
          fulfillment_rate: 0,
          total_negotiations: 0
        },
        recent_events: []
      };
      renderReputation();
      renderOverview();
    });
  }

  function renderReputation() {
    var summary = (state.currentReputation && state.currentReputation.summary) || {};
    dom.reputationScoreValue.textContent = formatScore(summary.reputation_score);
    dom.reputationRatingValue.textContent = summary.avg_rating != null ? summary.avg_rating.toFixed(1) : '—';
    dom.reputationFulfillmentValue.textContent = formatPercent(summary.fulfillment_rate);
    dom.reputationTotalValue.textContent = String(summary.total_negotiations || 0);
    dom.reputationEvents.textContent = '';
    var events = (state.currentReputation && state.currentReputation.recent_events) || [];
    if (!events.length) {
      dom.reputationEvents.innerHTML = '<p>暂无信誉事件，完成协商并评分后会显示在这里。</p>';
      return;
    }
    events.forEach(function (item) {
      var row = el('article', { className: 'event-row' });
      row.appendChild(el('div', {
        className: 'event-row-head',
        innerHTML: '<strong>' + translateEventType(item.event_type) + '</strong><span>' + formatTime(item.created_at * 1000 || item.created_at) + '</span>'
      }));
      row.appendChild(el('p', {
        textContent: '结果：' + translateOutcome(item.outcome) + ' · 评分：' + (item.rating != null ? item.rating + '/5' : '未打分') + ' · 对手方：' + shortId(item.counterparty_id)
      }));
      dom.reputationEvents.appendChild(row);
    });
  }

  function populateRateOptions() {
    var rateable = state.spaces.filter(function (space) {
      return space.status === 'concluded' || space.status === 'cancelled';
    });
    dom.rateSpaceSelect.textContent = '';
    if (!rateable.length) {
      dom.rateSpaceSelect.appendChild(el('option', { value: '', textContent: '暂无可评分空间' }));
      dom.rateTargetSelect.innerHTML = '<option value="">暂无评分对象</option>';
      return;
    }
    rateable.forEach(function (space) {
      dom.rateSpaceSelect.appendChild(el('option', {
        value: space.id,
        textContent: (space.name || '未命名空间') + ' · ' + translateStatus(space.status)
      }));
    });
    updateRateTargets();
  }

  function updateRateTargets() {
    var space = state.spaces.find(function (item) { return item.id === dom.rateSpaceSelect.value; });
    dom.rateTargetSelect.textContent = '';
    if (!space) {
      dom.rateTargetSelect.appendChild(el('option', { value: '', textContent: '暂无评分对象' }));
      return;
    }
    var targets = (space.agent_ids || []).filter(function (id) { return id !== state.currentAgentId; });
    if (!targets.length) dom.rateTargetSelect.appendChild(el('option', { value: '', textContent: '未找到对手方' }));
    targets.forEach(function (id) {
      var agent = state.userAgents.find(function (item) { return item.id === id; });
      dom.rateTargetSelect.appendChild(el('option', { value: id, textContent: (agent ? agent.name : shortId(id)) + ' · ' + shortId(id) }));
    });
    dom.rateEventType.value = space.status === 'cancelled' ? 'cancelled' : 'concluded';
    dom.rateHint.textContent = '当前空间：' + (space.name || '未命名空间') + '，将以当前 Agent 身份为对手方提交评分。';
  }

  function handleRateSubmit(event) {
    event.preventDefault();
    var spaceId = dom.rateSpaceSelect.value;
    var targetId = dom.rateTargetSelect.value;
    if (!spaceId || !targetId) return toast('请选择空间与评分对象', 'error');
    request('/api/v1/spaces/' + spaceId + '/rate', {
      method: 'POST',
      body: JSON.stringify({
        space_id: spaceId,
        agent_id: targetId,
        event_type: dom.rateEventType.value,
        outcome: dom.rateOutcome.value,
        rating: Number(dom.rateValue.value || 0),
        counterparty_id: state.currentAgentId
      })
    }).then(function () {
      toast('评分已提交', 'success');
      loadCurrentReputation();
      if (state.selectedProvider && state.selectedProvider.agent_id === targetId) selectProvider(state.selectedProvider);
    }).catch(function (err) {
      toast('提交评分失败：' + err.message, 'error');
    });
  }

  function renderProfileCards() {
    var agent = currentAgent();
    dom.profileUserCard.innerHTML = state.currentUser
      ? '<div class="profile-card"><span>用户</span><strong>' + safeText(state.currentUser.display_name) + '</strong><p>' + safeText(state.currentUser.email) + '</p></div>'
      : '';
    dom.profileAgentCard.innerHTML = agent
      ? '<div class="profile-card"><span>当前 Agent</span><strong>' + safeText(agent.name) + '</strong><p>' + translateAgentType(agent.agent_type) + ' · ' + shortId(agent.id) + '<br>接口：' + state.apiBase + '</p></div>'
      : '<div class="profile-card"><span>当前 Agent</span><strong>未选择</strong><p>请先创建或选择一个 Agent。</p></div>';
  }

  function loadProviderProfile() {
    var agent = currentAgent();
    renderProfileCards();
    if (!agent) return;
    if (agent.agent_type !== 'provider') {
      state.currentProfile = null;
      dom.providerProfileNotice.textContent = '当前 Agent 为买方，无需维护公开 Provider Profile。';
      dom.providerProfileForm.classList.add('is-disabled');
      resetProviderForm(agent);
      return;
    }
    dom.providerProfileNotice.textContent = '当前 Agent 为卖方，可编辑公开发现资料并立即用于发现页。';
    dom.providerProfileForm.classList.remove('is-disabled');
    request('/api/v1/providers/' + agent.id + '/profile', { allowUnauthorized: true }).then(function (profile) {
      state.currentProfile = profile;
      fillProviderForm(profile, agent);
    }).catch(function () {
      state.currentProfile = null;
      resetProviderForm(agent);
    });
  }

  function fillProviderForm(profile, agent) {
    dom.profileDisplayName.value = profile.display_name || agent.name || '';
    dom.profileCategory.value = profile.capabilities && profile.capabilities.category || '';
    dom.profileDescription.value = profile.description || '';
    dom.profileSkills.value = (profile.skills || []).join(', ');
    dom.profileTags.value = profile.capabilities && profile.capabilities.tags ? profile.capabilities.tags.join(', ') : '';
    dom.profilePricing.value = typeof profile.pricing_model === 'string' ? profile.pricing_model : 'unknown';
    dom.profileAvailability.value = profile.availability_status || 'available';
    dom.profileMinPrice.value = profile.min_price != null ? profile.min_price : '';
    dom.profileMaxPrice.value = profile.max_price != null ? profile.max_price : '';
  }

  function resetProviderForm(agent) {
    var description = agent && agent.metadata && agent.metadata.description ? agent.metadata.description : '';
    dom.profileDisplayName.value = agent ? agent.name : '';
    dom.profileCategory.value = '';
    dom.profileDescription.value = description || '';
    dom.profileSkills.value = '';
    dom.profileTags.value = '';
    dom.profilePricing.value = 'negotiated';
    dom.profileAvailability.value = 'available';
    dom.profileMinPrice.value = '';
    dom.profileMaxPrice.value = '';
  }

  function handleProviderProfileSubmit(event) {
    event.preventDefault();
    var agent = currentAgent();
    if (!agent) return toast('请先选择 Agent', 'error');
    if (agent.agent_type !== 'provider') return toast('仅 Provider Agent 可维护公开 Profile', 'error');
    request('/api/v1/providers/me/profile', {
      method: 'PUT',
      token: agent.api_key,
      body: JSON.stringify({
        display_name: dom.profileDisplayName.value.trim() || agent.name,
        description: dom.profileDescription.value.trim() || null,
        skills: parseCommaList(dom.profileSkills.value),
        capabilities: {
          category: dom.profileCategory.value.trim() || 'unknown',
          tags: parseCommaList(dom.profileTags.value)
        },
        pricing_model: dom.profilePricing.value,
        availability_status: dom.profileAvailability.value,
        min_price: dom.profileMinPrice.value ? Number(dom.profileMinPrice.value) : null,
        max_price: dom.profileMaxPrice.value ? Number(dom.profileMaxPrice.value) : null
      })
    }).then(function (profile) {
      state.currentProfile = profile;
      toast('Provider Profile 已保存', 'success');
      loadDiscoveryProviders();
    }).catch(function (err) {
      toast('保存 Profile 失败：' + err.message, 'error');
    });
  }


  // ── Phase 3: RFP Negotiation Visualization ───────────

  function loadRfpVisualization(spaceId, space) {
    if (!space || space.space_type !== 'rfp') {
      if (dom.rfpVizSection) dom.rfpVizSection.style.display = 'none';
      return;
    }
    if (dom.rfpVizSection) dom.rfpVizSection.style.display = '';
    loadRoundInfo(spaceId, space);
    loadRfpProposals(spaceId);
  }

  function loadRoundInfo(spaceId, space) {
    request('/api/v1/spaces/' + spaceId + '/rounds').then(function (data) {
      state.currentRounds = data;
      renderRoundProgress(data, space);
    }).catch(function () {
      state.currentRounds = null;
      renderRoundProgress(null, space);
    });
  }

  function renderRoundProgress(roundsData, space) {
    if (!roundsData) {
      dom.roundProgressFill.style.width = '0%';
      dom.roundProgressLabel.textContent = 'Round ? / ?';
      dom.roundStatusLabel.textContent = '--';
      dom.roundStatusLabel.className = 'space-badge';
      dom.roundDeadlineInfo.style.display = 'none';
      dom.advanceRoundBtn.style.display = 'none';
      return;
    }
    var current = roundsData.current_round || 0;
    var allowed = roundsData.allowed_rounds || 0;
    var status = roundsData.round_status || 'unknown';
    var deadline = roundsData.round_deadline;

    var pct = allowed > 0 ? Math.min(100, Math.round((current / allowed) * 100)) : 0;
    dom.roundProgressFill.style.width = pct + '%';
    dom.roundProgressLabel.textContent = 'Round ' + current + ' / ' + allowed;

    var statusMap = { open: 'active', closed: 'concluded', expired: 'expired' };
    dom.roundStatusLabel.textContent = translateRoundStatus(status);
    dom.roundStatusLabel.className = 'space-badge ' + (statusMap[status] || 'created');

    if (deadline) {
      dom.roundDeadlineInfo.style.display = '';
      dom.roundDeadlineInfo.innerHTML = '截止：<code>' + formatTime(deadline) + '</code>';
    } else {
      dom.roundDeadlineInfo.style.display = 'none';
    }

    var isCreator = space && space.creator_id === state.currentAgentId;
    if (isCreator && status === 'open' && current < allowed) {
      dom.advanceRoundBtn.style.display = '';
    } else {
      dom.advanceRoundBtn.style.display = 'none';
    }
  }

  function translateRoundStatus(status) {
    var map = { open: '开放', closed: '已关闭', expired: '已过期', unknown: '未知' };
    return map[status] || safeText(status);
  }

  function handleAdvanceRound() {
    if (!state.selectedSpaceId) return;
    dom.advanceRoundBtn.disabled = true;
    dom.advanceRoundBtn.textContent = '推进中...';
    request('/api/v1/spaces/' + state.selectedSpaceId + '/rounds/advance', {
      method: 'POST'
    }).then(function (data) {
      toast('轮次已推进', 'success');
      state.currentRounds = data;
      var space = state.spaces.find(function (s) { return s.id === state.selectedSpaceId; });
      renderRoundProgress(data, space);
      loadRfpProposals(state.selectedSpaceId);
    }).catch(function (err) {
      toast('推进轮次失败：' + err.message, 'error');
    }).finally(function () {
      dom.advanceRoundBtn.disabled = false;
      dom.advanceRoundBtn.textContent = '推进轮次';
    });
  }

  function loadRfpProposals(spaceId) {
    request('/api/v1/spaces/' + spaceId + '/proposals').then(function (proposals) {
      state.rfpProposals = Array.isArray(proposals) ? proposals : [];
      renderRfpProposalTable(state.rfpProposals);
    }).catch(function () {
      state.rfpProposals = [];
      renderRfpProposalTable([]);
    });
  }

  function renderRfpProposalTable(proposals) {
    dom.rfpProposalTableBody.textContent = '';
    var pending = proposals.filter(function (p) { return p.status === 'pending'; });
    if (!pending.length) {
      var row = el('tr');
      row.appendChild(el('td', { colSpan: '6', style: 'text-align:center;color:var(--text-muted);padding:16px;', textContent: '暂无待处理提案' }));
      dom.rfpProposalTableBody.appendChild(row);
      return;
    }
    pending.forEach(function (proposal) {
      var row = el('tr');
      var dims = proposal.dimensions || {};
      row.appendChild(el('td', { className: 'provider-cell', textContent: resolveSenderName(proposal.sender_id) }));
      row.appendChild(el('td', { className: 'price-cell', textContent: dims.price != null ? String(dims.price) : '--' }));
      row.appendChild(el('td', { textContent: dims.timeline_days != null ? String(dims.timeline_days) : '--' }));
      row.appendChild(el('td', { textContent: dims.quality_tier || '--' }));
      row.appendChild(el('td', { innerHTML: '<span class="space-badge pending">' + translateStatus('pending') + '</span>' }));

      var actionsCell = el('td', { className: 'action-cell' });
      actionsCell.appendChild(el('button', {
        className: 'btn-accept',
        type: 'button',
        textContent: '接受',
        onclick: function () { handleProposalAction(proposal.id, 'accept'); }
      }));
      actionsCell.appendChild(el('button', {
        className: 'btn-reject',
        type: 'button',
        textContent: '拒绝',
        onclick: function () { handleProposalAction(proposal.id, 'reject'); }
      }));
      row.appendChild(actionsCell);
      dom.rfpProposalTableBody.appendChild(row);
    });
  }

  function handleProposalAction(proposalId, action) {
    if (!state.selectedSpaceId) return;
    request('/api/v1/spaces/' + state.selectedSpaceId + '/proposals/' + proposalId + '/respond', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ action: action })
    }).then(function () {
      toast('提案已' + (action === 'accept' ? '接受' : '拒绝'), 'success');
      loadRfpProposals(state.selectedSpaceId);
      loadProposals(state.selectedSpaceId);
    }).catch(function (err) {
      toast('操作失败：' + err.message, 'error');
    });
  }

  function setupEvaluateSliders() {
    function updateSliderValue(slider, display) {
      display.textContent = slider.value + '%';
    }
    [dom.weightPrice, dom.weightTimeline, dom.weightQuality].forEach(function (slider, idx) {
      var displays = [dom.weightPriceValue, dom.weightTimelineValue, dom.weightQualityValue];
      if (!slider) return;
      slider.addEventListener('input', function () {
        updateSliderValue(slider, displays[idx]);
      });
    });
  }

  function handleEvaluate() {
    if (!state.selectedSpaceId) return;
    var wp = Number(dom.weightPrice.value) / 100;
    var wt = Number(dom.weightTimeline.value) / 100;
    var wq = Number(dom.weightQuality.value) / 100;
    dom.evaluateBtn.disabled = true;
    dom.evaluateBtn.textContent = '评估中...';
    request('/api/v1/spaces/' + state.selectedSpaceId + '/proposals/evaluate', {
      method: 'POST',
      body: JSON.stringify({ weights: { price: wp, timeline: wt, quality: wq } })
    }).then(function (data) {
      var results = Array.isArray(data) ? data : (data && data.results ? data.results : []);
      renderEvaluateResults(results);
      toast('评估完成', 'success');
    }).catch(function (err) {
      toast('评估失败：' + err.message, 'error');
      dom.evaluateResultTableWrap.style.display = 'none';
    }).finally(function () {
      dom.evaluateBtn.disabled = false;
      dom.evaluateBtn.textContent = '评估';
    });
  }

  function renderEvaluateResults(results) {
    dom.evaluateResultTableBody.textContent = '';
    if (!results.length) {
      dom.evaluateResultTableWrap.style.display = 'none';
      return;
    }
    dom.evaluateResultTableWrap.style.display = '';
    results.forEach(function (item, idx) {
      var row = el('tr', { className: idx === 0 ? 'evaluate-rank-1 evaluate-highlight' : (idx < 3 ? 'evaluate-highlight' : '') });
      row.appendChild(el('td', { textContent: '#' + (idx + 1), style: 'font-weight:700;font-family:var(--font-mono);' }));
      row.appendChild(el('td', { className: 'provider-cell', textContent: item.provider_name || item.agent_name || shortId(item.provider_id || item.agent_id) }));
      row.appendChild(el('td', { textContent: formatScore(item.composite_score || item.total_score || 0), style: 'font-weight:700;color:var(--accent-light);' }));
      row.appendChild(el('td', { textContent: formatScore(item.price_score || 0) }));
      row.appendChild(el('td', { textContent: formatScore(item.timeline_score || 0) }));
      row.appendChild(el('td', { textContent: formatScore(item.quality_score || 0) }));
      dom.evaluateResultTableBody.appendChild(row);
    });
  }

  function openCreateRfpFromNeedModal(needId) {
    dom.rfpFromNeedId.value = needId;
    dom.rfpFromNeedRounds.value = '3';
    dom.rfpFromNeedBudget.value = '';
    dom.rfpFromNeedProviders.textContent = '';

    if (state.discoveryResults.length) {
      state.discoveryResults.forEach(function (profile) {
        var item = el('div', { className: 'rfp-provider-check-item' });
        var cb = el('input', { type: 'checkbox', value: profile.agent_id, id: 'rfpP_' + profile.agent_id });
        var label = el('label', { 'for': 'rfpP_' + profile.agent_id });
        label.appendChild(el('span', { className: 'provider-check-name', textContent: profile.display_name || shortId(profile.agent_id) }));
        label.appendChild(el('span', { className: 'provider-check-id', textContent: ' ' + shortId(profile.agent_id) }));
        item.appendChild(cb);
        item.appendChild(label);
        dom.rfpFromNeedProviders.appendChild(item);
      });
    } else {
      dom.rfpFromNeedProviders.appendChild(el('p', {
        className: 'rfp-provider-empty',
        textContent: '暂无可选 Provider，请先在发现页添加。你也可以手动输入 Provider ID。'
      }));
      var inputRow = el('div', { className: 'login-input-group', style: 'margin-top:8px;' });
      inputRow.appendChild(el('label', { textContent: 'Provider IDs（逗号分隔）' }));
      inputRow.appendChild(el('input', {
        type: 'text',
        className: 'login-input',
        id: 'rfpManualProviderIds',
        placeholder: 'agent-a, agent-b'
      }));
      dom.rfpFromNeedProviders.appendChild(inputRow);
    }

    dom.createRfpFromNeedModal.style.display = 'flex';
  }

  function closeCreateRfpFromNeedModal() {
    dom.createRfpFromNeedModal.style.display = 'none';
    dom.createRfpFromNeedForm.reset();
  }

  function handleCreateRfpFromNeed(event) {
    event.preventDefault();
    var needId = dom.rfpFromNeedId.value;
    if (!needId) return toast('需求 ID 缺失', 'error');

    var providerIds = [];
    var checkboxes = dom.rfpFromNeedProviders.querySelectorAll('input[type="checkbox"]:checked');
    checkboxes.forEach(function (cb) { providerIds.push(cb.value); });

    var manualInput = document.getElementById('rfpManualProviderIds');
    if (manualInput && manualInput.value.trim()) {
      parseCommaList(manualInput.value).forEach(function (id) {
        if (providerIds.indexOf(id) < 0) providerIds.push(id);
      });
    }

    if (!providerIds.length) return toast('请至少选择一个 Provider', 'error');

    var agent = currentAgent();
    if (!agent) return toast('请先选择 Agent', 'error');

    var body = {
      provider_ids: providerIds,
      allowed_rounds: dom.rfpFromNeedRounds.value ? Number(dom.rfpFromNeedRounds.value) : 3
    };
    if (dom.rfpFromNeedBudget.value) body.budget = Number(dom.rfpFromNeedBudget.value);

    request('/api/v1/needs/' + needId + '/create-rfp', {
      method: 'POST',
      token: agent.api_key,
      body: JSON.stringify(body)
    }).then(function () {
      toast('RFP 已发起', 'success');
      closeCreateRfpFromNeedModal();
      loadSpaces();
      loadMyNeeds();
    }).catch(function (err) {
      toast('发起 RFP 失败：' + err.message, 'error');
    });
  }

  // ── Phase 4: 合同管理 ───────────────────────────────────

  // 加载当前 Agent 的所有合同
  function loadAgentContracts() {
    if (!state.currentAgentId) return;
    request('/api/v1/agents/' + state.currentAgentId + '/contracts').then(function (contracts) {
      state.contracts = Array.isArray(contracts) ? contracts : [];
      renderContractsList();
      dom.contractsModal.style.display = 'flex';
    }).catch(function (err) {
      toast('加载合同失败：' + err.message, 'error');
    });
  }

  // 渲染合同列表
  function renderContractsList() {
    dom.contractsList.textContent = '';
    if (!state.contracts.length) {
      dom.contractsList.innerHTML = '<p style="padding:16px;color:var(--text-muted)">暂无合同。</p>';
      return;
    }
    state.contracts.forEach(function (contract) {
      var card = el('div', { className: 'contract-card' });
      var isSeller = contract.seller_id === state.currentAgentId;
      var roleLabel = isSeller ? 'Seller' : 'Buyer';
      var otherParty = isSeller ? contract.buyer_id : contract.seller_id;

      // 计算里程碑进度
      var totalMilestones = (contract.milestones || []).length;
      var completedMilestones = (contract.milestones || []).filter(function (m) {
        return m.status === 'accepted';
      }).length;
      var progressPercent = totalMilestones > 0 ? Math.round((completedMilestones / totalMilestones) * 100) : 0;

      // 计算总额
      var totalAmount = (contract.milestones || []).reduce(function (sum, m) {
        return sum + (m.amount || 0);
      }, 0);

      card.innerHTML =
        '<div class="contract-card-header">' +
        '<div><h3>合同 ' + shortId(contract.id) + '</h3>' +
        '<p>Space: ' + shortId(contract.space_id) + ' · ' + roleLabel + '</p></div>' +
        '<span class="contract-status-badge status-' + (contract.status || 'active') + '">' +
        translateContractStatus(contract.status) + '</span>' +
        '</div>' +
        '<div class="contract-meta">' +
        '<span>对手方：' + shortId(otherParty) + '</span>' +
        '<span>总额：' + totalAmount + '</span>' +
        '</div>' +
        '<div class="contract-progress">' +
        '<div class="progress-bar"><div class="progress-fill" style="width:' + progressPercent + '%"></div></div>' +
        '<span class="progress-text">' + completedMilestones + '/' + totalMilestones + ' 里程碑已完成</span>' +
        '</div>';

      var actions = el('div', { className: 'inline-actions' });
      actions.appendChild(el('button', {
        className: 'btn-accent',
        type: 'button',
        textContent: '查看详情',
        onclick: function () { showContractDetail(contract.id); }
      }));
      if (contract.status === 'active' && isSeller) {
        actions.appendChild(el('button', {
          className: 'btn-sm',
          type: 'button',
          textContent: '发起争议',
          onclick: function () { openDisputeModal(contract.id); }
        }));
      }
      card.appendChild(actions);
      dom.contractsList.appendChild(card);
    });
  }

  // 合同状态翻译
  function translateContractStatus(status) {
    var map = {
      pending: '待确认',
      active: '执行中',
      completed: '已完成',
      disputed: '争议中',
      cancelled: '已取消'
    };
    return map[status] || safeText(status);
  }

  // 显示合同详情
  function showContractDetail(contractId) {
    request('/api/v1/contracts/' + contractId).then(function (contract) {
      state.currentContract = contract;
      renderContractDetail(contract);
      dom.contractDetailPanel.style.display = 'block';
    }).catch(function (err) {
      toast('加载合同详情失败：' + err.message, 'error');
    });
  }

  // 渲染合同详情
  function renderContractDetail(contract) {
    var isSeller = contract.seller_id === state.currentAgentId;

    // 计算总额
    var totalAmount = (contract.milestones || []).reduce(function (sum, m) {
      return sum + (m.amount || 0);
    }, 0);

    dom.contractDetailPanel.innerHTML =
      '<div class="contract-detail-header">' +
      '<div><h2>合同 ' + shortId(contract.id) + '</h2>' +
      '<p>Space: ' + shortId(contract.space_id) + '</p></div>' +
      '<button class="detail-close" id="contractDetailClose">&times;</button>' +
      '</div>' +
      '<div class="contract-info-grid">' +
      '<div><span>状态</span><strong>' + translateContractStatus(contract.status) + '</strong></div>' +
      '<div><span>Seller</span><strong>' + shortId(contract.seller_id) + '</strong></div>' +
      '<div><span>Buyer</span><strong>' + shortId(contract.buyer_id) + '</strong></div>' +
      '<div><span>总金额</span><strong>' + totalAmount + '</strong></div>' +
      '<div><span>创建时间</span><strong>' + formatTime(contract.created_at) + '</strong></div>' +
      '</div>' +
      '<h3 style="margin:24px 0 16px;">里程碑时间线</h3>' +
      '<div class="milestone-timeline" id="milestoneTimeline"></div>';

    document.getElementById('contractDetailClose').addEventListener('click', function () {
      dom.contractDetailPanel.style.display = 'none';
      state.currentContract = null;
    });

    renderMilestoneTimeline(contract.milestones || [], contract, isSeller);
  }

  // 渲染里程碑时间线
  function renderMilestoneTimeline(milestones, contract, isSeller) {
    var timeline = document.getElementById('milestoneTimeline');
    if (!timeline) return;

    timeline.textContent = '';
    if (!milestones.length) {
      timeline.innerHTML = '<p style="padding:16px;color:var(--text-muted)">暂无里程碑。</p>';
      return;
    }

    milestones.forEach(function (milestone, index) {
      var item = el('div', { className: 'milestone-item' });

      var statusClass = 'status-' + (milestone.status || 'pending');
      var statusLabel = translateMilestoneStatus(milestone.status);
      var isPastDue = milestone.due_date && new Date(milestone.due_date) < new Date() && milestone.status === 'pending';

      item.appendChild(el('div', { className: 'milestone-dot ' + statusClass }));

      var content = el('div', { className: 'milestone-content' });
      content.appendChild(el('div', {
        className: 'milestone-header',
        innerHTML: '<strong>' + safeText(milestone.title) + '</strong>' +
        '<span class="milestone-status ' + statusClass + '">' + statusLabel + '</span>'
      }));

      if (milestone.description) {
        content.appendChild(el('p', {
          className: 'milestone-description',
          textContent: milestone.description
        }));
      }

      var meta = el('div', { className: 'milestone-meta' });
      if (milestone.amount != null) {
        meta.appendChild(el('span', { textContent: '金额：' + milestone.amount }));
      }
      if (milestone.due_date) {
        var dueDate = new Date(milestone.due_date);
        var dateText = dueDate.toLocaleString('zh-CN');
        if (isPastDue) {
          meta.appendChild(el('span', {
            className: 'milestone-overdue',
            textContent: '截止：' + dateText + ' (已逾期)'
          }));
        } else {
          meta.appendChild(el('span', { textContent: '截止：' + dateText }));
        }
      }
      content.appendChild(meta);

      // 根据状态显示操作按钮
      if (milestone.status === 'pending') {
        var actions = el('div', { className: 'milestone-actions' });

        if (isProvider) {
          actions.appendChild(el('button', {
            className: 'btn-sm btn-submit-milestone',
            type: 'button',
            textContent: '提交交付物',
            'data-contract-id': contract.id,
            'data-milestone-id': milestone.id,
            'data-milestone-title': milestone.title
          }));
        } else {
          actions.appendChild(el('button', {
            className: 'btn-sm btn-accept-milestone',
            type: 'button',
            textContent: '验收通过',
            'data-contract-id': contract.id,
            'data-milestone-id': milestone.id
          }));
          actions.appendChild(el('button', {
            className: 'btn-sm btn-sm btn-reject-milestone',
            type: 'button',
            textContent: '拒绝',
            'data-contract-id': contract.id,
            'data-milestone-id': milestone.id
          }));
        }
        content.appendChild(actions);
      }

      if (milestone.status === 'submitted') {
        var metaDiv = el('div', { className: 'milestone-meta' });
        metaDiv.appendChild(el('span', {
          className: 'deliverable-link',
          textContent: '交付物：' + (milestone.deliverable_url || '—')
        }));
        content.appendChild(metaDiv);
      }

      item.appendChild(content);
      timeline.appendChild(item);
    });

    // 绑定里程碑操作事件
    timeline.querySelectorAll('.btn-submit-milestone').forEach(function (btn) {
      btn.addEventListener('click', function () {
        submitMilestone(this.dataset.contractId, this.dataset.milestoneId, this.dataset.milestoneTitle);
      });
    });
    timeline.querySelectorAll('.btn-accept-milestone').forEach(function (btn) {
      btn.addEventListener('click', function () {
        acceptMilestone(this.dataset.contractId, this.dataset.milestoneId, true);
      });
    });
    timeline.querySelectorAll('.btn-reject-milestone').forEach(function (btn) {
      btn.addEventListener('click', function () {
        acceptMilestone(this.dataset.contractId, this.dataset.milestoneId, false);
      });
    });
  }

  // 里程碑状态翻译
  function translateMilestoneStatus(status) {
    var map = {
      pending: '待处理',
      submitted: '已提交',
      accepted: '已验收',
      rejected: '已拒绝',
      disputed: '争议中'
    };
    return map[status] || safeText(status);
  }

  // 提交里程碑交付物
  function submitMilestone(contractId, milestoneId, milestoneTitle) {
    var deliverableUrl = prompt('请输入「' + milestoneTitle + '」的交付物 URL：');
    if (!deliverableUrl || !deliverableUrl.trim()) return;

    request('/api/v1/contracts/' + contractId + '/milestones/' + milestoneId + '/submit', {
      method: 'POST',
      body: JSON.stringify({ deliverable_url: deliverableUrl.trim() })
    }).then(function () {
      toast('交付物已提交', 'success');
      if (state.currentContract && state.currentContract.id === contractId) {
        showContractDetail(contractId);
      }
      loadAgentContracts();
    }).catch(function (err) {
      toast('提交失败：' + err.message, 'error');
    });
  }

  // 验收/拒绝里程碑
  function acceptMilestone(contractId, milestoneId, accepted) {
    var comment = null;
    if (!accepted) {
      comment = prompt('请输入拒绝原因（可选）：');
      if (comment === null) return;
    }

    request('/api/v1/contracts/' + contractId + '/milestones/' + milestoneId + '/accept', {
      method: 'POST',
      body: JSON.stringify({ accepted: accepted, comment: comment || null })
    }).then(function () {
      toast(accepted ? '已验收通过' : '已拒绝', 'success');
      if (state.currentContract && state.currentContract.id === contractId) {
        showContractDetail(contractId);
      }
      loadAgentContracts();
    }).catch(function (err) {
      toast('操作失败：' + err.message, 'error');
    });
  }

  // 打开里程碑编辑器（从已成交 Space 创建合同）
  function openMilestoneEditor(spaceId) {
    dom.milestoneEditorForm.reset();
    dom.milestoneEditorList.textContent = '';

    // 添加初始行
    addMilestoneEditorRow();

    dom.milestoneEditorModal.style.display = 'flex';

    // 保存 spaceId 到表单 dataset
    dom.milestoneEditorForm.dataset.spaceId = spaceId;
  }

  // 添加里程碑编辑行
  function addMilestoneEditorRow() {
    var row = el('div', { className: 'milestone-editor-row' });
    row.innerHTML =
      '<input type="text" class="login-input milestone-title-input" placeholder="里程碑标题" required>' +
      '<input type="number" class="login-input milestone-amount-input" placeholder="金额" style="width:120px;">' +
      '<input type="date" class="login-input milestone-date-input" style="width:160px;">' +
      '<button type="button" class="btn-sm btn-remove-row" style="color:var(--status-cancelled)">×</button>';

    row.querySelector('.btn-remove-row').addEventListener('click', function () {
      if (dom.milestoneEditorList.querySelectorAll('.milestone-editor-row').length > 1) {
        row.remove();
      } else {
        toast('至少保留一个里程碑', 'info');
      }
    });

    dom.milestoneEditorList.appendChild(row);
  }

  // 提交创建合同
  function handleCreateContract(event) {
    event.preventDefault();
    var spaceId = dom.milestoneEditorForm.dataset.spaceId;
    if (!spaceId) return;

    var milestones = [];
    var rows = dom.milestoneEditorList.querySelectorAll('.milestone-editor-row');
    rows.forEach(function (row) {
      var title = row.querySelector('.milestone-title-input').value.trim();
      var amount = row.querySelector('.milestone-amount-input').value;
      var dueDate = row.querySelector('.milestone-date-input').value;
      if (!title) return;

      var milestone = { title: title };
      if (amount) milestone.amount = Number(amount);
      if (dueDate) milestone.due_date = dueDate;
      milestones.push(milestone);
    });

    if (!milestones.length) return toast('请至少添加一个里程碑', 'error');

    request('/api/v1/spaces/' + spaceId + '/contract', {
      method: 'POST',
      body: JSON.stringify({ milestones: milestones })
    }).then(function () {
      toast('合同创建成功', 'success');
      dom.milestoneEditorModal.style.display = 'none';
      loadAgentContracts();
    }).catch(function (err) {
      toast('创建合同失败：' + err.message, 'error');
    });
  }

  // 打开争议弹窗
  function openDisputeModal(contractId) {
    dom.disputeForm.reset();
    dom.disputeForm.dataset.contractId = contractId;
    dom.disputeModal.style.display = 'flex';
  }

  // 提交争议
  function handleDispute(event) {
    event.preventDefault();
    var contractId = dom.disputeForm.dataset.contractId;
    var reason = dom.disputeReason.value.trim();
    if (!reason) return toast('请输入争议原因', 'error');

    request('/api/v1/contracts/' + contractId + '/dispute', {
      method: 'POST',
      body: JSON.stringify({ reason: reason })
    }).then(function () {
      toast('争议已发起', 'success');
      dom.disputeModal.style.display = 'none';
      loadAgentContracts();
    }).catch(function (err) {
      toast('发起争议失败：' + err.message, 'error');
    });
  }

  // ── Need Broadcast ────────────────────────────────────

  function loadNeeds() {
    if (!state.currentAgentId) return;
    var params = [];
    if (dom.needSearchCategory.value) params.push('category=' + encodeURIComponent(dom.needSearchCategory.value));
    if (dom.needSearchSkills.value.trim()) params.push('skills=' + encodeURIComponent(dom.needSearchSkills.value.trim()));
    var query = params.length ? ('?' + params.join('&')) : '';
    request('/api/v1/needs' + query, { allowUnauthorized: true }).then(function (data) {
      state.needs = Array.isArray(data && data.items) ? data.items : (Array.isArray(data) ? data : []);
      renderNeeds();
    }).catch(function (err) {
      dom.needResults.innerHTML = '<div class="empty-state"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/></svg><h3>加载失败</h3><p>' + safeText(err.message) + '</p></div>';
    });
  }

  function renderNeeds() {
    dom.needResultCount.textContent = '共 ' + state.needs.length + ' 个需求';
    dom.needResults.textContent = '';
    if (!state.needs.length) {
      dom.needResults.innerHTML = '<div class="empty-state"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/></svg><h3>暂无需求</h3><p>暂无匹配的开放需求。</p></div>';
      return;
    }
    state.needs.forEach(function (need) {
      var card = el('article', { className: 'provider-card' });
      card.appendChild(el('div', {
        className: 'provider-card-head',
        innerHTML: '<div><h3>' + safeText(need.title) + '</h3><p>' + translateNeedCategory(need.category) + ' · ' + shortId(need.creator_id) + '</p></div><span class="space-badge active">' + translateNeedStatus(need.status || 'open') + '</span>'
      }));
      if (need.description) {
        card.appendChild(el('p', { className: 'provider-desc', textContent: need.description.length > 120 ? need.description.slice(0, 120) + '...' : need.description }));
      }
      var metaParts = [];
      if (need.budget_min != null || need.budget_max != null) metaParts.push('预算：' + formatPrice(need.budget_min, need.budget_max));
      if (need.deadline) metaParts.push('截止：' + formatTime(need.deadline));
      if (need.matched_provider_count) metaParts.push('已匹配 ' + need.matched_provider_count + ' 个 Provider');
      if (metaParts.length) {
        card.appendChild(el('div', { className: 'provider-meta-line', textContent: metaParts.join(' · ') }));
      }
      if (need.required_skills && need.required_skills.length) {
        var tags = el('div', { className: 'chip-list compact' });
        need.required_skills.slice(0, 5).forEach(function (skill) {
          tags.appendChild(el('span', { className: 'chip', textContent: skill }));
        });
        card.appendChild(tags);
      }
      var actions = el('div', { className: 'inline-actions' });
      actions.appendChild(el('button', {
        className: 'btn-sm',
        type: 'button',
        textContent: '响应需求',
        onclick: function () {
          setView('arena');
          dom.bilateralInviteeId.value = need.creator_id;
          dom.createSpaceName.value = need.title + ' 协商';
          dom.createSpaceDescription.value = need.description || '';
        }
      }));
      card.appendChild(actions);
      dom.needResults.appendChild(card);
    });
  }

  function publishNeed(data) {
    var agent = currentAgent();
    if (!agent) return Promise.reject(new Error('请先选择 Agent'));
    return request('/api/v1/needs', {
      method: 'POST',
      token: agent.api_key,
      body: JSON.stringify(data)
    }).then(function (need) {
      toast('需求已发布：' + (need.title || data.title), 'success');
      dom.publishNeedForm.reset();
      loadMyNeeds();
      loadNeeds();
      return need;
    }).catch(function (err) {
      toast('发布需求失败：' + err.message, 'error');
      throw err;
    });
  }

  function loadMyNeeds() {
    if (!state.currentAgentId) return;
    var agent = currentAgent();
    if (!agent) return;
    request('/api/v1/needs/my', { token: agent.api_key }).then(function (needs) {
      state.myNeeds = Array.isArray(needs) ? needs : [];
      renderMyNeeds();
    }).catch(function () {
      state.myNeeds = [];
      renderMyNeeds();
    });
  }

  function renderMyNeeds() {
    dom.myNeedsList.textContent = '';
    if (!state.myNeeds.length) {
      dom.myNeedsList.innerHTML = '<div class="empty-state"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M12 4v16m8-8H4"/></svg><h3>暂无需求</h3><p>暂无已发布的需求。</p></div>';
      return;
    }
    state.myNeeds.forEach(function (need) {
      var row = el('div', { className: 'overview-row' });
      var info = el('div', { style: 'flex:1;min-width:0;' });
      info.appendChild(el('span', { className: 'overview-row-title', textContent: need.title || '未命名需求' }));
      info.appendChild(el('span', { className: 'overview-row-meta', textContent: translateNeedCategory(need.category) + ' · ' + translateNeedStatus(need.status) + ' · ' + formatTime(need.created_at) }));
      row.appendChild(info);
      row.appendChild(el('span', {
        className: 'space-badge ' + (need.status === 'open' ? 'active' : need.status === 'matched' ? 'concluded' : 'created'),
        textContent: translateNeedStatus(need.status)
      }));
      if (need.status === 'open') {
        row.appendChild(el('button', {
          className: 'btn-sm',
          type: 'button',
          textContent: '发起 RFP',
          style: 'margin-left:8px;',
          onclick: function () { openCreateRfpFromNeedModal(need.id); }
        }));
        row.appendChild(el('button', {
          className: 'btn-sm',
          type: 'button',
          textContent: '取消',
          style: 'margin-left:8px;',
          onclick: function () { cancelNeed(need.id); }
        }));
      }
      dom.myNeedsList.appendChild(row);
    });
  }

  function cancelNeed(needId) {
    var agent = currentAgent();
    if (!agent) return toast('请先选择 Agent', 'error');
    if (!confirm('确定要取消此需求吗？')) return;
    request('/api/v1/needs/' + needId + '/cancel', {
      method: 'POST',
      token: agent.api_key
    }).then(function () {
      toast('需求已取消', 'success');
      loadMyNeeds();
      loadNeeds();
    }).catch(function (err) {
      toast('取消需求失败：' + err.message, 'error');
    });
  }

  function setupFilters() {
    Array.prototype.slice.call(document.querySelectorAll('.filter-btn')).forEach(function (button) {
      button.addEventListener('click', function () {
        Array.prototype.slice.call(document.querySelectorAll('.filter-btn')).forEach(function (node) {
          node.classList.remove('active');
        });
        button.classList.add('active');
        state.currentFilter = button.dataset.filter;
        renderSpaceList();
      });
    });
  }

  function setupViewButtons() {
    dom.viewButtons.forEach(function (button) {
      button.addEventListener('click', function () {
        setView(button.dataset.view);
      });
    });
  }

  function setupDocsPage() {
    window.copyCode = function (btn) {
      var code = btn.closest('.code-block').querySelector('code');
      navigator.clipboard.writeText(code.textContent).then(function () {
        btn.textContent = '已复制';
        setTimeout(function () { btn.textContent = '复制'; }, 1500);
      });
    };

    // Docs sidebar active link tracking
    var links = document.querySelectorAll('.docs-sidebar-link');
    var sections = document.querySelectorAll('.docs-content h2[id]');
    if (!links.length || !sections.length) return;

    var observer = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          links.forEach(function (l) { l.classList.remove('active'); });
          var active = document.querySelector('.docs-sidebar-link[href="#' + entry.target.id + '"]');
          if (active) active.classList.add('active');
        }
      });
    }, { rootMargin: '-80px 0px -60% 0px' });

    sections.forEach(function (s) { observer.observe(s); });

    // Smooth scroll on sidebar click
    links.forEach(function (link) {
      link.addEventListener('click', function (e) {
        e.preventDefault();
        var id = link.getAttribute('href').slice(1);
        var target = document.getElementById(id);
        if (target) target.scrollIntoView({ behavior: 'smooth', block: 'start' });
      });
    });
  }

  function initConsolePage() {
    var navSignInBtn = document.getElementById('navSignIn');
    var navGetStartedBtn = document.getElementById('navGetStarted');
    var heroGetStartedBtn = document.getElementById('heroGetStarted');
    var ctaGetStartedBtn = document.getElementById('ctaGetStarted');

    if (navSignInBtn) navSignInBtn.addEventListener('click', function () { openAuthModal('login'); });
    if (navGetStartedBtn) navGetStartedBtn.addEventListener('click', function () { openAuthModal('register'); });
    if (heroGetStartedBtn) heroGetStartedBtn.addEventListener('click', function () { openAuthModal('register'); });
    if (ctaGetStartedBtn) ctaGetStartedBtn.addEventListener('click', function () { openAuthModal('register'); });
    dom.loginForm.addEventListener('submit', handleLogin);
    dom.registerForm.addEventListener('submit', handleRegister);
    dom.showRegister.addEventListener('click', function (event) {
      event.preventDefault();
      openAuthModal('register');
    });
    dom.showLogin.addEventListener('click', function (event) {
      event.preventDefault();
      openAuthModal('login');
    });
    dom.authModal.addEventListener('click', function (event) {
      if (event.target === dom.authModal) closeAuthModal();
    });
    dom.logoutBtn.addEventListener('click', function (event) {
      event.preventDefault();
      handleLogout();
    });
    dom.agentSelect.addEventListener('change', function () {
      if (dom.agentSelect.value) selectAgent(dom.agentSelect.value);
    });
    dom.createAgentBtn2.addEventListener('click', function () {
      dom.createAgentModal.style.display = 'flex';
    });
    dom.closeAgentModal.addEventListener('click', function () {
      dom.createAgentModal.style.display = 'none';
      dom.agentResult.style.display = 'none';
      dom.createAgentForm.reset();
    });
    dom.createAgentForm.addEventListener('submit', handleCreateAgent);
    dom.detailClose.addEventListener('click', closeSpaceDetail);
    dom.mobileToggle.addEventListener('click', function () {
      dom.sidebar.classList.toggle('open');
      dom.navLinks.classList.toggle('open');
    });
    dom.quickDiscovery.addEventListener('click', function () { setView('nexus'); });
    dom.quickCreateNegotiation.addEventListener('click', function () { setView('arena'); });
    dom.quickCreateRfp.addEventListener('click', function () { setView('arena'); });
    dom.overviewRefreshSpaces.addEventListener('click', loadSpaces);
    dom.focusCreateRating.addEventListener('click', function () { setView('ledger'); });
    dom.providerSearchForm.addEventListener('submit', function (event) {
      event.preventDefault();
      loadDiscoveryProviders();
      if (window.closeBottomSheet) window.closeBottomSheet('providerSearchSheet');
    });
    dom.loadAllProviders.addEventListener('click', function () {
      dom.providerSearchForm.reset();
      loadDiscoveryProviders();
      if (window.closeBottomSheet) window.closeBottomSheet('providerSearchSheet');
    });
    dom.useSelectedProvider.addEventListener('click', function () {
      if (!state.selectedProvider) return toast('请先在发现页选择 Provider', 'info');
      dom.bilateralInviteeId.value = state.selectedProvider.agent_id;
      dom.createSpaceName.value = state.selectedProvider.display_name + ' 协商';
    });
    dom.syncRfpCandidates.addEventListener('click', syncRfpCandidateInput);
    dom.createSpaceForm.addEventListener('submit', handleCreateSpace);
    dom.createRfpForm.addEventListener('submit', handleCreateRfp);
    dom.rateSpaceSelect.addEventListener('change', updateRateTargets);
    dom.rateAgentForm.addEventListener('submit', handleRateSubmit);
    dom.providerProfileForm.addEventListener('submit', handleProviderProfileSubmit);
    dom.publishNeedForm.addEventListener('submit', function (event) {
      event.preventDefault();
      var skills = parseCommaList(dom.needSkills.value);
      var data = {
        title: dom.needTitle.value.trim(),
        description: dom.needDescription.value.trim(),
        category: dom.needCategory.value,
        required_skills: skills.length ? skills : [],
        budget_min: dom.needBudgetMin.value ? Number(dom.needBudgetMin.value) : null,
        budget_max: dom.needBudgetMax.value ? Number(dom.needBudgetMax.value) : null,
        deadline: dom.needDeadline.value ? new Date(dom.needDeadline.value).getTime() : null
      };
      if (!data.title || !data.category || !data.description) return toast('请填写标题、分类和描述', 'error');
      publishNeed(data);
    });
    dom.refreshMyNeeds.addEventListener('click', loadMyNeeds);
    dom.needSearchForm.addEventListener('submit', function (event) {
      event.preventDefault();
      loadNeeds();
    });
    dom.loadAllNeeds.addEventListener('click', function () {
      dom.needSearchForm.reset();
      loadNeeds();
    });
    // Phase 3: RFP Negotiation Visualization
    dom.advanceRoundBtn.addEventListener('click', handleAdvanceRound);
    setupEvaluateSliders();
    dom.evaluateBtn.addEventListener('click', handleEvaluate);
    dom.closeCreateRfpFromNeed.addEventListener('click', closeCreateRfpFromNeedModal);
    dom.cancelRfpFromNeed.addEventListener('click', closeCreateRfpFromNeedModal);
    dom.createRfpFromNeedForm.addEventListener('submit', handleCreateRfpFromNeed);
    dom.createRfpFromNeedModal.addEventListener('click', function (e) {
      if (e.target === dom.createRfpFromNeedModal) closeCreateRfpFromNeedModal();
    });

    // Phase 4: 合同管理事件绑定
    var myContractsBtn = document.getElementById('myContractsBtn');
    if (myContractsBtn) myContractsBtn.addEventListener('click', loadAgentContracts);
    if (dom.closeContractsModal) dom.closeContractsModal.addEventListener('click', function () {
      dom.contractsModal.style.display = 'none';
    });
    if (dom.contractsModal) dom.contractsModal.addEventListener('click', function (e) {
      if (e.target === dom.contractsModal) dom.contractsModal.style.display = 'none';
    });

    setupFilters();
    setupViewButtons();
    setupDocsPage();
    checkHealth();
    state.healthTimer = setInterval(checkHealth, HEALTH_INTERVAL);
    checkAuth();
  }

  function init() {
    setupDocsPage();
    if (!isConsolePage()) return;
    initConsolePage();
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();

// ── Global helpers ────────────────────────────────────

function copyText(elementId) {
  var el = document.getElementById(elementId);
  if (!el) return;
  var text = el.textContent || '';
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text);
  } else {
    var range = document.createRange();
    range.selectNodeContents(el);
    var sel = window.getSelection();
    sel.removeAllRanges();
    sel.addRange(range);
    document.execCommand('copy');
    sel.removeAllRanges();
  }
  // 简单反馈
  var btn = el.parentElement && el.parentElement.querySelector('.copy-btn-sm');
  if (btn) {
    var orig = btn.textContent;
    btn.textContent = '已复制';
    setTimeout(function () { btn.textContent = orig; }, 1500);
  }
}
