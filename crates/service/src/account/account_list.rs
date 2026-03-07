use std::collections::HashMap;

use codexmanager_core::{
    rpc::types::{AccountListParams, AccountListResult, AccountSummary},
    storage::{Account, UsageSnapshotRecord},
};

use crate::{
    account_availability::{evaluate_snapshot, Availability},
    storage_helpers::open_storage,
};

const DEFAULT_ACCOUNT_PAGE_SIZE: i64 = 5;
const MAX_ACCOUNT_PAGE_SIZE: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccountFilter {
    All,
    Active,
    Low,
}

pub(crate) fn read_accounts(
    params: AccountListParams,
    pagination_requested: bool,
) -> Result<AccountListResult, String> {
    // 中文注释：账号页需要后端分页，但仪表盘/日志等全局功能仍依赖全量账号列表；
    // 因此这里兼容“无分页参数时返回全量，有分页参数时返回当前页”两种模式。
    let params = params.normalized();
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let query = normalize_optional_text(params.query);
    let group_filter = normalize_optional_text(params.group_filter);
    let filter = normalize_filter(params.filter);

    if filter == AccountFilter::All {
        if pagination_requested {
            let page_size = normalize_page_size(params.page_size);
            let total = storage
                .account_count_filtered(query.as_deref(), group_filter.as_deref())
                .map_err(|err| format!("count accounts failed: {err}"))?;
            let page = clamp_page(params.page, total, page_size);
            let offset = (page - 1) * page_size;
            let accounts = storage
                .list_accounts_paginated(
                    query.as_deref(),
                    group_filter.as_deref(),
                    offset,
                    page_size,
                )
                .map_err(|err| format!("list accounts failed: {err}"))?;
            return Ok(AccountListResult {
                items: accounts.into_iter().map(to_account_summary).collect(),
                total,
                page,
                page_size,
            });
        }

        let accounts = storage
            .list_accounts_filtered(query.as_deref(), group_filter.as_deref())
            .map_err(|err| format!("list accounts failed: {err}"))?;
        let total = accounts.len() as i64;
        return Ok(AccountListResult {
            items: accounts.into_iter().map(to_account_summary).collect(),
            total,
            page: 1,
            page_size: if total > 0 {
                total
            } else {
                DEFAULT_ACCOUNT_PAGE_SIZE
            },
        });
    }

    let usage_by_account: HashMap<String, UsageSnapshotRecord> = storage
        .latest_usage_snapshots_by_account()
        .map_err(|err| format!("list usage snapshots failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect();
    let mut accounts = storage
        .list_accounts_filtered(query.as_deref(), group_filter.as_deref())
        .map_err(|err| format!("list accounts failed: {err}"))?;
    accounts.retain(|account| matches_filter(filter, account, usage_by_account.get(&account.id)));

    let total = accounts.len() as i64;
    if pagination_requested {
        let page_size = normalize_page_size(params.page_size);
        let page = clamp_page(params.page, total, page_size);
        let offset = ((page - 1) * page_size) as usize;
        let paged = accounts
            .into_iter()
            .skip(offset)
            .take(page_size as usize)
            .collect::<Vec<_>>();
        return Ok(AccountListResult {
            items: paged.into_iter().map(to_account_summary).collect(),
            total,
            page,
            page_size,
        });
    }

    Ok(AccountListResult {
        items: accounts.into_iter().map(to_account_summary).collect(),
        total,
        page: 1,
        page_size: if total > 0 {
            total
        } else {
            DEFAULT_ACCOUNT_PAGE_SIZE
        },
    })
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    let trimmed = value.unwrap_or_default().trim().to_string();
    if trimmed.is_empty() || trimmed == "all" {
        return None;
    }
    Some(trimmed)
}

fn normalize_filter(value: Option<String>) -> AccountFilter {
    match value
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "active" => AccountFilter::Active,
        "low" => AccountFilter::Low,
        _ => AccountFilter::All,
    }
}

fn normalize_page_size(value: i64) -> i64 {
    value.clamp(1, MAX_ACCOUNT_PAGE_SIZE)
}

fn clamp_page(page: i64, total: i64, page_size: i64) -> i64 {
    let normalized_page = page.max(1);
    let total_pages = if total <= 0 {
        1
    } else {
        ((total + page_size - 1) / page_size).max(1)
    };
    normalized_page.min(total_pages)
}

fn matches_filter(
    filter: AccountFilter,
    account: &Account,
    snapshot: Option<&UsageSnapshotRecord>,
) -> bool {
    match filter {
        AccountFilter::All => true,
        AccountFilter::Active => {
            if account.status.trim().eq_ignore_ascii_case("inactive") {
                return false;
            }
            snapshot
                .map(|item| matches!(evaluate_snapshot(item), Availability::Available))
                .unwrap_or(false)
        }
        AccountFilter::Low => snapshot.map(is_low_quota).unwrap_or(false),
    }
}

fn is_low_quota(snapshot: &UsageSnapshotRecord) -> bool {
    snapshot
        .used_percent
        .map(|value| value >= 80.0)
        .unwrap_or(false)
        || snapshot
            .secondary_used_percent
            .map(|value| value >= 80.0)
            .unwrap_or(false)
}

fn to_account_summary(acc: Account) -> AccountSummary {
    AccountSummary {
        id: acc.id,
        label: acc.label,
        group_name: acc.group_name,
        sort: acc.sort,
        status: acc.status,
    }
}
