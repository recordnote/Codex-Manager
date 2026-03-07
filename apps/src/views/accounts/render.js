import { state } from "../../state.js";
import { dom } from "../../ui/dom.js";
import { formatLimitLabel, formatTs } from "../../utils/format.js";
import { getRefreshAllProgress } from "../../services/management/account-actions.js";
import {
  buildAccountDerivedMap,
  buildGroupFilterOptions,
  filterAccounts,
  normalizeGroupName,
} from "./state.js";

const ACCOUNT_ACTION_OPEN_USAGE = "open-usage";
const ACCOUNT_ACTION_SET_CURRENT = "set-current";
const ACCOUNT_ACTION_DELETE = "delete";
const ACCOUNT_FIELD_SELECT = "selected";
const ACCOUNT_PAGE_SIZE_OPTIONS = [5, 10, 20, 50, 80, 120, 500];
const DEFAULT_ACCOUNT_PAGE_SIZE = 5;

let accountRowsEventsBoundEl = null;
let accountRowsClickHandler = null;
let accountRowsChangeHandler = null;
let accountSelectAllBoundEl = null;
let accountSelectAllChangeHandler = null;
let accountPaginationBoundRefs = null;
let accountPageSizeChangeHandler = null;
let accountPagePrevClickHandler = null;
let accountPageNextClickHandler = null;
let accountRowHandlers = null;
let accountLookupById = new Map();
let accountRowNodesById = new Map();
let groupOptionsAccountsRef = null;
let groupOptionsCache = [];
let groupSelectRenderedKey = null;
let refreshProgressNode = null;
let derivedCacheAccountsRef = null;
let derivedCacheUsageRef = null;
let derivedCacheMap = new Map();

function ensureRefreshProgressNode() {
  if (refreshProgressNode?.isConnected) {
    return refreshProgressNode;
  }
  if (!dom.accountsToolbar) {
    return null;
  }
  const existing = dom.accountsToolbar.querySelector(".accounts-refresh-progress");
  if (existing) {
    refreshProgressNode = existing;
    return refreshProgressNode;
  }
  const node = document.createElement("div");
  node.className = "accounts-refresh-progress";
  node.hidden = true;
  node.setAttribute("aria-live", "polite");
  dom.accountsToolbar.prepend(node);
  refreshProgressNode = node;
  return refreshProgressNode;
}

export function renderAccountsRefreshProgress(progress = getRefreshAllProgress()) {
  const node = ensureRefreshProgressNode();
  if (!node) return;
  const total = Math.max(0, Number(progress?.total || 0));
  const completed = Math.min(total, Math.max(0, Number(progress?.completed || 0)));
  const remaining = Math.max(0, Number(progress?.remaining ?? total - completed));
  const active = Boolean(progress?.active) && Boolean(progress?.manual) && total > 0;
  if (!active) {
    node.hidden = true;
    node.textContent = "";
    return;
  }
  const primaryText = `刷新进度 ${completed}/${total}，剩余 ${remaining} 项`;
  const lastTaskLabel = String(progress?.lastTaskLabel || "").trim();
  node.hidden = false;
  node.textContent = lastTaskLabel ? `${primaryText} · 最近完成：${lastTaskLabel}` : primaryText;
}

function getSelectedAccountIdSet() {
  if (state.selectedAccountIds instanceof Set) {
    return state.selectedAccountIds;
  }
  const next = new Set(Array.isArray(state.selectedAccountIds) ? state.selectedAccountIds : []);
  state.selectedAccountIds = next;
  return next;
}

function pruneSelectedAccountIds(accounts = state.accountList) {
  const selected = getSelectedAccountIdSet();
  if (selected.size === 0) {
    return;
  }
  const validIds = new Set(
    Array.from(accounts || [])
      .map((item) => String(item?.id || "").trim())
      .filter(Boolean),
  );
  for (const accountId of Array.from(selected)) {
    if (!validIds.has(accountId)) {
      selected.delete(accountId);
    }
  }
}

function isAccountSelected(accountId) {
  if (!accountId) return false;
  return getSelectedAccountIdSet().has(accountId);
}

