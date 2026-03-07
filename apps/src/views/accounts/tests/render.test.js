import test from "node:test";
import assert from "node:assert/strict";

import { state } from "../../../state.js";
import { dom } from "../../../ui/dom.js";
import { handleAccountRowsChange, handleAccountRowsClick, renderAccounts } from "../render.js";

function createAccountActionTarget(action, accountId) {
  const row = {
    dataset: { accountId },
  };
  const button = {
    dataset: { action },
    closest(selector) {
      if (selector === "tr[data-account-id]") return row;
      return null;
    },
  };
  return {
    closest(selector) {
      if (selector === "button[data-action]") return button;
      return null;
    },
  };
}

function createSortChangeTarget(accountId, value) {
  const row = {
    dataset: { accountId },
  };
  const input = {
    value,
    dataset: { originSort: String(value) },
    closest(selector) {
      if (selector === "tr[data-account-id]") return row;
      return null;
    },
  };
  return {
    closest(selector) {
      if (selector === "input[data-field='sort']") return input;
      return null;
    },
  };
}

test("handleAccountRowsClick delegates open usage action by account id", () => {
  const account = { id: "acc-1", label: "main" };
  const lookup = new Map([[account.id, account]]);
  let opened = null;
  const handled = handleAccountRowsClick(
    createAccountActionTarget("open-usage", "acc-1"),
    {
      onOpenUsage: (item) => {
        opened = item;
      },
    },
    lookup,
  );
  assert.equal(handled, true);
  assert.deepEqual(opened, account);
});

test("handleAccountRowsClick delegates delete action by account id", () => {
  const account = { id: "acc-2", label: "backup" };
  const lookup = new Map([[account.id, account]]);
  let deleted = null;
  const handled = handleAccountRowsClick(
    createAccountActionTarget("delete", "acc-2"),
    {
      onDelete: (item) => {
        deleted = item;
      },
    },
    lookup,
  );
  assert.equal(handled, true);
  assert.deepEqual(deleted, account);
});

test("handleAccountRowsChange delegates sort change with numeric value", () => {
  let payload = null;
  const target = createSortChangeTarget("acc-3", "42");
  target.closest("input[data-field='sort']").dataset.originSort = "7";
  const handled = handleAccountRowsChange(target, {
    onUpdateSort: (accountId, sort) => {
      payload = { accountId, sort };
    },
  });
  assert.equal(handled, true);
  assert.deepEqual(payload, { accountId: "acc-3", sort: 42 });
});

test("handleAccountRowsChange skips unchanged sort value", () => {
  let called = false;
  const handled = handleAccountRowsChange(createSortChangeTarget("acc-4", "5"), {
    onUpdateSort: () => {
      called = true;
    },
  });
  assert.equal(handled, false);
  assert.equal(called, false);
});

class FakeClassList {
  constructor() {
    this.tokens = new Set();
  }

  setFromString(value) {
    this.tokens.clear();
    for (const token of String(value || "").split(/\s+/)) {
      if (token) this.tokens.add(token);
    }
  }

  add(...tokens) {
    for (const token of tokens) {
      if (token) this.tokens.add(token);
    }
  }

  contains(token) {
    return this.tokens.has(token);
  }

  toString() {
    return [...this.tokens].join(" ");
  }
}

function matchesSelector(node, selector) {
  if (!node || !selector) return false;
  if (selector.startsWith(".")) {
    return node.classList?.contains(selector.slice(1));
  }
  const attrMatch = selector.match(/^([a-zA-Z0-9_-]+)?\[([^=\]]+)(?:=['"]?([^'"\]]+)['"]?)?\]$/);
  if (attrMatch) {
    const [, rawTag, rawAttr, rawValue] = attrMatch;
    const tag = rawTag ? rawTag.toUpperCase() : "";
    const attr = rawAttr;
    if (tag && node.tagName !== tag) return false;
    const actual = attr.startsWith("data-")
      ? node.dataset?.[attr.slice(5).replace(/-([a-z])/g, (_, ch) => ch.toUpperCase())]
      : node.attributes?.[attr];
    if (rawValue == null) {
      return actual != null;
    }
    return String(actual) === rawValue;
  }
  return node.tagName === selector.toUpperCase();
}

