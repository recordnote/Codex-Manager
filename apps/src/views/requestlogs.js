import { dom } from "../ui/dom.js";
import { state } from "../state.js";
import { copyText } from "../utils/clipboard.js";
import {
  createRequestLogRow,
  createTopSpacerRow,
  renderEmptyRequestLogs,
} from "./requestlogs/row-render.js";
import {
  buildRequestRouteMeta,
  collectFilteredRequestLogs,
  ensureAccountLabelMap,
  fallbackAccountDisplayFromKey,
  isAppendOnlyResult,
  resolveAccountDisplayName,
  resolveDisplayRequestPath,
} from "./requestlogs/selectors.js";

const REQUEST_LOG_BATCH_SIZE = 80;
const REQUEST_LOG_DOM_LIMIT = 240;
const REQUEST_LOG_DOM_RECYCLE_TO = 180;
const REQUEST_LOG_SCROLL_BUFFER = 180;
const REQUEST_LOG_FALLBACK_ROW_HEIGHT = 54;
const REQUEST_LOG_COLUMN_COUNT = 9;
const REQUEST_LOG_NEAR_BOTTOM_MAX_BATCHES = 1;

const requestLogWindowState = {
  filter: "all",
  filtered: [],
  filteredKeys: [],
  nextIndex: 0,
  topSpacerHeight: 0,
  recycledRowHeight: REQUEST_LOG_FALLBACK_ROW_HEIGHT,
  accountListRef: null,
  accountLabelById: new Map(),
  topSpacerRow: null,
  topSpacerCell: null,
  boundRowsEl: null,
  boundScrollerEl: null,
  scrollTickHandle: null,
  scrollTickMode: "",
  hasRendered: false,
};

function getRowHeight(row) {
  if (!row) return REQUEST_LOG_FALLBACK_ROW_HEIGHT;
  if (typeof row.getBoundingClientRect === "function") {
    const rectHeight = Number(row.getBoundingClientRect().height);
    if (Number.isFinite(rectHeight) && rectHeight > 0) {
      return rectHeight;
    }
  }
  const offsetHeight = Number(row.offsetHeight);
  if (Number.isFinite(offsetHeight) && offsetHeight > 0) {
    return offsetHeight;
  }
  return REQUEST_LOG_FALLBACK_ROW_HEIGHT;
}

function updateTopSpacer() {
  const spacerRow = requestLogWindowState.topSpacerRow;
  const spacerCell = requestLogWindowState.topSpacerCell;
  if (!spacerRow || !spacerCell) return;
  const height = Math.max(0, Math.round(requestLogWindowState.topSpacerHeight));
  spacerRow.hidden = height <= 0;
  spacerCell.style.height = `${height}px`;
}

function appendRequestLogBatch() {
  if (!dom.requestLogRows) return false;
  const start = requestLogWindowState.nextIndex;
  if (start >= requestLogWindowState.filtered.length) return false;
  const end = Math.min(
    start + REQUEST_LOG_BATCH_SIZE,
    requestLogWindowState.filtered.length,
  );
  const fragment = document.createDocumentFragment();
  const accountLabelById = requestLogWindowState.accountLabelById;
  const rowRenderHelpers = {
    resolveAccountDisplayName: (item) =>
      resolveAccountDisplayName(item, accountLabelById),
    fallbackAccountDisplayFromKey,
    resolveDisplayRequestPath,
    buildRequestRouteMeta,
  };
  for (let i = start; i < end; i += 1) {
    fragment.appendChild(
      createRequestLogRow(requestLogWindowState.filtered[i], i, rowRenderHelpers),
    );
  }
  dom.requestLogRows.appendChild(fragment);
  requestLogWindowState.nextIndex = end;
  recycleLogRowsIfNeeded();
  return true;
}

function appendNearBottomBatches(scroller, maxBatches = REQUEST_LOG_NEAR_BOTTOM_MAX_BATCHES) {
  let appended = false;
  let rounds = 0;
  while (
    rounds < maxBatches &&
    isNearBottom(scroller) &&
    appendRequestLogBatch()
  ) {
    appended = true;
    rounds += 1;
  }
  return appended;
}