function setAccountSelected(accountId, selected) {
  const normalizedId = String(accountId || "").trim();
  if (!normalizedId) return false;
  const selectedIds = getSelectedAccountIdSet();
  const had = selectedIds.has(normalizedId);
  if (selected) {
    if (!had) {
      selectedIds.add(normalizedId);
      return true;
    }
    return false;
  }
  if (had) {
    selectedIds.delete(normalizedId);
    return true;
  }
  return false;
}

function setPageSelection(accounts, selected) {
  let changed = false;
  for (const account of accounts || []) {
    changed = setAccountSelected(account?.id, selected) || changed;
  }
  return changed;
}

function syncAccountSelectionControls(accounts) {
  const pageItems = Array.isArray(accounts) ? accounts : [];
  const selectedIds = getSelectedAccountIdSet();
  const currentPageIds = pageItems
    .map((item) => String(item?.id || "").trim())
    .filter(Boolean);
  let selectedOnPage = 0;
  for (const accountId of currentPageIds) {
    if (selectedIds.has(accountId)) {
      selectedOnPage += 1;
    }
  }

  if (dom.accountSelectAll) {
    const allSelected = currentPageIds.length > 0 && selectedOnPage === currentPageIds.length;
    dom.accountSelectAll.disabled = currentPageIds.length === 0;
    dom.accountSelectAll.checked = allSelected;
    dom.accountSelectAll.indeterminate =
      selectedOnPage > 0 && selectedOnPage < currentPageIds.length;
  }

  if (dom.deleteSelectedAccountsBtn) {
    const count = selectedIds.size;
    dom.deleteSelectedAccountsBtn.disabled = count === 0;
    dom.deleteSelectedAccountsBtn.textContent = count > 0
      ? `删除选中账号（${count}）`
      : "删除选中账号";
  }
}

function getGroupOptions(accounts) {
  const list = Array.isArray(accounts) ? accounts : [];
  if (groupOptionsAccountsRef !== accounts) {
    groupOptionsAccountsRef = accounts;
    groupOptionsCache = buildGroupFilterOptions(list);
  } else if (Array.isArray(groupOptionsCache) && groupOptionsCache.length > 0) {
    // 保持“全部分组”计数与当前账号数量一致。
    groupOptionsCache[0] = {
      ...groupOptionsCache[0],
      count: list.length,
    };
  }
  return groupOptionsCache;
}

function syncGroupFilterSelect(options, optionsKey) {
  if (!dom.accountGroupFilter) return;
  const select = dom.accountGroupFilter;
  const safeOptions = Array.isArray(options) ? options : [];
  const nextValues = new Set(safeOptions.map((item) => item.value));

  // 中文注释：分组来自实时账号数据；若分组被删除/重命名，不自动回退会导致列表“看似空白”且用户难定位原因。
  if (!nextValues.has(state.accountGroupFilter)) {
    state.accountGroupFilter = "all";
  }

  if (groupSelectRenderedKey === optionsKey && select.children.length === safeOptions.length) {
    if (select.value !== state.accountGroupFilter) {
      select.value = state.accountGroupFilter;
    }
    return;
  }

  select.innerHTML = "";
  for (const option of safeOptions) {
    const node = document.createElement("option");
    node.value = option.value;
    node.textContent = `${option.label} (${option.count})`;
    if (option.value === state.accountGroupFilter) {
      node.selected = true;
    }
    select.appendChild(node);
  }
  groupSelectRenderedKey = optionsKey;
  if (!nextValues.has(state.accountGroupFilter)) {
    select.value = "all";
  }
}

function getAccountDerivedMapCached(accounts, usageSource) {
  if (derivedCacheAccountsRef === accounts && derivedCacheUsageRef === usageSource) {
    return derivedCacheMap;
  }
  derivedCacheAccountsRef = accounts;
  derivedCacheUsageRef = usageSource;
  derivedCacheMap = buildAccountDerivedMap(accounts, usageSource);
  return derivedCacheMap;
}

