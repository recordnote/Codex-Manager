"use client";

import { useMemo, useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertCircle,
  BarChart3,
  KeyRound,
  LineChart,
  Pencil,
  Plus,
  RefreshCw,
  ShieldCheck,
  UserPlus,
  UsersRound,
  WalletCards,
} from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Empty, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { APP_SESSION_QUERY_KEY } from "@/hooks/useAppSession";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { appClient } from "@/lib/api/app-client";
import { dashboardClient } from "@/lib/api/dashboard-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";
import { formatCompactNumber } from "@/lib/utils/usage";
import type { AccountManagerStatus, AppUser, MemberDashboardSummary } from "@/types";

const ACCOUNT_MANAGER_QUERY_KEYS = {
  status: ["account-manager", "status"] as const,
  users: ["account-manager", "users"] as const,
};

const CREDIT_MICROS_PER_USD = 1_000_000;

function formatCreditMicros(value: number | null | undefined): string {
  const normalized =
    typeof value === "number" && Number.isFinite(value) ? value : 0;
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(normalized / CREDIT_MICROS_PER_USD);
}

function formatUsd(value: number | null | undefined): string {
  const normalized =
    typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : 0;
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(normalized);
}

function formatTokenAmount(value: number | null | undefined): string {
  const normalized =
    typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : 0;
  if (normalized < 1000) {
    return normalized.toLocaleString("zh-CN");
  }
  return formatCompactNumber(normalized, "0.00", 2, true);
}

function parseCreditInput(value: string): number | null {
  const normalized = Number(String(value || "").trim());
  if (!Number.isFinite(normalized) || normalized < 0) {
    return null;
  }
  return Math.round(normalized * CREDIT_MICROS_PER_USD);
}

function formatTime(value: number | null | undefined): string {
  if (!value) return "从未";
  const date = new Date(value * 1000);
  if (Number.isNaN(date.getTime())) return "未知";
  return date.toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatShortDate(value: number | null | undefined): string {
  if (!value) return "--";
  const date = new Date(value * 1000);
  if (Number.isNaN(date.getTime())) return "--";
  return date.toLocaleDateString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
  });
}

function modeLabel(mode: string): string {
  switch (mode) {
    case "accounts":
      return "账号登录";
    case "password":
      return "共享密码";
    case "none":
      return "未开启";
    default:
      return mode || "未知";
  }
}

function roleLabel(role: string): string {
  return role === "admin" ? "管理员" : "成员";
}

function isAdminUser(user: AppUser): boolean {
  return user.role === "admin";
}

function userCanOwnWallet(user: AppUser): boolean {
  return !isAdminUser(user);
}

function statusLabel(status: string): string {
  if (status === "disabled") return "禁用";
  return status === "active" ? "启用" : status || "未知";
}

function userSelectLabel(user: AppUser | null | undefined): string {
  if (!user) return "选择可分发成员";
  return user.displayName ? `${user.displayName} (${user.username})` : user.username;
}

function StatCard({
  title,
  value,
  detail,
  icon: Icon,
}: {
  title: string;
  value: string;
  detail: string;
  icon: typeof ShieldCheck;
}) {
  return (
    <Card className="glass-card shadow-sm">
      <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
        <div className="space-y-1">
          <CardDescription>{title}</CardDescription>
          <CardTitle className="text-2xl">{value}</CardTitle>
        </div>
        <div className="rounded-lg bg-primary/10 p-2 text-primary">
          <Icon className="h-4 w-4" />
        </div>
      </CardHeader>
      <CardContent>
        <p className="text-xs text-muted-foreground">{detail}</p>
      </CardContent>
    </Card>
  );
}

