// ── Gaggle — app.js ────────────────────────────────────
// Landing + Docs + 中文控制台（Provider 发现 / Profile / 创建谈判 / RFP / 信誉）

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
    currentView: 'overview',
    discoveryResults: [],
    selectedProvider: null,
    providerReputation: null,
    rfpCandidates: [],
    currentReputation: null,
    currentProfile: null,
    spaceKeys: {},
    agentNames: {},
    pendingMessages: {}
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
    profileMaxPrice: document.getElementById('profileMaxPrice')
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
      space_closed: '空间已关闭：' + translateStatus(data.payload && data.payload.conclusion)
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
      state.spaceKeys = {};
      state.spaces.forEach(function (space) {
        if (space.encryption_key) state.spaceKeys[space.id] = space.encryption_key;
      });
      renderSpaceList();
      renderOverview();
      populateRateOptions();
      if (state.selectedSpaceId) {
        var exists = state.spaces.some(function (space) { return space.id === state.selectedSpaceId; });
        if (exists) selectSpace(state.selectedSpaceId);
        else closeSpaceDetail();
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
    dom.detailView.classList.remove('visible');
    dom.emptyState.style.display = '';
    dom.selectedSpaceSummary.className = 'selected-space-summary empty';
    dom.selectedSpaceSummary.innerHTML = '<p>从左侧选择一个空间，查看成员、上下文、消息流与提案时间线。</p>';
    renderSpaceList();
    renderOverview();
  }

  function decryptMessage(encryptionKey, content) {
    if (!encryptionKey || !content || !content.cipher || !content.nonce) return Promise.resolve('[已加密]');
    return crypto.subtle.digest('SHA-256', new TextEncoder().encode(encryptionKey))
      .then(function (hash) {
        return crypto.subtle.importKey('raw', hash, 'AES-GCM', false, ['decrypt']);
      })
      .then(function (key) {
        var nonce = Uint8Array.from(atob(content.nonce), function (char) { return char.charCodeAt(0); });
        var cipher = Uint8Array.from(atob(content.cipher), function (char) { return char.charCodeAt(0); });
        return crypto.subtle.decrypt({ name: 'AES-GCM', iv: nonce }, key, cipher);
      })
      .then(function (plain) { return new TextDecoder().decode(plain); })
      .catch(function () { return '[已加密]'; });
  }

  function resolveContent(content, spaceId) {
    if (!content) return Promise.resolve('[无内容]');
    if (typeof content === 'string') return Promise.resolve(content);
    if (typeof content === 'object' && content.cipher) return decryptMessage(state.spaceKeys[spaceId], content);
    return Promise.resolve(JSON.stringify(content, null, 2));
  }

  function loadMessages(spaceId) {
    dom.messageList.innerHTML = '<div class="spinner"></div>';
    request('/api/v1/spaces/' + spaceId + '/messages?limit=100').then(function (messages) {
      renderMessages(Array.isArray(messages) ? messages : [], spaceId);
    }).catch(function (err) {
      dom.messageList.innerHTML = '<p style="padding:16px;color:var(--text-muted)">加载消息失败：' + err.message + '</p>';
    });
  }

  function renderMessages(messages, spaceId) {
    dom.msgCount.textContent = '(' + messages.length + ')';
    dom.messageList.textContent = '';
    if (!messages.length) {
      dom.messageList.appendChild(el('p', { textContent: '暂无消息。', style: 'color:var(--text-muted)' }));
      return;
    }
    var selfId = state.currentAgentId;
    var chain = Promise.resolve();
    messages.forEach(function (message) {
      chain = chain.then(function () {
        return resolveContent(message.content, spaceId).then(function (text) {
          dom.messageList.appendChild(buildBubble(message, text, selfId));
        });
      });
    });
    chain.then(function () {
      dom.messageList.scrollTop = dom.messageList.scrollHeight;
    });
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
      dom.proposalTimeline.innerHTML = '<p style="color:var(--text-muted)">暂无提案。</p>';
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
    pending.forEach(function (msg) { appendMessage(msg, spaceId); });
    delete state.pendingMessages[spaceId];
  }

  function startAutoRefresh() {
    stopAutoRefresh();
    state.refreshTimer = setInterval(function () {
      loadSpaces();
      if (state.selectedSpaceId) {
        loadMessages(state.selectedSpaceId);
        loadProposals(state.selectedSpaceId);
      }
    }, REFRESH_INTERVAL);
  }

  function stopAutoRefresh() {
    if (state.refreshTimer) {
      clearInterval(state.refreshTimer);
      state.refreshTimer = null;
    }
  }

  function appendMessage(message, spaceId) {
    resolveContent(message.content, spaceId).then(function (text) {
      var bubble = buildBubble(message, text, state.currentAgentId || '');
      dom.messageList.appendChild(bubble);
      dom.messageList.scrollTop = dom.messageList.scrollHeight;
      dom.msgCount.textContent = '(' + dom.messageList.querySelectorAll('.message-bubble').length + ')';
    });
  }

  function setView(view) {
    state.currentView = view;
    Array.prototype.slice.call(document.querySelectorAll('.app-view')).forEach(function (section) {
      section.classList.toggle('active', section.id === view + 'View');
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
      dom.providerResults.innerHTML = '<p style="color:var(--text-muted)">加载 Provider 失败：' + err.message + '</p>';
    });
  }

  function renderDiscoveryResults() {
    dom.providerResultCount.textContent = '共 ' + state.discoveryResults.length + ' 个 Provider';
    dom.providerResults.textContent = '';
    if (!state.discoveryResults.length) {
      dom.providerResults.innerHTML = '<p style="color:var(--text-muted)">未发现匹配的 Provider，可尝试放宽筛选条件。</p>';
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
          setView('create');
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
      setView('create');
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
      dom.myRole.value = 'buyer';
      setView('overview');
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
      state.rfpCandidates = [];
      renderRfpCandidates();
      setView('overview');
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
    document.getElementById('navSignIn').addEventListener('click', function () { openAuthModal('login'); });
    document.getElementById('navGetStarted').addEventListener('click', function () { openAuthModal('register'); });
    document.getElementById('heroGetStarted').addEventListener('click', function () { openAuthModal('register'); });
    document.getElementById('ctaGetStarted').addEventListener('click', function () { openAuthModal('register'); });
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
    dom.quickDiscovery.addEventListener('click', function () { setView('discovery'); });
    dom.quickCreateNegotiation.addEventListener('click', function () { setView('create'); });
    dom.quickCreateRfp.addEventListener('click', function () { setView('create'); });
    dom.overviewRefreshSpaces.addEventListener('click', loadSpaces);
    dom.focusCreateRating.addEventListener('click', function () { setView('reputation'); });
    dom.providerSearchForm.addEventListener('submit', function (event) {
      event.preventDefault();
      loadDiscoveryProviders();
    });
    dom.loadAllProviders.addEventListener('click', function () {
      dom.providerSearchForm.reset();
      loadDiscoveryProviders();
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