function renderMiniUsageLine(label, remain, secondary) {
  const line = document.createElement("div");
  line.className = "progress-line";
  if (secondary) line.classList.add("secondary");
  const text = document.createElement("span");
  text.textContent = `${label} ${remain == null ? "--" : `${remain}%`}`;
  const track = document.createElement("div");
  track.className = "track";
  const fill = document.createElement("div");
  fill.className = "fill";
  fill.style.width = remain == null ? "0%" : `${remain}%`;
  track.appendChild(fill);
  line.appendChild(text);
  line.appendChild(track);
  return line;
}

function createSelectCell(account) {
  const cellSelect = document.createElement("td");
  cellSelect.className = "account-col-select";
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.className = "account-select-checkbox";
  checkbox.setAttribute("data-field", ACCOUNT_FIELD_SELECT);
  checkbox.checked = isAccountSelected(account?.id);
  checkbox.setAttribute("aria-label", `选择账号 ${account?.label || account?.id || ""}`);
  cellSelect.appendChild(checkbox);
  return cellSelect;
}

function createStatusTag(status) {
  const statusTag = document.createElement("span");
  statusTag.className = "status-tag";
  statusTag.textContent = status.text;
  if (status.level === "ok") statusTag.classList.add("status-ok");
  if (status.level === "warn") statusTag.classList.add("status-warn");
  if (status.level === "bad") statusTag.classList.add("status-bad");
  if (status.level === "unknown") statusTag.classList.add("status-unknown");
  return statusTag;
}

function createAccountCell(account, accountDerived) {
  const cellAccount = document.createElement("td");
  cellAccount.className = "account-col-account";
  const accountWrap = document.createElement("div");
  accountWrap.className = "cell-stack";
  const primaryRemain = accountDerived?.primaryRemain ?? null;
  const secondaryRemain = accountDerived?.secondaryRemain ?? null;
  const accountTitle = document.createElement("strong");
  accountTitle.textContent = account.label || "-";
  const accountMeta = document.createElement("small");
  accountMeta.textContent = `${account.id || "-"}`;
  accountWrap.appendChild(accountTitle);
  accountWrap.appendChild(accountMeta);
  const mini = document.createElement("div");
  mini.className = "mini-usage";
  const usage = accountDerived?.usage || null;
  const hasPrimaryWindow = usage?.usedPercent != null && usage?.windowMinutes != null;
  const hasSecondaryWindow =
    usage?.secondaryUsedPercent != null
    || usage?.secondaryWindowMinutes != null;

  if (hasPrimaryWindow) {
    const primaryLabel = formatLimitLabel(usage?.windowMinutes, "5小时");
    mini.appendChild(
      renderMiniUsageLine(primaryLabel, primaryRemain, false),
    );
  }

  if (hasSecondaryWindow) {
    mini.appendChild(
      renderMiniUsageLine("7天", secondaryRemain, true),
    );
  }
  accountWrap.appendChild(mini);
  cellAccount.appendChild(accountWrap);
  return cellAccount;
}

function createGroupCell(account) {
  const cellGroup = document.createElement("td");
  cellGroup.className = "account-col-group";
  cellGroup.textContent = normalizeGroupName(account.groupName) || "-";
  return cellGroup;
}

function createSortCell(account) {
  const cellSort = document.createElement("td");
  cellSort.className = "account-col-sort";
  const sortInput = document.createElement("input");
  sortInput.className = "sort-input";
  sortInput.type = "number";
  sortInput.setAttribute("data-field", "sort");
  sortInput.value = account.sort != null ? String(account.sort) : "0";
  sortInput.dataset.originSort = sortInput.value;
  cellSort.appendChild(sortInput);
  return cellSort;
}

function createUpdatedCell(usage) {
  const cellUpdated = document.createElement("td");
  cellUpdated.className = "account-col-updated";
  const updatedText = document.createElement("strong");
  updatedText.textContent = usage && usage.capturedAt ? formatTs(usage.capturedAt) : "未知";
  cellUpdated.appendChild(updatedText);
  return cellUpdated;
}

