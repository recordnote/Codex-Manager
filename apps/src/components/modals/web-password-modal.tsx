"use client";

import { useEffect, useState } from "react";
import { 
  Dialog, 
  DialogContent, 
  DialogDescription, 
  DialogHeader, 
  DialogTitle,
  DialogFooter
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useAppStore } from "@/lib/store/useAppStore";
import { appClient } from "@/lib/api/app-client";
import { toast } from "sonner";
import { ShieldAlert, ShieldCheck, KeyRound, Trash2 } from "lucide-react";

interface WebPasswordModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function WebPasswordModal({ open, onOpenChange }: WebPasswordModalProps) {
  const { appSettings, setAppSettings } = useAppStore();
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  useEffect(() => {
    if (!open) {
      setPassword("");
      setConfirmPassword("");
      return;
    }

    let cancelled = false;
    const syncSettings = async () => {
      try {
        const settings = await appClient.getSettings();
        if (!cancelled) {
          setAppSettings(settings);
        }
      } catch (err: unknown) {
        if (!cancelled) {
          toast.error(
            `读取密码状态失败: ${err instanceof Error ? err.message : String(err)}`
          );
        }
      }
    };

    void syncSettings();

    return () => {
      cancelled = true;
    };
  }, [open, setAppSettings]);

  const handleSave = async () => {
    if (!password) {
      toast.error("请输入密码");
      return;
    }
    if (password !== confirmPassword) {
      toast.error("两次输入的密码不一致");
      return;
    }

    setIsLoading(true);
    try {
      const settings = await appClient.setSettings({ webAccessPassword: password });
      setAppSettings(settings);
      toast.success("访问密码已设置");
      onOpenChange(false);
      setPassword("");
      setConfirmPassword("");
    } catch (err: unknown) {
      toast.error(`保存失败: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setIsLoading(false);
    }
  };

  const handleClear = async () => {
    setIsLoading(true);
    try {
      const settings = await appClient.setSettings({ webAccessPassword: "" });
      setAppSettings(settings);
      toast.success("访问密码已清除");
      onOpenChange(false);
      setPassword("");
      setConfirmPassword("");
    } catch (err: unknown) {
      toast.error(`清除失败: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <div className="flex items-center gap-3 mb-2">
            <div className="p-2 rounded-full bg-primary/10">
              <KeyRound className="h-5 w-5 text-primary" />
            </div>
            <DialogTitle>访问密码</DialogTitle>
          </div>
          <DialogDescription>
            该密码用于保护 Web 管理页访问。在桌面端或 Web 端修改后，都会写入同一份服务配置并立即生效。
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4 py-4">
          {appSettings.webAccessPasswordConfigured ? (
            <div className="flex items-center gap-3 p-3 rounded-lg bg-green-500/10 border border-green-500/20 text-green-600 dark:text-green-400 text-sm">
              <ShieldCheck className="h-4 w-4" />
              <span>当前已启用访问密码保护</span>
            </div>
          ) : (
            <div className="flex items-center gap-3 p-3 rounded-lg bg-yellow-500/10 border border-yellow-500/20 text-yellow-600 dark:text-yellow-400 text-sm">
              <ShieldAlert className="h-4 w-4" />
              <span>当前未设置访问密码，Web 管理页处于公开状态</span>
            </div>
          )}

          <div className="grid gap-2">
            <Label htmlFor="password">新密码</Label>
            <Input 
              id="password" 
              type="password" 
              placeholder="请输入新密码"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="confirm">确认新密码</Label>
            <Input 
              id="confirm" 
              type="password" 
              placeholder="请再次输入新密码"
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
            />
          </div>
        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          {appSettings.webAccessPasswordConfigured && (
            <Button variant="ghost" onClick={handleClear} disabled={isLoading} className="text-destructive hover:text-destructive hover:bg-destructive/10">
              <Trash2 className="h-4 w-4 mr-2" /> 清除密码
            </Button>
          )}
          <Button onClick={handleSave} disabled={isLoading}>
            {isLoading ? "保存中..." : "保存设置"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