function appendAtLeastOneBatch(scroller, extraMaxBatches = REQUEST_LOG_NEAR_BOTTOM_MAX_BATCHES - 1) {
  const appended = appendRequestLogBatch();
  if (!appended) return false;
  if (extraMaxBatches > 0) {
    appendNearBottomBatches(scroller, extraMaxBatches);
  }
  return true;
}

function recycleLogRowsIfNeeded() {
  if (!dom.requestLogRows) return;
  const rows = [];
  for (const child of dom.requestLogRows.children) {
    if (child?.dataset?.logRow === "1") {
      rows.push(child);
    }
  }
  if (rows.length <= REQUEST_LOG_DOM_LIMIT) {
    return;
  }
  const removeCount = rows.length - REQUEST_LOG_DOM_RECYCLE_TO;
  // 中文注释：避免对每一行调用 getBoundingClientRect/offsetHeight（强制同步布局，滚动时很容易卡顿）。
  // 这里抽样一行高度来估算回收高度即可；配合 error/path 的摘要展示，行高波动很小。
  const sampleHeight = getRowHeight(rows[0]);
  if (Number.isFinite(sampleHeight) && sampleHeight > 0) {
    requestLogWindowState.recycledRowHeight = sampleHeight;
  }
  const removedHeight = requestLogWindowState.recycledRowHeight * removeCount;
  for (let i = 0; i < removeCount; i += 1) {
    rows[i].remove();
  }
  requestLogWindowState.topSpacerHeight += removedHeight;
  updateTopSpacer();
}

function isNearBottom(scroller) {
  if (!scroller) return false;
  const scrollTop = Number(scroller.scrollTop);
  const clientHeight = Number(scroller.clientHeight);
  const scrollHeight = Number(scroller.scrollHeight);
  if (!Number.isFinite(scrollTop) || !Number.isFinite(clientHeight) || !Number.isFinite(scrollHeight)) {
    return false;
  }
  return scrollTop + clientHeight >= scrollHeight - REQUEST_LOG_SCROLL_BUFFER;
}

function resolveRequestLogScroller(rowsEl) {
  if (!rowsEl || typeof rowsEl.closest !== "function") {
    return null;
  }
  return rowsEl.closest(".requestlog-wrap");
}

async function onRequestLogRowsClick(event) {
  const target = event?.target;
  if (!target || typeof target.closest !== "function") {
    return;
  }
  const copyBtn = target.closest("button.path-copy");
  if (!copyBtn || !dom.requestLogRows || !dom.requestLogRows.contains(copyBtn)) {
    return;
  }
  const index = Number(copyBtn.dataset.logIndex);
  if (!Number.isInteger(index)) {
    return;
  }
  const rowItem = requestLogWindowState.filtered[index];
  const textToCopy = resolveDisplayRequestPath(rowItem) || rowItem?.requestPath || "";
  if (!textToCopy) {
    return;
  }
  const ok = await copyText(textToCopy);
  copyBtn.textContent = ok ? "已复制" : "失败";
  const token = String(Date.now());
  copyBtn.dataset.copyToken = token;
  setTimeout(() => {
    if (copyBtn.dataset.copyToken !== token) return;
    copyBtn.textContent = "复制";
  }, 900);
}

function onRequestLogScroll() {
  if (requestLogWindowState.scrollTickHandle != null) {
    return;
  }
  const flush = () => {
    requestLogWindowState.scrollTickHandle = null;
    requestLogWindowState.scrollTickMode = "";
    if (!isNearBottom(requestLogWindowState.boundScrollerEl)) {
      return;
    }
    appendNearBottomBatches(requestLogWindowState.boundScrollerEl);
  };
  if (typeof window !== "undefined" && typeof window.requestAnimationFrame === "function") {
    requestLogWindowState.scrollTickMode = "raf";
    requestLogWindowState.scrollTickHandle = window.requestAnimationFrame(flush);
    return;
  }
  flush();
}

function cancelPendingScrollTick() {
  if (requestLogWindowState.scrollTickHandle == null) {
    return;
  }
  if (
    requestLogWindowState.scrollTickMode === "raf"
    && typeof window !== "undefined"
    && typeof window.cancelAnimationFrame === "function"
  ) {
    window.cancelAnimationFrame(requestLogWindowState.scrollTickHandle);
  } else {
    clearTimeout(requestLogWindowState.scrollTickHandle);
  }
  requestLogWindowState.scrollTickHandle = null;
  requestLogWindowState.scrollTickMode = "";
}