function createActionsCell(isDeletable) {
  const cellActions = document.createElement("td");
  cellActions.className = "account-col-actions";
  const actionsWrap = document.createElement("div");
  actionsWrap.className = "cell-actions";
  const btn = document.createElement("button");
  btn.className = "secondary";
  btn.type = "button";
  btn.setAttribute("data-action", ACCOUNT_ACTION_OPEN_USAGE);
  btn.textContent = "用量查询";
  actionsWrap.appendChild(btn);
  const setCurrent = document.createElement("button");
  setCurrent.className = "secondary";
  setCurrent.type = "button";
  setCurrent.setAttribute("data-action", ACCOUNT_ACTION_SET_CURRENT);
  setCurrent.textContent = "切到当前";
  actionsWrap.appendChild(setCurrent);

  if (isDeletable) {
    const del = document.createElement("button");
    del.className = "danger";
    del.type = "button";
    del.setAttribute("data-action", ACCOUNT_ACTION_DELETE);
    del.textContent = "删除";
    actionsWrap.appendChild(del);
  }
  cellActions.appendChild(actionsWrap);
  return cellActions;
}

function syncSetCurrentButton(actionsWrap, status) {
  if (!actionsWrap) return;
  const btn = actionsWrap.querySelector(`button[data-action="${ACCOUNT_ACTION_SET_CURRENT}"]`);
  if (!btn) return;
  const level = status?.level;
  const disabled = level === "warn" || level === "bad";
  btn.disabled = disabled;
  btn.title = disabled ? `账号当前不可用（${status?.text || "不可用"}），不参与网关选路` : "锁定为当前账号（异常前持续优先使用）";
}

function renderEmptyRow(message) {
  const emptyRow = document.createElement("tr");
  const emptyCell = document.createElement("td");
  emptyCell.colSpan = 7;
  emptyCell.textContent = message;
  emptyRow.appendChild(emptyCell);
  dom.accountRows.appendChild(emptyRow);
}

function renderAccountRow(account, accountDerivedMap, { onDelete }) {
  const row = document.createElement("tr");
  row.setAttribute("data-account-id", account.id || "");
  const accountDerived = accountDerivedMap.get(account.id) || {
    usage: null,
    primaryRemain: null,
    secondaryRemain: null,
    status: { text: "未知", level: "unknown" },
  };

  row.appendChild(createSelectCell(account));
  row.appendChild(createAccountCell(account, accountDerived));
  row.appendChild(createGroupCell(account));
  row.appendChild(createSortCell(account));

  const cellStatus = document.createElement("td");
  cellStatus.className = "account-col-status";
  cellStatus.appendChild(createStatusTag(accountDerived.status));
  row.appendChild(cellStatus);

  row.appendChild(createUpdatedCell(accountDerived.usage));
  const actionsCell = createActionsCell(Boolean(onDelete));
  row.appendChild(actionsCell);
  syncSetCurrentButton(actionsCell.querySelector(".cell-actions"), accountDerived.status);
  return row;
}

function normalizeAccountPageSize(value) {
  const parsed = Number(value);
  if (ACCOUNT_PAGE_SIZE_OPTIONS.includes(parsed)) {
    return parsed;
  }
  return DEFAULT_ACCOUNT_PAGE_SIZE;
}

function clampAccountPage(page, totalPages) {
  const normalized = Number(page);
  if (!Number.isFinite(normalized) || normalized < 1) {
    return 1;
  }
  return Math.min(Math.trunc(normalized), Math.max(1, totalPages));
}

function getAccountPageContext(filtered) {
  const total = Array.isArray(filtered) ? filtered.length : 0;
  const pageSize = normalizeAccountPageSize(state.accountPageSize);
  state.accountPageSize = pageSize;
  const totalPages = Math.max(1, Math.ceil(total / pageSize));
  const page = clampAccountPage(state.accountPage, totalPages);
  state.accountPage = page;
  const startIndex = total > 0 ? (page - 1) * pageSize : 0;
  const endIndex = total > 0 ? Math.min(startIndex + pageSize, total) : 0;
  return {
    total,
    pageSize,
    totalPages,
    page,
    startIndex,
    endIndex,
    items: total > 0 ? filtered.slice(startIndex, endIndex) : [],
  };
}

function rerenderAccountsPage() {
  if (!accountRowHandlers) return;
  renderAccounts(accountRowHandlers);
}