function UserUsageTrendLine({ summary }: { summary: MemberDashboardSummary }) {
  const points = summary.usageTrend7d;
  const maxTokens = Math.max(1, ...points.map((item) => item.totalTokens));
  const width = 320;
  const height = 112;
  const padding = 12;
  const plotWidth = width - padding * 2;
  const plotHeight = height - padding * 2;
  const coords = points.map((item, index) => {
    const x =
      points.length <= 1 ? width / 2 : padding + (index / (points.length - 1)) * plotWidth;
    const y = padding + plotHeight - (item.totalTokens / maxTokens) * plotHeight;
    return { x, y, item };
  });
  const path = coords
    .map((point, index) => `${index === 0 ? "M" : "L"} ${point.x} ${point.y}`)
    .join(" ");

  return (
    <div className="rounded-xl bg-background/35 p-3">
      <svg
        viewBox={`0 0 ${width} ${height}`}
        className="h-36 w-full text-primary"
        role="img"
        aria-label="user token usage line chart"
      >
        <path
          d={path}
          fill="none"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="3"
        />
        {coords.map((point) => (
          <circle
            key={point.item.dayStartTs}
            cx={point.x}
            cy={point.y}
            r="3.2"
            className="fill-background stroke-primary"
            strokeWidth="2"
          >
            <title>
              {formatShortDate(point.item.dayStartTs)} · {formatTokenAmount(point.item.totalTokens)}
            </title>
          </circle>
        ))}
      </svg>
      <div className="grid grid-cols-7 gap-1 text-center text-[10px] text-muted-foreground">
        {points.map((item) => (
          <span key={item.dayStartTs}>{formatShortDate(item.dayStartTs)}</span>
        ))}
      </div>
    </div>
  );
}