class FakeElement {
  constructor(tagName = "div") {
    this.tagName = tagName.toUpperCase();
    this.children = [];
    this.parentElement = null;
    this.dataset = {};
    this.attributes = {};
    this.handlers = new Map();
    this.classList = new FakeClassList();
    this.style = {};
    this.hidden = false;
    this.disabled = false;
    this.checked = false;
    this.indeterminate = false;
    this.value = "";
    this.type = "";
    this.colSpan = 1;
    this._className = "";
    this._textContent = "";
  }

  get isConnected() {
    return Boolean(this.parentElement);
  }

  set className(value) {
    this._className = String(value || "");
    this.classList.setFromString(this._className);
  }

  get className() {
    return this.classList.toString();
  }

  set textContent(value) {
    this._textContent = String(value ?? "");
    for (const child of this.children) {
      child.parentElement = null;
    }
    this.children = [];
  }

  get textContent() {
    if (this.children.length === 0) {
      return this._textContent;
    }
    return this._textContent + this.children.map((child) => child.textContent).join("");
  }

  set innerHTML(value) {
    this._textContent = String(value || "");
    for (const child of this.children) {
      child.parentElement = null;
    }
    this.children = [];
  }

  get firstElementChild() {
    return this.children[0] || null;
  }

  get nextElementSibling() {
    if (!this.parentElement) return null;
    const siblings = this.parentElement.children;
    const index = siblings.indexOf(this);
    return index >= 0 ? siblings[index + 1] || null : null;
  }

  setAttribute(name, value) {
    this.attributes[name] = String(value);
    if (name === "class") {
      this.className = value;
      return;
    }
    if (name.startsWith("data-")) {
      const key = name.slice(5).replace(/-([a-z])/g, (_, ch) => ch.toUpperCase());
      this.dataset[key] = String(value);
    }
  }

  appendChild(node) {
    if (!node) return node;
    if (node.parentElement) {
      node.parentElement.removeChild(node);
    }
    node.parentElement = this;
    this.children.push(node);
    return node;
  }

  prepend(node) {
    if (!node) return node;
    if (node.parentElement) {
      node.parentElement.removeChild(node);
    }
    node.parentElement = this;
    this.children.unshift(node);
    return node;
  }

  insertBefore(node, referenceNode) {
    if (!referenceNode) {
      return this.appendChild(node);
    }
    if (node.parentElement) {
      node.parentElement.removeChild(node);
    }
    const index = this.children.indexOf(referenceNode);
    if (index < 0) {
      return this.appendChild(node);
    }
    node.parentElement = this;
    this.children.splice(index, 0, node);
    return node;
  }

  removeChild(node) {
    const index = this.children.indexOf(node);
    if (index >= 0) {
      this.children.splice(index, 1);
      node.parentElement = null;
    }
    return node;
  }

  remove() {
    this.parentElement?.removeChild(this);
  }

  addEventListener(type, handler) {
    if (!this.handlers.has(type)) {
      this.handlers.set(type, new Set());
    }
    this.handlers.get(type).add(handler);
  }

  removeEventListener(type, handler) {
    this.handlers.get(type)?.delete(handler);
  }

  dispatch(type, event = {}) {
    const list = this.handlers.get(type);
    if (!list) return;
    for (const handler of list) {
      handler({ ...event, target: event.target || this });
    }
  }

  contains(node) {
    if (node === this) return true;
    return this.children.some((child) => child.contains(node));
  }

  closest(selector) {
    let current = this;
    while (current) {
      if (matchesSelector(current, selector)) {
        return current;
      }
      current = current.parentElement;
    }
    return null;
  }

  querySelector(selector) {
    for (const child of this.children) {
      if (matchesSelector(child, selector)) {
        return child;
      }
      const nested = child.querySelector(selector);
      if (nested) return nested;
    }
    return null;
  }
}

class FakeDocument {
  createElement(tagName) {
    return new FakeElement(tagName);
  }
}