function requestAccountsPageReload() {
  if (typeof accountRowHandlers?.onRefreshPage === "function") {
    void accountRowHandlers.onRefreshPage();
    return;
  }
  rerenderAccountsPage();
}

function getRenderedAccountsContext() {
  const usingRemotePagination = state.accountPageLoaded === true;
  const sourceAccounts = usingRemotePagination ? state.accountPageItems : state.accountList;
  const accountDerivedMap = getAccountDerivedMapCached(sourceAccounts, state.usageList);
  const pageContext = usingRemotePagination
    ? getRemoteAccountPageContext(state.accountPageItems, state.accountPageTotal)
    : getAccountPageContext(filterAccounts(
      state.accountList,
      accountDerivedMap,
      state.accountSearch,
      state.accountFilter,
      state.accountGroupFilter,
    ));
  return {
    usingRemotePagination,
    sourceAccounts,
    accountDerivedMap,
    pageContext,
  };
}

function ensureAccountSelectAllEventsBound() {
  if (!dom.accountSelectAll) {
    return;
  }
  if (!accountSelectAllChangeHandler) {
    accountSelectAllChangeHandler = (event) => {
      const nextSelected = Boolean(event?.target?.checked);
      const { pageContext } = getRenderedAccountsContext();
      const changed = setPageSelection(pageContext.items, nextSelected);
      syncAccountSelectionControls(pageContext.items);
      if (changed) {
        rerenderAccountsPage();
      }
    };
  }
  if (accountSelectAllBoundEl && accountSelectAllBoundEl !== dom.accountSelectAll) {
    accountSelectAllBoundEl.removeEventListener("change", accountSelectAllChangeHandler);
  }
  if (accountSelectAllBoundEl === dom.accountSelectAll) {
    return;
  }
  dom.accountSelectAll.addEventListener("change", accountSelectAllChangeHandler);
  accountSelectAllBoundEl = dom.accountSelectAll;
}

function ensureAccountPaginationEventsBound() {
  if (!dom.accountPageSize || !dom.accountPagePrev || !dom.accountPageNext) {
    return;
  }
  const nextRefs = {
    pageSize: dom.accountPageSize,
    prev: dom.accountPagePrev,
    next: dom.accountPageNext,
  };
  if (
    accountPaginationBoundRefs
    && accountPaginationBoundRefs.pageSize === nextRefs.pageSize
    && accountPaginationBoundRefs.prev === nextRefs.prev
    && accountPaginationBoundRefs.next === nextRefs.next
  ) {
    return;
  }
  if (!accountPageSizeChangeHandler) {
    accountPageSizeChangeHandler = (event) => {
      const nextPageSize = normalizeAccountPageSize(event.target?.value);
      if (nextPageSize === state.accountPageSize && state.accountPage === 1) {
        return;
      }
      state.accountPageSize = nextPageSize;
      state.accountPage = 1;
      requestAccountsPageReload();
    };
  }
  if (!accountPagePrevClickHandler) {
    accountPagePrevClickHandler = () => {
      if (state.accountPage <= 1) {
        return;
      }
      state.accountPage -= 1;
      requestAccountsPageReload();
    };
  }
  if (!accountPageNextClickHandler) {
    accountPageNextClickHandler = () => {
      state.accountPage += 1;
      requestAccountsPageReload();
    };
  }
  if (accountPaginationBoundRefs) {
    accountPaginationBoundRefs.pageSize?.removeEventListener("change", accountPageSizeChangeHandler);
    accountPaginationBoundRefs.prev?.removeEventListener("click", accountPagePrevClickHandler);
    accountPaginationBoundRefs.next?.removeEventListener("click", accountPageNextClickHandler);
  }
  nextRefs.pageSize.addEventListener("change", accountPageSizeChangeHandler);
  nextRefs.prev.addEventListener("click", accountPagePrevClickHandler);
  nextRefs.next.addEventListener("click", accountPageNextClickHandler);
  accountPaginationBoundRefs = nextRefs;
}