function UserUsageDetail({
  user,
  summary,
}: {
  user: AppUser;
  summary: MemberDashboardSummary;
}) {
  const { t } = useI18n();
  const successRate =
    summary.usageToday.successRate == null
      ? "--"
      : `${Math.round(summary.usageToday.successRate * 100)}%`;
  return (
    <div className="grid gap-4">
      <div className="grid gap-3 sm:grid-cols-3">
        <div className="rounded-xl bg-background/35 p-3">
          <div className="text-xs text-muted-foreground">{t("可用额度")}</div>
          <div className="mt-1 text-lg font-semibold">
            {formatCreditMicros(summary.wallet?.availableCreditMicros ?? user.wallet?.availableCreditMicros)}
          </div>
        </div>
        <div className="rounded-xl bg-background/35 p-3">
          <div className="text-xs text-muted-foreground">{t("今日 Token")}</div>
          <div className="mt-1 text-lg font-semibold">
            {formatTokenAmount(summary.usageToday.totalTokens)}
          </div>
        </div>
        <div className="rounded-xl bg-background/35 p-3">
          <div className="text-xs text-muted-foreground">{t("成功率 / 费用")}</div>
          <div className="mt-1 text-lg font-semibold">
            {successRate} · {formatUsd(summary.usageToday.estimatedCostUsd)}
          </div>
        </div>
      </div>

      <div>
        <div className="mb-2 flex items-center gap-2 text-sm font-semibold">
          <LineChart className="h-4 w-4 text-primary" />
          {t("Token 消耗曲线")}
        </div>
        <UserUsageTrendLine summary={summary} />
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <div className="rounded-xl bg-background/25 p-3">
          <div className="mb-2 flex items-center gap-2 text-sm font-semibold">
            <KeyRound className="h-4 w-4 text-primary" />
            {t("Key 消耗明细")}
          </div>
          <div className="space-y-2">
            {summary.topKeys.length === 0 ? (
              <Empty className="min-h-20 border bg-muted/20 p-3">
                <EmptyHeader>
                  <EmptyTitle>{t("暂无 Key 用量")}</EmptyTitle>
                </EmptyHeader>
              </Empty>
            ) : (
              summary.topKeys.map((item) => (
                <div
                  key={item.keyId}
                  className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 text-xs"
                >
                  <div className="min-w-0">
                    <div className="truncate font-medium">{item.name || item.keyId}</div>
                    <div className="truncate text-muted-foreground">{item.modelSlug || "auto"}</div>
                  </div>
                  <div className="text-right font-semibold">
                    {formatTokenAmount(item.todayTokens || item.totalTokens)}
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
        <div className="rounded-xl bg-background/25 p-3">
          <div className="mb-2 flex items-center gap-2 text-sm font-semibold">
            <BarChart3 className="h-4 w-4 text-primary" />
            {t("模型消耗明细")}
          </div>
          <div className="space-y-2">
            {summary.topModels.length === 0 ? (
              <Empty className="min-h-20 border bg-muted/20 p-3">
                <EmptyHeader>
                  <EmptyTitle>{t("暂无模型用量")}</EmptyTitle>
                </EmptyHeader>
              </Empty>
            ) : (
              summary.topModels.map((item) => (
                <div
                  key={item.model}
                  className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 text-xs"
                >
                  <div className="truncate font-mono font-medium">{item.model}</div>
                  <div className="text-right font-semibold">
                    {formatTokenAmount(item.totalTokens)}
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      </div>

      <div className="rounded-xl bg-background/25 p-3">
        <div className="mb-2 text-sm font-semibold">{t("近期请求上下文")}</div>
        {summary.recentLogs.length === 0 ? (
          <Empty className="min-h-20 border bg-muted/20 p-3">
            <EmptyHeader>
              <EmptyTitle>{t("暂无请求日志")}</EmptyTitle>
            </EmptyHeader>
          </Empty>
        ) : (
          <div className="divide-y divide-border/40">
            {summary.recentLogs.slice(0, 5).map((log) => (
              <div
                key={log.id}
                className="grid gap-2 py-2 text-xs sm:grid-cols-[minmax(0,1fr)_auto]"
              >
                <div className="min-w-0">
                  <div className="truncate font-mono font-medium">{log.model || "unknown"}</div>
                  <div className="truncate text-muted-foreground">{formatTime(log.createdAt)}</div>
                </div>
                <div className="flex gap-3 text-muted-foreground sm:justify-end">
                  <span>{log.statusCode || "-"}</span>
                  <span>{formatTokenAmount(log.totalTokens)}</span>
                  <span>{formatUsd(log.estimatedCostUsd)}</span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default function AccountManagerPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const isPageActive = useDesktopPageActive("/account-manager/");
  const shouldQuery =
    useDeferredDesktopActivation(canAccessManagementRpc) && isPageActive;
  const [createDialogOpen, setCreateDialogOpen] = useState(false);
  const [topUpUserId, setTopUpUserId] = useState<string | null>(null);
  const [editUserId, setEditUserId] = useState<string | null>(null);
  const [usageUserId, setUsageUserId] = useState<string | null>(null);
  const [createDraft, setCreateDraft] = useState({
    username: "",
    displayName: "",
    password: "",
    role: "member",
    initialBalance: "0",
  });
  const [topUpDraft, setTopUpDraft] = useState({
    amount: "1",
    note: "",
  });
  const [editDraft, setEditDraft] = useState({
    displayName: "",
    role: "member",
    status: "active",
    password: "",
  });

  const statusQuery = useQuery<AccountManagerStatus>({
    queryKey: ACCOUNT_MANAGER_QUERY_KEYS.status,
    queryFn: () => appClient.getAccountManagerStatus(),
    enabled: shouldQuery,
  });
  const usersQuery = useQuery<AppUser[]>({
    queryKey: ACCOUNT_MANAGER_QUERY_KEYS.users,
    queryFn: () => appClient.listAppUsers(),
    enabled: shouldQuery,
  });
  const usageDetailQuery = useQuery<MemberDashboardSummary>({
    queryKey: ["account-manager", "user-usage", usageUserId],
    queryFn: () => dashboardClient.getMemberSummary({ userId: usageUserId }),
    enabled: shouldQuery && Boolean(usageUserId),
    retry: 1,
  });

  usePageTransitionReady(
    "/account-manager/",
    !canAccessManagementRpc ||
      statusQuery.isFetched ||
      statusQuery.isError ||
      !isPageActive,
  );

  const users = usersQuery.data ?? [];
  const usersById = useMemo(
    () => new Map(users.map((user) => [user.id, user])),
    [users],
  );
  const walletUsers = useMemo(
    () => users.filter((user) => userCanOwnWallet(user)),
    [users],
  );
  const status = statusQuery.data;
  const topUpUser = topUpUserId ? usersById.get(topUpUserId) ?? null : null;
  const editUser = editUserId ? usersById.get(editUserId) ?? null : null;
  const usageUser = usageUserId ? usersById.get(usageUserId) ?? null : null;

  const refreshAll = async () => {
    await Promise.all([
      queryClient.invalidateQueries({
        queryKey: ACCOUNT_MANAGER_QUERY_KEYS.status,
      }),
      queryClient.invalidateQueries({
        queryKey: ACCOUNT_MANAGER_QUERY_KEYS.users,
      }),
      queryClient.invalidateQueries({
        queryKey: ["account-manager", "user-usage"],
      }),
      queryClient.invalidateQueries({
        queryKey: ["account-manager", "api-key-owners"],
      }),
      queryClient.invalidateQueries({ queryKey: APP_SESSION_QUERY_KEY }),
    ]);
  };

  const createUser = useMutation({
    mutationFn: async () => {
      const username = createDraft.username.trim();
      const password = createDraft.password;
      if (!username) throw new Error("请输入用户名");
      if (!password) throw new Error("请输入初始密码");
      const creatingAdmin = createDraft.role === "admin";
      const initialBalanceCreditMicros = creatingAdmin
        ? null
        : parseCreditInput(createDraft.initialBalance);
      if (!creatingAdmin && initialBalanceCreditMicros === null) {
        throw new Error("初始额度必须是非负数字");
      }
      return appClient.createAppUser({
        username,
        password,
        displayName: createDraft.displayName.trim() || null,
        role: createDraft.role,
        initialBalanceCreditMicros,
      });
    },
    onSuccess: async () => {
      setCreateDraft({
        username: "",
        displayName: "",
        password: "",
        role: "member",
        initialBalance: "0",
      });
      setCreateDialogOpen(false);
      await refreshAll();
      toast.success(t("账号已创建"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("创建失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const topUpWallet = useMutation({
    mutationFn: async () => {
      if (!topUpUser || !userCanOwnWallet(topUpUser)) {
        throw new Error("请选择可分发成员");
      }
      const amountCreditMicros = parseCreditInput(topUpDraft.amount);
      if (!amountCreditMicros || amountCreditMicros <= 0) {
        throw new Error("充值额度必须大于 0");
      }
      return appClient.topUpWallet({
        ownerKind: "user",
        ownerId: topUpUser.id,
        amountCreditMicros,
        note: topUpDraft.note.trim() || null,
      });
    },
    onSuccess: async () => {
      setTopUpUserId(null);
      setTopUpDraft({ amount: "1", note: "" });
      await refreshAll();
      toast.success(t("钱包已充值"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("充值失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const updateUser = useMutation({
    mutationFn: async () => {
      if (!editUser) throw new Error("请选择要编辑的账号");
      const password = editDraft.password.trim();
      return appClient.updateAppUser({
        id: editUser.id,
        displayName: editDraft.displayName.trim() || null,
        role: editDraft.role,
        status: editDraft.status,
        password: password || null,
      });
    },
    onSuccess: async () => {
      setEditUserId(null);
      setEditDraft({
        displayName: "",
        role: "member",
        status: "active",
        password: "",
      });
      await refreshAll();
      toast.success(t("账号已更新"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("更新失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const handleCreateUser = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    createUser.mutate();
  };

  const handleTopUp = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    topUpWallet.mutate();
  };

  const handleUpdateUser = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    updateUser.mutate();
  };

  const openTopUpDialog = (user: AppUser) => {
    setTopUpUserId(user.id);
    setTopUpDraft({ amount: "1", note: "" });
  };

  const openEditDialog = (user: AppUser) => {
    setEditUserId(user.id);
    setEditDraft({
      displayName: user.displayName || "",
      role: user.role === "admin" ? "admin" : "member",
      status: user.status === "disabled" ? "disabled" : "active",
      password: "",
    });
  };

  const isRefreshing = statusQuery.isFetching || usersQuery.isFetching;

  return (
    <div className="container mx-auto space-y-6 p-6">
      <div className="flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">{t("账号管理")}</h1>
          <p className="mt-2 text-sm text-muted-foreground">
            {t("管理 Web 登录成员和额度分发钱包。平台 Key 归属在平台密钥中配置。")}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            className="glass-card h-10 gap-2 rounded-xl px-3 shadow-sm"
            disabled={!canAccessManagementRpc || isRefreshing}
            onClick={() => void refreshAll()}
          >
            <RefreshCw className={cn("h-4 w-4", isRefreshing && "animate-spin")} />
            {t("刷新")}
          </Button>
          <Button
            className="h-10 gap-2 shadow-sm shadow-primary/20"
            disabled={!canAccessManagementRpc}
            onClick={() => setCreateDialogOpen(true)}
          >
            <Plus className="h-4 w-4" />
            {t("新建账号")}
          </Button>
        </div>
      </div>

      {!canAccessManagementRpc ? (
        <Card className="glass-card shadow-sm">
          <CardContent className="flex items-start gap-3 py-5">
            <AlertCircle className="mt-0.5 h-5 w-5 text-destructive" />
            <div className="space-y-1">
              <p className="font-medium">{t("当前 Web 运行方式不受支持")}</p>
              <p className="text-sm text-muted-foreground">
                {t("请通过 CodexManager Web 运行壳访问账号管理。")}
              </p>
            </div>
          </CardContent>
        </Card>
      ) : null}

      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <StatCard
          title={t("登录模式")}
          value={modeLabel(status?.mode || "none")}
          detail={
            status?.appUsersConfigured
              ? t("账号系统已初始化")
              : t("账号系统未初始化")
          }
          icon={ShieldCheck}
        />
        <StatCard
          title={t("额度分发")}
          value={status?.distributionEnabled ? t("已开启") : t("未开启")}
          detail={t("开启后平台 Key 会按归属钱包扣减额度")}
          icon={WalletCards}
        />
        <StatCard
          title={t("管理员")}
          value={String(status?.activeAdminCount ?? 0)}
          detail={t("当前启用的管理员账号数量")}
          icon={UsersRound}
        />
        <StatCard
          title={t("可分发成员")}
          value={String(walletUsers.length)}
          detail={t("不包含管理员账号")}
          icon={UserPlus}
        />
      </div>

      <Card className="glass-card overflow-hidden py-0 shadow-sm">
        <CardHeader className="border-b bg-background/35 py-4">
          <CardTitle>{t("登录账号")}</CardTitle>
          <CardDescription>
            {t("管理员只负责控制面管理；成员才参与额度分发和平台 Key 消费。")}
          </CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="px-4">{t("用户名")}</TableHead>
                <TableHead>{t("角色")}</TableHead>
                <TableHead>{t("状态")}</TableHead>
                <TableHead>{t("可用额度")}</TableHead>
                <TableHead>{t("最后登录")}</TableHead>
                <TableHead>{t("账号 ID")}</TableHead>
                <TableHead className="pr-4 text-right">{t("操作")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {usersQuery.isLoading ? (
                <TableRow>
                  <TableCell colSpan={7} className="h-24 text-center">
                    {t("读取中...")}
                  </TableCell>
                </TableRow>
              ) : users.length === 0 ? (
                <TableRow>
                  <TableCell
                    colSpan={7}
                    className="h-24 text-center text-muted-foreground"
                  >
                    <Empty className="min-h-20 border-0 bg-transparent">
                      <EmptyHeader>
                        <EmptyTitle>{t("暂无登录账号")}</EmptyTitle>
                      </EmptyHeader>
                    </Empty>
                  </TableCell>
                </TableRow>
              ) : (
                users.map((user) => (
                  <TableRow key={user.id}>
                    <TableCell className="px-4">
                      <div className="flex flex-col gap-1">
                        <span className="font-medium">{user.username}</span>
                        {user.displayName ? (
                          <span className="text-xs text-muted-foreground">
                            {user.displayName}
                          </span>
                        ) : null}
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge
                        variant={user.role === "admin" ? "default" : "secondary"}
                      >
                        {roleLabel(user.role)}
                      </Badge>
                    </TableCell>
                    <TableCell>{statusLabel(user.status)}</TableCell>
                    <TableCell>
                      {isAdminUser(user) ? (
                        <Badge variant="outline">{t("不参与分发")}</Badge>
                      ) : (
                        formatCreditMicros(user.wallet?.availableCreditMicros)
                      )}
                    </TableCell>
                    <TableCell>{formatTime(user.lastLoginAt)}</TableCell>
                    <TableCell className="max-w-[180px] truncate font-mono text-xs text-muted-foreground">
                      {user.id}
                    </TableCell>
                    <TableCell className="pr-4 text-right">
                      <div className="flex flex-wrap justify-end gap-2">
                        <Button
                          variant="outline"
                          size="sm"
                          disabled={!canAccessManagementRpc || !userCanOwnWallet(user)}
                          onClick={() => openTopUpDialog(user)}
                        >
                          {t("充值")}
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="gap-1"
                          disabled={!canAccessManagementRpc}
                          onClick={() => setUsageUserId(user.id)}
                        >
                          <LineChart className="h-3.5 w-3.5" />
                          {t("用量")}
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="gap-1"
                          disabled={!canAccessManagementRpc}
                          onClick={() => openEditDialog(user)}
                        >
                          <Pencil className="h-3.5 w-3.5" />
                          {t("编辑")}
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Dialog open={createDialogOpen} onOpenChange={setCreateDialogOpen}>
        <DialogContent className="glass-card sm:max-w-[560px]">
          <DialogHeader>
            <DialogTitle>{t("新建登录账号")}</DialogTitle>
            <DialogDescription>
              {t("成员账号可拥有额度钱包；管理员账号只负责管理。")}
            </DialogDescription>
          </DialogHeader>
          <form className="grid gap-4" onSubmit={handleCreateUser}>
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="grid gap-1.5">
                <Label htmlFor="app-user-username">{t("用户名")}</Label>
                <Input
                  id="app-user-username"
                  value={createDraft.username}
                  onChange={(event) =>
                    setCreateDraft((draft) => ({
                      ...draft,
                      username: event.target.value,
                    }))
                  }
                  placeholder="member-one"
                />
              </div>
              <div className="grid gap-1.5">
                <Label htmlFor="app-user-display-name">{t("显示名称")}</Label>
                <Input
                  id="app-user-display-name"
                  value={createDraft.displayName}
                  onChange={(event) =>
                    setCreateDraft((draft) => ({
                      ...draft,
                      displayName: event.target.value,
                    }))
                  }
                  placeholder="Member One"
                />
              </div>
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="app-user-password">{t("初始密码")}</Label>
              <Input
                id="app-user-password"
                type="password"
                value={createDraft.password}
                onChange={(event) =>
                  setCreateDraft((draft) => ({
                    ...draft,
                    password: event.target.value,
                  }))
                }
                placeholder="password123"
              />
            </div>
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="grid gap-1.5">
                <Label>{t("角色")}</Label>
                <Select
                  value={createDraft.role}
                  onValueChange={(value) =>
                    setCreateDraft((draft) => ({
                      ...draft,
                      role: String(value),
                    }))
                  }
                >
                  <SelectTrigger className="w-full">
                    <SelectValue>
                      {(value) => roleLabel(String(value || "member"))}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                    <SelectItem value="member">{t("成员")}</SelectItem>
                    <SelectItem value="admin">{t("管理员")}</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-1.5">
                <Label htmlFor="app-user-initial-balance">{t("初始额度")}</Label>
                <Input
                  id="app-user-initial-balance"
                  inputMode="decimal"
                  value={createDraft.initialBalance}
                  onChange={(event) =>
                    setCreateDraft((draft) => ({
                      ...draft,
                      initialBalance: event.target.value,
                    }))
                  }
                  disabled={createDraft.role === "admin"}
                  placeholder={
                    createDraft.role === "admin" ? t("管理员不设置额度") : "0"
                  }
                />
              </div>
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setCreateDialogOpen(false)}
              >
                {t("取消")}
              </Button>
              <Button
                type="submit"
                className="gap-2"
                disabled={!canAccessManagementRpc || createUser.isPending}
              >
                <UserPlus className="h-4 w-4" />
                {createUser.isPending ? t("创建中...") : t("创建账号")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(editUserId)}
        onOpenChange={(open) => {
          if (!open) {
            setEditUserId(null);
            setEditDraft({
              displayName: "",
              role: "member",
              status: "active",
              password: "",
            });
          }
        }}
      >
        <DialogContent className="glass-card sm:max-w-[560px]">
          <DialogHeader>
            <DialogTitle>{t("编辑登录账号")}</DialogTitle>
            <DialogDescription>
              {editUser ? userSelectLabel(editUser) : t("选择登录账号")}
            </DialogDescription>
          </DialogHeader>
          <form className="grid gap-4" onSubmit={handleUpdateUser}>
            <div className="grid gap-1.5">
              <Label htmlFor="edit-app-user-display-name">{t("显示名称")}</Label>
              <Input
                id="edit-app-user-display-name"
                value={editDraft.displayName}
                onChange={(event) =>
                  setEditDraft((draft) => ({
                    ...draft,
                    displayName: event.target.value,
                  }))
                }
                placeholder={t("可选")}
              />
            </div>
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="grid gap-1.5">
                <Label>{t("角色")}</Label>
                <Select
                  value={editDraft.role}
                  onValueChange={(value) =>
                    setEditDraft((draft) => ({
                      ...draft,
                      role: String(value || "member"),
                    }))
                  }
                >
                  <SelectTrigger className="w-full">
                    <SelectValue>
                      {(value) => roleLabel(String(value || "member"))}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                    <SelectItem value="member">{t("成员")}</SelectItem>
                    <SelectItem value="admin">{t("管理员")}</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-1.5">
                <Label>{t("状态")}</Label>
                <Select
                  value={editDraft.status}
                  onValueChange={(value) =>
                    setEditDraft((draft) => ({
                      ...draft,
                      status: String(value || "active"),
                    }))
                  }
                >
                  <SelectTrigger className="w-full">
                    <SelectValue>
                      {(value) => statusLabel(String(value || "active"))}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                    <SelectItem value="active">{t("启用")}</SelectItem>
                    <SelectItem value="disabled">{t("禁用")}</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </div>
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="edit-app-user-password">{t("重置密码")}</Label>
              <Input
                id="edit-app-user-password"
                type="password"
                value={editDraft.password}
                onChange={(event) =>
                  setEditDraft((draft) => ({
                    ...draft,
                    password: event.target.value,
                  }))
                }
                placeholder={t("不修改则留空")}
              />
              <p className="text-xs text-muted-foreground">
                {t("填写后会替换该账号的登录密码，至少 8 位。")}
              </p>
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setEditUserId(null)}
              >
                {t("取消")}
              </Button>
              <Button
                type="submit"
                className="gap-2"
                disabled={!canAccessManagementRpc || !editUser || updateUser.isPending}
              >
                <Pencil className="h-4 w-4" />
                {updateUser.isPending ? t("保存中...") : t("保存修改")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(usageUserId)}
        onOpenChange={(open) => {
          if (!open) {
            setUsageUserId(null);
          }
        }}
      >
        <DialogContent className="glass-card max-h-[85vh] overflow-y-auto sm:max-w-[760px]">
          <DialogHeader>
            <DialogTitle>{t("成员用量详情")}</DialogTitle>
            <DialogDescription>
              {usageUser ? userSelectLabel(usageUser) : t("选择登录账号")}
            </DialogDescription>
          </DialogHeader>
          {!usageUser ? (
            <div className="rounded-xl bg-background/35 p-4 text-sm text-muted-foreground">
              {t("未找到登录账号")}
            </div>
          ) : usageDetailQuery.isLoading ? (
            <div className="grid gap-3">
              <Skeleton className="h-20 w-full rounded-xl" />
              <Skeleton className="h-44 w-full rounded-xl" />
              <Skeleton className="h-28 w-full rounded-xl" />
            </div>
          ) : usageDetailQuery.isError ? (
            <div className="rounded-xl border border-destructive/30 bg-destructive/10 p-4 text-sm text-destructive">
              {t("用量详情读取失败")}
            </div>
          ) : usageDetailQuery.data ? (
            <UserUsageDetail user={usageUser} summary={usageDetailQuery.data} />
          ) : (
            <Empty className="min-h-28 border bg-background/35">
              <EmptyHeader>
                <EmptyTitle>{t("暂无用量详情")}</EmptyTitle>
              </EmptyHeader>
            </Empty>
          )}
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => setUsageUserId(null)}
            >
              {t("关闭")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(topUpUserId)}
        onOpenChange={(open) => {
          if (!open) {
            setTopUpUserId(null);
            setTopUpDraft({ amount: "1", note: "" });
          }
        }}
      >
        <DialogContent className="glass-card sm:max-w-[480px]">
          <DialogHeader>
            <DialogTitle>{t("钱包充值")}</DialogTitle>
            <DialogDescription>
              {topUpUser
                ? `${t("充值账号")}：${userSelectLabel(topUpUser)}`
                : t("选择可分发成员")}
            </DialogDescription>
          </DialogHeader>
          <form className="grid gap-4" onSubmit={handleTopUp}>
            <div className="grid gap-1.5">
              <Label htmlFor="wallet-top-up-amount">{t("充值额度")}</Label>
              <Input
                id="wallet-top-up-amount"
                inputMode="decimal"
                value={topUpDraft.amount}
                onChange={(event) =>
                  setTopUpDraft((draft) => ({
                    ...draft,
                    amount: event.target.value,
                  }))
                }
                placeholder="1"
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="wallet-top-up-note">{t("备注")}</Label>
              <Input
                id="wallet-top-up-note"
                value={topUpDraft.note}
                onChange={(event) =>
                  setTopUpDraft((draft) => ({
                    ...draft,
                    note: event.target.value,
                  }))
                }
                placeholder={t("可选")}
              />
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setTopUpUserId(null)}
              >
                {t("取消")}
              </Button>
              <Button
                type="submit"
                className="gap-2"
                disabled={
                  !canAccessManagementRpc ||
                  !topUpUser ||
                  topUpWallet.isPending
                }
              >
                <WalletCards className="h-4 w-4" />
                {topUpWallet.isPending ? t("充值中...") : t("确认充值")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