function createAccountsLayout() {
  const toolbar = new FakeElement("div");
  toolbar.className = "accounts-toolbar";
  const groupFilter = new FakeElement("select");
  const selectAll = new FakeElement("input");
  selectAll.type = "checkbox";
  const deleteSelectedAccountsBtn = new FakeElement("button");
  deleteSelectedAccountsBtn.type = "button";
  const panel = new FakeElement("div");
  panel.className = "panel";
  const tableWrap = new FakeElement("div");
  tableWrap.className = "table-wrap";
  const table = new FakeElement("table");
  const rows = new FakeElement("tbody");
  const pagination = new FakeElement("div");
  const summary = new FakeElement("div");
  const controls = new FakeElement("div");
  const sizeLabel = new FakeElement("label");
  const sizeText = new FakeElement("span");
  const pageSize = new FakeElement("select");
  const prev = new FakeElement("button");
  const info = new FakeElement("div");
  const next = new FakeElement("button");

  pagination.className = "account-pagination";
  summary.className = "account-pagination-summary";
  controls.className = "account-pagination-controls";
  sizeLabel.className = "account-pagination-size";
  pageSize.className = "account-pagination-select";
  prev.type = "button";
  next.type = "button";
  sizeText.textContent = "每页";
  pageSize.value = "20";

  panel.appendChild(tableWrap);
  tableWrap.appendChild(table);
  table.appendChild(rows);
  panel.appendChild(pagination);
  pagination.appendChild(summary);
  pagination.appendChild(controls);
  controls.appendChild(sizeLabel);
  sizeLabel.appendChild(sizeText);
  sizeLabel.appendChild(pageSize);
  controls.appendChild(prev);
  controls.appendChild(info);
  controls.appendChild(next);

  return {
    toolbar,
    groupFilter,
    selectAll,
    deleteSelectedAccountsBtn,
    rows,
    pagination,
    summary,
    pageSize,
    prev,
    info,
    next,
  };
}

function makeAccount(index) {
  return {
    id: `acc-${index}`,
    label: `账号${index}`,
    groupName: index % 2 === 0 ? "A组" : "",
    sort: index,
  };
}

function visibleDataRows(rows) {
  return rows.children.filter((child) => child.tagName === "TR" && child.dataset.accountId);
}

function getAccountCell(row) {
  return row.querySelector(".account-col-account");
}