function renderAccountPagination(pageContext) {
  ensureAccountPaginationEventsBound();
  if (
    !dom.accountPagination
    || !dom.accountPaginationSummary
    || !dom.accountPageSize
    || !dom.accountPagePrev
    || !dom.accountPageInfo
    || !dom.accountPageNext
  ) {
    return;
  }
  const {
    total,
    pageSize,
    totalPages,
    page,
    startIndex,
    endIndex,
  } = pageContext;
  dom.accountPagination.hidden = false;
  dom.accountPageSize.value = String(pageSize);
  if (total <= 0) {
    dom.accountPaginationSummary.textContent = "共 0 个账号";
  } else {
    dom.accountPaginationSummary.textContent = `共 ${total} 个账号，当前显示 ${startIndex + 1}-${endIndex}`;
  }
  dom.accountPageInfo.textContent = `第 ${page} / ${totalPages} 页`;
  dom.accountPagePrev.disabled = total <= 0 || page <= 1;
  dom.accountPageNext.disabled = total <= 0 || page >= totalPages;
}

function removeAllAccountRows() {
  if (!dom.accountRows) return;
  while (dom.accountRows.firstElementChild) {
    dom.accountRows.firstElementChild.remove();
  }
  accountRowNodesById = new Map();
}

function updateStatusTag(node, status) {
  if (!node) return;
  const next = status || { text: "未知", level: "unknown" };
  node.textContent = next.text;
  node.className = "status-tag";
  if (next.level === "ok") node.classList.add("status-ok");
  if (next.level === "warn") node.classList.add("status-warn");
  if (next.level === "bad") node.classList.add("status-bad");
  if (next.level === "unknown") node.classList.add("status-unknown");
}

function updateMiniUsage(mini, usage, primaryRemain, secondaryRemain) {
  if (!mini) return;
  const safeUsage = usage || null;
  const hasPrimaryWindow = safeUsage?.usedPercent != null && safeUsage?.windowMinutes != null;
  const hasSecondaryWindow =
    safeUsage?.secondaryUsedPercent != null
    || safeUsage?.secondaryWindowMinutes != null;

  mini.textContent = "";
  if (hasPrimaryWindow) {
    const primaryLabel = formatLimitLabel(safeUsage?.windowMinutes, "5小时");
    mini.appendChild(renderMiniUsageLine(primaryLabel, primaryRemain ?? null, false));
  }
  if (hasSecondaryWindow) {
    mini.appendChild(renderMiniUsageLine("7天", secondaryRemain ?? null, true));
  }
}

function ensureDeleteButton(actionsWrap) {
  if (!actionsWrap) return null;
  const existing = actionsWrap.querySelector(`button[data-action="${ACCOUNT_ACTION_DELETE}"]`);
  if (existing) return existing;
  const del = document.createElement("button");
  del.className = "danger";
  del.type = "button";
  del.setAttribute("data-action", ACCOUNT_ACTION_DELETE);
  del.textContent = "删除";
  actionsWrap.appendChild(del);
  return del;
}

function syncDeleteButton(actionsWrap, enabled) {
  if (!actionsWrap) return;
  const existing = actionsWrap.querySelector(`button[data-action="${ACCOUNT_ACTION_DELETE}"]`);
  if (enabled) {
    ensureDeleteButton(actionsWrap);
    return;
  }
  existing?.remove();
}

