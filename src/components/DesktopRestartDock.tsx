import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, RotateCw } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { desktopRestartApi, type DesktopAppTarget } from "@/lib/api";
import { extractErrorMessage } from "@/utils/errorUtils";
import { ClaudeIcon, CodexIcon } from "@/components/BrandIcons";

/**
 * 右下角悬浮的桌面端重启入口。
 *
 * 桌面版（Claude / Codex）只在启动时读一次配置，切换供应商后必须重启才生效，
 * 这里给一键操作，省去用户手动去任务栏杀进程。
 */
export function DesktopRestartDock() {
  const { t } = useTranslation();
  const [pending, setPending] = useState<DesktopAppTarget | null>(null);

  const handleRestart = async (target: DesktopAppTarget, label: string) => {
    if (pending) return;
    setPending(target);
    try {
      const result = await desktopRestartApi.restart(target);
      if (result.wasRunning) {
        toast.success(
          t("restart.done", {
            defaultValue: "{{app}} 已重启",
            app: label,
          }),
        );
      } else {
        toast.success(
          t("restart.started", {
            defaultValue: "{{app}} 未在运行，已启动",
            app: label,
          }),
        );
      }
    } catch (error) {
      toast.error(
        t("restart.failed", {
          defaultValue: "重启 {{app}} 失败：{{error}}",
          app: label,
          error: extractErrorMessage(error) || t("common.unknown"),
        }),
        { duration: 6000 },
      );
    } finally {
      setPending(null);
    }
  };

  const items: Array<{
    target: DesktopAppTarget;
    label: string;
    icon: JSX.Element;
  }> = [
    {
      target: "claude",
      label: t("restart.claudeDesktop", { defaultValue: "Claude 桌面版" }),
      icon: <ClaudeIcon className="h-4 w-4" />,
    },
    {
      target: "codex",
      label: t("restart.codexDesktop", { defaultValue: "Codex 桌面版" }),
      icon: <CodexIcon className="h-4 w-4" />,
    },
  ];

  return (
    <div className="fixed bottom-4 right-4 z-40 flex items-center gap-1 rounded-full border border-border/60 bg-background/85 px-1.5 py-1 shadow-lg backdrop-blur-md">
      <RotateCw className="ml-1 h-3.5 w-3.5 shrink-0 text-muted-foreground" />
      {items.map(({ target, label, icon }) => (
        <Button
          key={target}
          size="icon"
          variant="ghost"
          onClick={() => void handleRestart(target, label)}
          disabled={pending !== null}
          title={t("restart.tooltip", {
            defaultValue: "重启 {{app}}（使配置生效）",
            app: label,
          })}
          className={cn(
            "h-8 w-8 rounded-full",
            pending === target && "text-primary",
          )}
        >
          {pending === target ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            icon
          )}
        </Button>
      ))}
    </div>
  );
}

export default DesktopRestartDock;
