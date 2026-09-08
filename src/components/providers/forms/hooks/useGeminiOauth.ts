import { useCallback } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { geminiOAuthApi } from "@/lib/api";

export function useGeminiOauth() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  const queryKey = ["gemini-oauth-status"];

  const {
    data: status,
    isLoading: isLoadingStatus,
    isSuccess: isStatusSuccess,
    isError: isStatusError,
    refetch: refetchStatus,
  } = useQuery({
    queryKey,
    queryFn: () => geminiOAuthApi.geminiAuthStatus(),
    staleTime: 10_000,
    // The login completes in a separate terminal, not inside the app, so poll
    // periodically so the Auth Center flips to "authenticated" without a
    // manual refresh.
    refetchInterval: 10_000,
  });

  const loginMutation = useMutation({
    mutationFn: () => geminiOAuthApi.geminiAuthLogin(),
    onSuccess: () => {
      toast.success(
        t("geminiOauth.loginLaunched", {
          defaultValue: "已打开终端，请在浏览器中完成 Google 登录",
        }),
      );
    },
    onError: (e) => {
      console.error("[GeminiOauth] Failed to launch login:", e);
      toast.error(
        t("geminiOauth.loginFailed", {
          defaultValue: "启动登录失败，请确认已安装 Gemini CLI",
        }),
      );
    },
  });

  const logoutMutation = useMutation({
    mutationFn: () => geminiOAuthApi.geminiAuthLogout(),
    onSuccess: async () => {
      queryClient.setQueryData(queryKey, {
        authenticated: false,
        email: null,
        message: null,
      });
      await queryClient.invalidateQueries({ queryKey });
      toast.success(
        t("geminiOauth.loggedOut", { defaultValue: "已退出 Google 登录" }),
      );
    },
    onError: (e) => {
      console.error("[GeminiOauth] Failed to logout:", e);
      toast.error(
        t("geminiOauth.logoutFailed", { defaultValue: "退出登录失败" }),
      );
    },
  });

  const login = useCallback(() => loginMutation.mutate(), [loginMutation]);
  const logout = useCallback(() => logoutMutation.mutate(), [logoutMutation]);

  return {
    status,
    isLoadingStatus,
    isStatusSuccess,
    isStatusError,
    isAuthenticated: status?.authenticated ?? false,
    email: status?.email ?? null,
    message: status?.message ?? null,
    isLoggingIn: loginMutation.isPending,
    isLoggingOut: logoutMutation.isPending,
    login,
    logout,
    refetchStatus,
  };
}
