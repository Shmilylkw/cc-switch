import React from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  AlertTriangle,
  Loader2,
  LogIn,
  LogOut,
  Mail,
  RefreshCw,
} from "lucide-react";
import { useGeminiOauth } from "./hooks/useGeminiOauth";

interface GeminiOAuthSectionProps {
  className?: string;
}

/**
 * Google Official (Gemini) OAuth 认证区块
 *
 * Gemini CLI 使用浏览器回环 OAuth 流程（非设备码），登录由 CLI 在终端里完成，
 * 本区块只负责检测登录状态并拉起 `gemini auth login`。
 */
export const GeminiOAuthSection: React.FC<GeminiOAuthSectionProps> = ({
  className,
}) => {
  const { t } = useTranslation();
  const {
    isStatusSuccess,
    isStatusError,
    isAuthenticated,
    email,
    message,
    isLoggingIn,
    isLoggingOut,
    login,
    logout,
    refetchStatus,
  } = useGeminiOauth();

  return (
    <div className={`space-y-4 ${className ?? ""}`}>
      <div className="flex items-center justify-between">
        <Label>{t("geminiOauth.authStatus", "Google OAuth 认证")}</Label>
        <Badge
          variant={isAuthenticated ? "default" : "secondary"}
          className={
            isAuthenticated ? "bg-green-500 hover:bg-green-600" : ""
          }
        >
          {isAuthenticated
            ? t("geminiOauth.authenticated", "已登录")
            : t("geminiOauth.notAuthenticated", "未登录")}
        </Badge>
      </div>

      {isStatusError && (
        <div
          role="alert"
          className="flex items-center gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm text-destructive"
        >
          <AlertTriangle className="h-4 w-4 shrink-0" />
          <span className="min-w-0 flex-1">
            {t(
              "geminiOauth.statusLoadFailed",
              "无法加载 Google 登录状态，请重试。",
            )}
          </span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 shrink-0"
            onClick={() => void refetchStatus()}
          >
            <RefreshCw className="mr-1 h-3.5 w-3.5" />
            {t("common.retry", "重试")}
          </Button>
        </div>
      )}

      {!isStatusSuccess && !isStatusError && (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" />
          {t("geminiOauth.statusLoading", "正在加载...")}
        </div>
      )}

      {isStatusSuccess && isAuthenticated && email && (
        <div className="flex items-center gap-2 rounded-md border bg-muted/30 p-2 text-sm">
          <Mail className="h-4 w-4 shrink-0 text-muted-foreground" />
          <span className="truncate font-medium">{email}</span>
        </div>
      )}

      {isStatusSuccess && !isAuthenticated && message && (
        <p className="text-sm text-muted-foreground">{message}</p>
      )}

      {isStatusSuccess && (
        <Button
          type="button"
          variant="outline"
          className="w-full"
          disabled={isLoggingIn}
          onClick={login}
        >
          {isLoggingIn ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <LogIn className="mr-2 h-4 w-4" />
          )}
          {isAuthenticated
            ? t("geminiOauth.relogin", "重新登录 / 切换账号")
            : t("geminiOauth.login", "使用 Google 登录")}
        </Button>
      )}

      {isStatusSuccess && isAuthenticated && (
        <Button
          type="button"
          variant="outline"
          className="w-full text-red-500 hover:text-red-600"
          disabled={isLoggingOut}
          onClick={logout}
        >
          {isLoggingOut ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <LogOut className="mr-2 h-4 w-4" />
          )}
          {t("geminiOauth.logout", "退出 Google 登录")}
        </Button>
      )}
    </div>
  );
};

export default GeminiOAuthSection;