function ensureRequestLogBindings() {
  const rowsEl = dom.requestLogRows;
  if (!rowsEl || typeof rowsEl.addEventListener !== "function") {
    return;
  }
  if (requestLogWindowState.boundRowsEl && requestLogWindowState.boundRowsEl !== rowsEl) {
    requestLogWindowState.boundRowsEl.removeEventListener("click", onRequestLogRowsClick);
  }
  if (requestLogWindowState.boundRowsEl !== rowsEl) {
    rowsEl.addEventListener("click", onRequestLogRowsClick);
    requestLogWindowState.boundRowsEl = rowsEl;
  }
  const scroller = resolveRequestLogScroller(rowsEl);
  if (
    requestLogWindowState.boundScrollerEl &&
    requestLogWindowState.boundScrollerEl !== scroller
  ) {
    requestLogWindowState.boundScrollerEl.removeEventListener("scroll", onRequestLogScroll);
    cancelPendingScrollTick();
  }
  if (scroller && requestLogWindowState.boundScrollerEl !== scroller) {
    scroller.addEventListener("scroll", onRequestLogScroll, { passive: true });
    requestLogWindowState.boundScrollerEl = scroller;
  } else if (!scroller) {
    cancelPendingScrollTick();
    requestLogWindowState.boundScrollerEl = null;
  }
}

export function renderRequestLogs() {
  if (!dom.requestLogRows) {
    return;
  }
  ensureRequestLogBindings();
  ensureAccountLabelMap(state.accountList, requestLogWindowState);
  const filter = state.requestLogStatusFilter || "all";
  const { filtered, filteredKeys } = collectFilteredRequestLogs(
    state.requestLogList,
    filter,
  );
  const sameFilter = filter === requestLogWindowState.filter;
  const appendOnly = sameFilter && isAppendOnlyResult(
    requestLogWindowState.filteredKeys,
    filteredKeys,
  );
  const unchanged = appendOnly && filteredKeys.length === requestLogWindowState.filteredKeys.length;
  const canReuseRenderedDom = filtered.length > 0
    ? Boolean(
      requestLogWindowState.topSpacerRow &&
      dom.requestLogRows.contains(requestLogWindowState.topSpacerRow),
    )
    : dom.requestLogRows.children.length > 0;

  if (requestLogWindowState.hasRendered && canReuseRenderedDom && unchanged) {
    requestLogWindowState.filtered = filtered;
    requestLogWindowState.filteredKeys = filteredKeys;
    return;
  }

  if (
    requestLogWindowState.hasRendered &&
    appendOnly &&
    requestLogWindowState.topSpacerRow &&
    dom.requestLogRows.contains(requestLogWindowState.topSpacerRow)
  ) {
    const previousLength = requestLogWindowState.filtered.length;
    requestLogWindowState.filtered = filtered;
    requestLogWindowState.filteredKeys = filteredKeys;
    requestLogWindowState.filter = filter;
    if (
      requestLogWindowState.nextIndex >= previousLength ||
      isNearBottom(requestLogWindowState.boundScrollerEl)
    ) {
      appendAtLeastOneBatch(requestLogWindowState.boundScrollerEl);
    }
    return;
  }

  dom.requestLogRows.innerHTML = "";
  requestLogWindowState.filtered = filtered;
  requestLogWindowState.filteredKeys = filteredKeys;
  requestLogWindowState.filter = filter;
  requestLogWindowState.nextIndex = 0;
  requestLogWindowState.topSpacerHeight = 0;
  requestLogWindowState.recycledRowHeight = REQUEST_LOG_FALLBACK_ROW_HEIGHT;
  requestLogWindowState.topSpacerRow = null;
  requestLogWindowState.topSpacerCell = null;
  requestLogWindowState.hasRendered = true;
  if (!filtered.length) {
    renderEmptyRequestLogs(dom.requestLogRows, REQUEST_LOG_COLUMN_COUNT);
    return;
  }
  dom.requestLogRows.appendChild(
    createTopSpacerRow({
      columnCount: REQUEST_LOG_COLUMN_COUNT,
      windowState: requestLogWindowState,
    }),
  );
  appendAtLeastOneBatch(requestLogWindowState.boundScrollerEl, 1);
}