test("renderAccounts paginates rows and page size changes reset to first page", () => {
  const previousDocument = globalThis.document;
  const previousDom = {
    accountsToolbar: dom.accountsToolbar,
    accountGroupFilter: dom.accountGroupFilter,
    accountSelectAll: dom.accountSelectAll,
    accountRows: dom.accountRows,
    accountPagination: dom.accountPagination,
    accountPaginationSummary: dom.accountPaginationSummary,
    accountPageSize: dom.accountPageSize,
    accountPagePrev: dom.accountPagePrev,
    accountPageInfo: dom.accountPageInfo,
    accountPageNext: dom.accountPageNext,
    deleteSelectedAccountsBtn: dom.deleteSelectedAccountsBtn,
  };
  const previousState = {
    accountList: state.accountList,
    usageList: state.usageList,
    accountSearch: state.accountSearch,
    accountFilter: state.accountFilter,
    accountGroupFilter: state.accountGroupFilter,
    accountPage: state.accountPage,
    accountPageSize: state.accountPageSize,
    accountPageItems: state.accountPageItems,
    accountPageTotal: state.accountPageTotal,
    accountPageLoaded: state.accountPageLoaded,
    selectedAccountIds: state.selectedAccountIds,
  };

  globalThis.document = new FakeDocument();
  const layout = createAccountsLayout();
  dom.accountsToolbar = layout.toolbar;
  dom.accountGroupFilter = layout.groupFilter;
  dom.accountSelectAll = layout.selectAll;
  dom.accountRows = layout.rows;
  dom.accountPagination = layout.pagination;
  dom.accountPaginationSummary = layout.summary;
  dom.accountPageSize = layout.pageSize;
  dom.accountPagePrev = layout.prev;
  dom.accountPageInfo = layout.info;
  dom.accountPageNext = layout.next;
  dom.deleteSelectedAccountsBtn = layout.deleteSelectedAccountsBtn;

  state.accountList = Array.from({ length: 25 }, (_, index) => makeAccount(index + 1));
  state.usageList = [];
  state.accountSearch = "";
  state.accountFilter = "all";
  state.accountGroupFilter = "all";
  state.accountPage = 1;
  state.accountPageSize = 20;
  state.accountPageItems = [];
  state.accountPageTotal = 0;
  state.accountPageLoaded = false;
  state.selectedAccountIds = new Set();

  try {
    renderAccounts({});
    assert.equal(visibleDataRows(layout.rows).length, 20);
    assert.match(getAccountCell(visibleDataRows(layout.rows)[0]).textContent, /账号1/);
    assert.equal(layout.summary.textContent, "共 25 个账号，当前显示 1-20");
    assert.equal(layout.info.textContent, "第 1 / 2 页");
    assert.equal(layout.prev.disabled, true);
    assert.equal(layout.next.disabled, false);
    assert.equal(layout.selectAll.checked, false);
    assert.equal(layout.deleteSelectedAccountsBtn.disabled, true);

    layout.next.dispatch("click", { target: layout.next });
    assert.equal(state.accountPage, 2);
    assert.equal(visibleDataRows(layout.rows).length, 5);
    assert.match(getAccountCell(visibleDataRows(layout.rows)[0]).textContent, /账号21/);
    assert.equal(layout.info.textContent, "第 2 / 2 页");

    layout.pageSize.value = "10";
    layout.pageSize.dispatch("change", { target: layout.pageSize });
    assert.equal(state.accountPage, 1);
    assert.equal(state.accountPageSize, 10);
    assert.equal(visibleDataRows(layout.rows).length, 10);
    assert.match(getAccountCell(visibleDataRows(layout.rows)[0]).textContent, /账号1/);
    assert.equal(layout.summary.textContent, "共 25 个账号，当前显示 1-10");
    assert.equal(layout.info.textContent, "第 1 / 3 页");
  } finally {
    globalThis.document = previousDocument;
    dom.accountsToolbar = previousDom.accountsToolbar;
    dom.accountGroupFilter = previousDom.accountGroupFilter;
    dom.accountSelectAll = previousDom.accountSelectAll;
    dom.accountRows = previousDom.accountRows;
    dom.accountPagination = previousDom.accountPagination;
    dom.accountPaginationSummary = previousDom.accountPaginationSummary;
    dom.accountPageSize = previousDom.accountPageSize;
    dom.accountPagePrev = previousDom.accountPagePrev;
    dom.accountPageInfo = previousDom.accountPageInfo;
    dom.accountPageNext = previousDom.accountPageNext;
    dom.deleteSelectedAccountsBtn = previousDom.deleteSelectedAccountsBtn;
    state.accountList = previousState.accountList;
    state.usageList = previousState.usageList;
    state.accountSearch = previousState.accountSearch;
    state.accountFilter = previousState.accountFilter;
    state.accountGroupFilter = previousState.accountGroupFilter;
    state.accountPage = previousState.accountPage;
    state.accountPageSize = previousState.accountPageSize;
    state.accountPageItems = previousState.accountPageItems;
    state.accountPageTotal = previousState.accountPageTotal;
    state.accountPageLoaded = previousState.accountPageLoaded;
    state.selectedAccountIds = previousState.selectedAccountIds;
  }
});