function updateAccountRow(row, account, accountDerivedMap, { onDelete }) {
  if (!row || !account || !account.id) {
    return row;
  }
  row.setAttribute("data-account-id", account.id);
  const accountDerived = accountDerivedMap.get(account.id) || {
    usage: null,
    primaryRemain: null,
    secondaryRemain: null,
    status: { text: "未知", level: "unknown" },
  };

  const selectInput = row.querySelector?.(`input[data-field='${ACCOUNT_FIELD_SELECT}']`);
  if (selectInput) {
    selectInput.checked = isAccountSelected(account.id);
  }

  const cellAccount = row.querySelector?.(".account-col-account");
  const title = cellAccount?.querySelector?.("strong");
  const meta = cellAccount?.querySelector?.("small");
  if (title) title.textContent = account.label || "-";
  if (meta) meta.textContent = `${account.id || "-"}`;
  const mini = cellAccount?.querySelector?.(".mini-usage");
  updateMiniUsage(mini, accountDerived.usage, accountDerived.primaryRemain, accountDerived.secondaryRemain);

  const cellGroup = row.querySelector?.(".account-col-group");
  if (cellGroup) cellGroup.textContent = normalizeGroupName(account.groupName) || "-";

  const sortCell = row.querySelector?.(".account-col-sort");
  const sortInput = sortCell?.querySelector?.("input[data-field='sort']");
  if (sortInput) {
    const next = account.sort != null ? String(account.sort) : "0";
    if (document.activeElement !== sortInput) {
      sortInput.value = next;
      sortInput.dataset.originSort = next;
    }
  }

  const statusCell = row.querySelector?.(".account-col-status");
  const statusTag = statusCell?.querySelector?.(".status-tag");
  updateStatusTag(statusTag, accountDerived.status);

  const updatedCell = row.querySelector?.(".account-col-updated");
  const updatedStrong = updatedCell?.querySelector?.("strong");
  if (updatedStrong) {
    updatedStrong.textContent = accountDerived.usage && accountDerived.usage.capturedAt
      ? formatTs(accountDerived.usage.capturedAt)
      : "未知";
  }

  const actionsCell = row.querySelector?.(".account-col-actions");
  const actionsWrap = actionsCell?.querySelector?.(".cell-actions");
  syncDeleteButton(actionsWrap, Boolean(onDelete));
  syncSetCurrentButton(actionsWrap, accountDerived.status);
  return row;
}

function syncAccountRows(filtered, accountDerivedMap, { onDelete }) {
  if (!dom.accountRows) return;
  const nextIds = new Set(filtered.map((account) => account.id));

  // Remove stale cache entries (and DOM nodes if still present)
  for (const [accountId, cachedRow] of accountRowNodesById.entries()) {
    if (!nextIds.has(accountId)) {
      cachedRow?.remove?.();
      accountRowNodesById.delete(accountId);
    }
  }

  let cursor = dom.accountRows.firstElementChild;
  for (const account of filtered) {
    if (!account || !account.id) continue;
    const accountId = account.id;
    let row = accountRowNodesById.get(accountId);
    if (!row || !row.isConnected) {
      row = renderAccountRow(account, accountDerivedMap, { onDelete });
      accountRowNodesById.set(accountId, row);
    } else {
      updateAccountRow(row, account, accountDerivedMap, { onDelete });
    }

    if (row === cursor) {
      cursor = cursor?.nextElementSibling || null;
      continue;
    }
    dom.accountRows.insertBefore(row, cursor);
  }

  // Remove any leftover nodes (including previous empty row) after the cursor.
  while (cursor) {
    const next = cursor.nextElementSibling;
    const accountId = cursor?.dataset?.accountId || "";
    if (!accountId || !nextIds.has(accountId)) {
      cursor.remove();
    }
    cursor = next;
  }

  syncAccountSelectionControls(filtered);
}

function getAccountFromRow(row, lookup) {
  const accountId = row?.dataset?.accountId;
  if (!accountId) return null;
  return lookup.get(accountId) || null;
}

export function handleAccountRowsClick(target, handlers = accountRowHandlers, lookup = accountLookupById) {
  const actionButton = target?.closest?.("button[data-action]");
  if (!actionButton) return false;
  const row = actionButton.closest("tr[data-account-id]");
  if (!row) return false;
  const account = getAccountFromRow(row, lookup);
  if (!account) return false;
  const action = actionButton.dataset.action;
  if (action === ACCOUNT_ACTION_OPEN_USAGE) {
    handlers?.onOpenUsage?.(account);
    return true;
  }
  if (action === ACCOUNT_ACTION_SET_CURRENT) {
    handlers?.onSetCurrentAccount?.(account);
    return true;
  }
  if (action === ACCOUNT_ACTION_DELETE) {
    handlers?.onDelete?.(account);
    return true;
  }
  return false;
}