test("renderAccounts uses remote page items and pagination actions trigger refresh callback", () => {
  const previousDocument = globalThis.document;
  const previousDom = {
    accountsToolbar: dom.accountsToolbar,
    accountGroupFilter: dom.accountGroupFilter,
    accountSelectAll: dom.accountSelectAll,
    accountRows: dom.accountRows,
    accountPagination: dom.accountPagination,
    accountPaginationSummary: dom.accountPaginationSummary,
    accountPageSize: dom.accountPageSize,
    accountPagePrev: dom.accountPagePrev,
    accountPageInfo: dom.accountPageInfo,
    accountPageNext: dom.accountPageNext,
    deleteSelectedAccountsBtn: dom.deleteSelectedAccountsBtn,
  };
  const previousState = {
    accountList: state.accountList,
    usageList: state.usageList,
    accountSearch: state.accountSearch,
    accountFilter: state.accountFilter,
    accountGroupFilter: state.accountGroupFilter,
    accountPage: state.accountPage,
    accountPageSize: state.accountPageSize,
    accountPageItems: state.accountPageItems,
    accountPageTotal: state.accountPageTotal,
    accountPageLoaded: state.accountPageLoaded,
    selectedAccountIds: state.selectedAccountIds,
  };

  globalThis.document = new FakeDocument();
  const layout = createAccountsLayout();
  dom.accountsToolbar = layout.toolbar;
  dom.accountGroupFilter = layout.groupFilter;
  dom.accountSelectAll = layout.selectAll;
  dom.accountRows = layout.rows;
  dom.accountPagination = layout.pagination;
  dom.accountPaginationSummary = layout.summary;
  dom.accountPageSize = layout.pageSize;
  dom.accountPagePrev = layout.prev;
  dom.accountPageInfo = layout.info;
  dom.accountPageNext = layout.next;
  dom.deleteSelectedAccountsBtn = layout.deleteSelectedAccountsBtn;

  state.accountList = Array.from({ length: 25 }, (_, index) => makeAccount(index + 1));
  state.usageList = [];
  state.accountSearch = "";
  state.accountFilter = "all";
  state.accountGroupFilter = "all";
  state.accountPage = 2;
  state.accountPageSize = 5;
  state.accountPageItems = state.accountList.slice(5, 10);
  state.accountPageTotal = 25;
  state.accountPageLoaded = true;
  state.selectedAccountIds = new Set();

  let refreshCount = 0;

  try {
    renderAccounts({
      onRefreshPage: () => {
        refreshCount += 1;
      },
    });
    assert.equal(visibleDataRows(layout.rows).length, 5);
    assert.match(getAccountCell(visibleDataRows(layout.rows)[0]).textContent, /账号6/);
    assert.equal(layout.summary.textContent, "共 25 个账号，当前显示 6-10");
    assert.equal(layout.info.textContent, "第 2 / 5 页");

    layout.next.dispatch("click", { target: layout.next });
    assert.equal(state.accountPage, 3);
    assert.equal(refreshCount, 1);

    layout.pageSize.value = "10";
    layout.pageSize.dispatch("change", { target: layout.pageSize });
    assert.equal(state.accountPage, 1);
    assert.equal(state.accountPageSize, 10);
    assert.equal(refreshCount, 2);
  } finally {
    globalThis.document = previousDocument;
    dom.accountsToolbar = previousDom.accountsToolbar;
    dom.accountGroupFilter = previousDom.accountGroupFilter;
    dom.accountSelectAll = previousDom.accountSelectAll;
    dom.accountRows = previousDom.accountRows;
    dom.accountPagination = previousDom.accountPagination;
    dom.accountPaginationSummary = previousDom.accountPaginationSummary;
    dom.accountPageSize = previousDom.accountPageSize;
    dom.accountPagePrev = previousDom.accountPagePrev;
    dom.accountPageInfo = previousDom.accountPageInfo;
    dom.accountPageNext = previousDom.accountPageNext;
    dom.deleteSelectedAccountsBtn = previousDom.deleteSelectedAccountsBtn;
    state.accountList = previousState.accountList;
    state.usageList = previousState.usageList;
    state.accountSearch = previousState.accountSearch;
    state.accountFilter = previousState.accountFilter;
    state.accountGroupFilter = previousState.accountGroupFilter;
    state.accountPage = previousState.accountPage;
    state.accountPageSize = previousState.accountPageSize;
    state.accountPageItems = previousState.accountPageItems;
    state.accountPageTotal = previousState.accountPageTotal;
    state.accountPageLoaded = previousState.accountPageLoaded;
    state.selectedAccountIds = previousState.selectedAccountIds;
  }
});

test("renderAccounts syncs row checkbox, select-all and batch delete count", () => {
  const previousDocument = globalThis.document;
  const previousDom = {
    accountsToolbar: dom.accountsToolbar,
    accountGroupFilter: dom.accountGroupFilter,
    accountSelectAll: dom.accountSelectAll,
    accountRows: dom.accountRows,
    accountPagination: dom.accountPagination,
    accountPaginationSummary: dom.accountPaginationSummary,
    accountPageSize: dom.accountPageSize,
    accountPagePrev: dom.accountPagePrev,
    accountPageInfo: dom.accountPageInfo,
    accountPageNext: dom.accountPageNext,
    deleteSelectedAccountsBtn: dom.deleteSelectedAccountsBtn,
  };
  const previousState = {
    accountList: state.accountList,
    usageList: state.usageList,
    accountSearch: state.accountSearch,
    accountFilter: state.accountFilter,
    accountGroupFilter: state.accountGroupFilter,
    accountPage: state.accountPage,
    accountPageSize: state.accountPageSize,
    accountPageItems: state.accountPageItems,
    accountPageTotal: state.accountPageTotal,
    accountPageLoaded: state.accountPageLoaded,
    selectedAccountIds: state.selectedAccountIds,
  };

  globalThis.document = new FakeDocument();
  const layout = createAccountsLayout();
  dom.accountsToolbar = layout.toolbar;
  dom.accountGroupFilter = layout.groupFilter;
  dom.accountSelectAll = layout.selectAll;
  dom.accountRows = layout.rows;
  dom.accountPagination = layout.pagination;
  dom.accountPaginationSummary = layout.summary;
  dom.accountPageSize = layout.pageSize;
  dom.accountPagePrev = layout.prev;
  dom.accountPageInfo = layout.info;
  dom.accountPageNext = layout.next;
  dom.deleteSelectedAccountsBtn = layout.deleteSelectedAccountsBtn;

  state.accountList = Array.from({ length: 3 }, (_, index) => makeAccount(index + 1));
  state.usageList = [];
  state.accountSearch = "";
  state.accountFilter = "all";
  state.accountGroupFilter = "all";
  state.accountPage = 1;
  state.accountPageSize = 5;
  state.accountPageItems = [];
  state.accountPageTotal = 0;
  state.accountPageLoaded = false;
  state.selectedAccountIds = new Set();

  try {
    renderAccounts({});
    const rows = visibleDataRows(layout.rows);
    const firstCheckbox = rows[0].querySelector("input[data-field='selected']");
    const secondCheckbox = rows[1].querySelector("input[data-field='selected']");
    assert.equal(firstCheckbox.checked, false);
    assert.equal(layout.selectAll.checked, false);
    assert.equal(layout.selectAll.indeterminate, false);
    assert.equal(layout.deleteSelectedAccountsBtn.disabled, true);

    firstCheckbox.checked = true;
    handleAccountRowsChange(firstCheckbox, {});
    assert.deepEqual(Array.from(state.selectedAccountIds), ["acc-1"]);
    assert.equal(layout.selectAll.checked, false);
    assert.equal(layout.selectAll.indeterminate, true);
    assert.equal(layout.deleteSelectedAccountsBtn.disabled, false);
    assert.equal(layout.deleteSelectedAccountsBtn.textContent, "删除选中账号（1）");

    layout.selectAll.checked = true;
    layout.selectAll.dispatch("change", { target: layout.selectAll });
    assert.equal(state.selectedAccountIds.size, 3);
    assert.equal(layout.selectAll.checked, true);
    assert.equal(layout.selectAll.indeterminate, false);
    assert.equal(secondCheckbox.checked, true);
    assert.equal(layout.deleteSelectedAccountsBtn.textContent, "删除选中账号（3）");

    layout.selectAll.checked = false;
    layout.selectAll.dispatch("change", { target: layout.selectAll });
    assert.equal(state.selectedAccountIds.size, 0);
    assert.equal(layout.selectAll.checked, false);
    assert.equal(layout.selectAll.indeterminate, false);
    assert.equal(layout.deleteSelectedAccountsBtn.disabled, true);
  } finally {
    globalThis.document = previousDocument;
    dom.accountsToolbar = previousDom.accountsToolbar;
    dom.accountGroupFilter = previousDom.accountGroupFilter;
    dom.accountSelectAll = previousDom.accountSelectAll;
    dom.accountRows = previousDom.accountRows;
    dom.accountPagination = previousDom.accountPagination;
    dom.accountPaginationSummary = previousDom.accountPaginationSummary;
    dom.accountPageSize = previousDom.accountPageSize;
    dom.accountPagePrev = previousDom.accountPagePrev;
    dom.accountPageInfo = previousDom.accountPageInfo;
    dom.accountPageNext = previousDom.accountPageNext;
    dom.deleteSelectedAccountsBtn = previousDom.deleteSelectedAccountsBtn;
    state.accountList = previousState.accountList;
    state.usageList = previousState.usageList;
    state.accountSearch = previousState.accountSearch;
    state.accountFilter = previousState.accountFilter;
    state.accountGroupFilter = previousState.accountGroupFilter;
    state.accountPage = previousState.accountPage;
    state.accountPageSize = previousState.accountPageSize;
    state.accountPageItems = previousState.accountPageItems;
    state.accountPageTotal = previousState.accountPageTotal;
    state.accountPageLoaded = previousState.accountPageLoaded;
    state.selectedAccountIds = previousState.selectedAccountIds;
  }
});