export function handleAccountRowsChange(target, handlers = accountRowHandlers) {
  const selectInput = target?.closest?.(`input[data-field='${ACCOUNT_FIELD_SELECT}']`);
  if (selectInput) {
    const row = selectInput.closest("tr[data-account-id]");
    if (!row) return false;
    const accountId = row.dataset.accountId;
    if (!accountId) return false;
    const changed = setAccountSelected(accountId, Boolean(selectInput.checked));
    const { pageContext } = getRenderedAccountsContext();
    syncAccountSelectionControls(pageContext.items);
    return changed;
  }
  const sortInput = target?.closest?.("input[data-field='sort']");
  if (!sortInput) return false;
  const row = sortInput.closest("tr[data-account-id]");
  if (!row) return false;
  const accountId = row.dataset.accountId;
  if (!accountId) return false;
  const sortValue = Number(sortInput.value || 0);
  const originSort = Number(sortInput.dataset.originSort);
  if (Number.isFinite(originSort) && originSort === sortValue) {
    return false;
  }
  sortInput.dataset.originSort = String(sortValue);
  handlers?.onUpdateSort?.(accountId, sortValue, originSort);
  return true;
}

function ensureAccountRowsEventsBound() {
  if (!dom.accountRows) {
    return;
  }
  if (!accountRowsClickHandler) {
    accountRowsClickHandler = (event) => {
      handleAccountRowsClick(event.target);
    };
  }
  if (!accountRowsChangeHandler) {
    accountRowsChangeHandler = (event) => {
      handleAccountRowsChange(event.target);
    };
  }
  if (accountRowsEventsBoundEl && accountRowsEventsBoundEl !== dom.accountRows) {
    accountRowsEventsBoundEl.removeEventListener("click", accountRowsClickHandler);
    accountRowsEventsBoundEl.removeEventListener("change", accountRowsChangeHandler);
  }
  if (accountRowsEventsBoundEl === dom.accountRows) {
    return;
  }
  dom.accountRows.addEventListener("click", accountRowsClickHandler);
  dom.accountRows.addEventListener("change", accountRowsChangeHandler);
  accountRowsEventsBoundEl = dom.accountRows;
}

function getRemoteAccountPageContext(items, total) {
  const safeItems = Array.isArray(items) ? items : [];
  const normalizedTotal = Math.max(0, Number(total || 0));
  const pageSize = normalizeAccountPageSize(state.accountPageSize);
  const totalPages = Math.max(1, Math.ceil(normalizedTotal / pageSize));
  const page = clampAccountPage(state.accountPage, totalPages);
  state.accountPage = page;
  state.accountPageSize = pageSize;
  const startIndex = normalizedTotal > 0 ? (page - 1) * pageSize : 0;
  const endIndex = normalizedTotal > 0 ? startIndex + safeItems.length : 0;
  return {
    total: normalizedTotal,
    pageSize,
    totalPages,
    page,
    startIndex,
    endIndex,
    items: safeItems,
  };
}

// 渲染账号列表
export function renderAccounts({
  onUpdateSort,
  onOpenUsage,
  onSetCurrentAccount,
  onDelete,
  onRefreshPage,
}) {
  ensureAccountRowsEventsBound();
  ensureAccountSelectAllEventsBound();
  renderAccountsRefreshProgress();
  accountRowHandlers = { onUpdateSort, onOpenUsage, onSetCurrentAccount, onDelete, onRefreshPage };
  syncGroupFilterSelect(getGroupOptions(state.accountList), state.accountList);
  if (state.accountList.length > 0) {
    pruneSelectedAccountIds(state.accountList);
  }
  const { pageContext, accountDerivedMap } = getRenderedAccountsContext();
  renderAccountPagination(pageContext);

  if (pageContext.total === 0) {
    accountLookupById = new Map();
    syncAccountSelectionControls([]);
    const hasAccounts = state.accountList.length > 0;
    const message = hasAccounts ? "当前筛选条件下无结果" : "暂无账号";
    removeAllAccountRows();
    renderEmptyRow(message);
    return;
  }

  accountLookupById = new Map(pageContext.items.map((account) => [account.id, account]));
  syncAccountRows(pageContext.items, accountDerivedMap, { onDelete });
}
